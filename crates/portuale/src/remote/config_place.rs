//! Placing the client's config root on the server.

use crate::remote::*;

// --- Resolve-path support ------------------------------------------------------

/// Temporarily point `PORTAGE_CONFIGROOT` at `dir` for a resolve run,
/// restoring the previous value on drop. `unsafe` because
/// `std::env::set_var` is (edition 2024):
/// SAFETY: the guarded region is `mrg`'s synchronous resolve path --
/// binary-only, so no build threads spawn, and nothing else in the
/// process writes this variable concurrently.
pub(crate) struct ConfigRootOverride {
    pub(crate) previous: Option<std::ffi::OsString>,
}

impl Drop for ConfigRootOverride {
    fn drop(&mut self) {
        // SAFETY: see the struct doc comment.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                None => std::env::remove_var("PORTAGE_CONFIGROOT"),
            }
        }
    }
}

/// A placed `/etc/portage` config root for a resolve run: `Server`
/// paths are used directly, `Client` trees are pulled once into a temp
/// dir re-rooted as `<tmp>/etc/portage` (a valid `PORTAGE_CONFIGROOT`)
/// with `ConfigRootOverride` held for the whole resolve. The pulled temp
/// dir is removed best-effort on drop; the pulled tree additionally
/// carries the server's own `make.globals` seed plus a remapped
/// `make.profile` (see `place_config_root`, `remap_client_make_profile`).
/// Shared by `run_remote_resolve` and `run_bundle_stage`.
pub(crate) struct PlacedConfig {
    /// The dir to resolve from (the server path, or the pulled temp root).
    pub(crate) dir: std::path::PathBuf,
    pub(crate) _config_guard: ConfigRootOverride,
    /// Temp dir holding a pulled client tree (`None` for `Server`).
    pub(crate) tmp: Option<std::path::PathBuf>,
}

impl Drop for PlacedConfig {
    fn drop(&mut self) {
        if let Some(tmp) = &self.tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
    }
}

/// Lexically normalize an absolute path (fold `.`/`..` without touching
/// the filesystem -- the client tree isn't here, only pulled symlink text).
pub(crate) fn lexical_normalize(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

/// Resolve a pulled client `make.profile` symlink against the server's
/// repos: only dangling symlinks whose target text names a repo profile
/// (a `/profiles/` component) are rewritten -- to the same-named server
/// repo's profile, else the first server repo providing it. Anything else
/// is left alone; nothing found is a hard `mrg: !!! …` error naming the
/// target, never a silent empty chain.
// Portage: package/ebuild/config.py, make.profile location warning
pub(crate) fn remap_client_make_profile(
    tmp: &std::path::Path,
    client_etc_portage: &str,
    server_config_root: &std::path::Path,
) -> Result<(), String> {
    let link = tmp.join("etc/portage/make.profile");
    let meta = match link.symlink_metadata() {
        Ok(meta) => meta,
        Err(_) => return Ok(()),
    };
    if !meta.file_type().is_symlink() {
        return Ok(());
    }
    // Already resolving inside the pulled tree: leave it -- only
    // dangling links need the server-repo mapping.
    if link.is_dir() {
        return Ok(());
    }
    // The link target *text*, never followed on the client; relative
    // targets resolve against the client's own `/etc/portage`.
    let target = std::fs::read_link(&link)
        .map_err(|e| format!("mrg: reading the pulled client make.profile link: {e}"))?;
    let target_text = target.display().to_string();
    let client_abs = if target.is_absolute() {
        target.clone()
    } else {
        lexical_normalize(&std::path::Path::new(client_etc_portage).join(&target))
    };
    let client_abs_text = client_abs.to_string_lossy().into_owned();
    let Some(split) = client_abs_text.find("/profiles/") else {
        return Ok(());
    };
    let prefix = lexical_normalize(std::path::Path::new(&client_abs_text[..split]));
    let rel = client_abs_text[split + "/profiles/".len()..].to_string();
    if rel.is_empty() {
        return Err(format!(
            "mrg: !!! the pulled client make.profile points at {target_text}, which names no profile under profiles/"
        ));
    }
    // The pulled client `repos.conf` (if any) names the repo whose
    // `location` is the link prefix; the same-named server repo
    // provides the profile.
    let client_repo_name: Option<String> = if tmp
        .join("etc/portage/repos.conf")
        .symlink_metadata()
        .is_ok()
    {
        portage_repo::find_repos(tmp).ok().and_then(|repos| {
            repos.into_iter().find_map(|repo| {
                (lexical_normalize(&repo.location) == prefix).then(|| repo.name.clone())
            })
        })
    } else {
        None
    };
    let server_repos = portage_repo::find_repos(server_config_root).map_err(|e| {
        format!(
            "mrg: !!! the pulled client make.profile points at {target_text}, and the server repos cannot be listed: {e}"
        )
    })?;
    // The same-named server repo first, then every server repo in
    // priority order with main first: the first existing
    // `<location>/profiles/<rel>` wins.
    let mut ordered: Vec<&portage_repo::RepoConfig> = Vec::new();
    if let Some(name) = client_repo_name.as_deref() {
        ordered.extend(server_repos.iter().filter(|repo| repo.name == name));
    }
    ordered.extend(server_repos.iter().filter(|repo| {
        client_repo_name
            .as_deref()
            .is_none_or(|name| repo.name != name)
    }));
    for repo in ordered {
        let candidate = repo.location.join("profiles").join(&rel);
        if candidate.is_dir() {
            std::fs::remove_file(&link)
                .map_err(|e| format!("mrg: replacing the pulled client make.profile link: {e}"))?;
            std::os::unix::fs::symlink(&candidate, &link).map_err(|e| {
                format!(
                    "mrg: pointing the pulled client make.profile at {}: {e}",
                    candidate.display()
                )
            })?;
            return Ok(());
        }
    }
    // Nothing found -- fail loudly, naming the target.
    let name_note = client_repo_name
        .as_deref()
        .map_or(String::new(), |name| format!(" for client repo {name:?}"));
    Err(format!(
        "mrg: !!! the pulled client make.profile points at {target_text}: no server repo provides profiles/{rel}{name_note} ({} server repo(s) checked)",
        server_repos.len()
    ))
}
