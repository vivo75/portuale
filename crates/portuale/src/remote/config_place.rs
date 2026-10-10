//! Placing the client's config root on the server.

use crate::remote::*;

// --- Resolve-path support (slice 5) --------------------------------------------

/// Temporarily point `PORTAGE_CONFIGROOT` at `dir` (pulled client config
/// or an explicit server path) for a resolve run, restoring the previous
/// value on drop. `unsafe` because `std::env::set_var` is (edition 2024):
/// SAFETY: the guarded region is `mrg`'s synchronous resolve path --
/// binary-only, so no build threads spawn, and every env read inside
/// observes one constant value; nothing else in the process writes this
/// variable concurrently.
pub(crate) struct ConfigRootOverride {
    pub(crate) previous: Option<std::ffi::OsString>,
}

impl ConfigRootOverride {
    pub(crate) fn set(dir: &std::path::Path) -> Self {
        let previous = std::env::var_os("PORTAGE_CONFIGROOT");
        // SAFETY: see the struct doc comment.
        unsafe {
            std::env::set_var("PORTAGE_CONFIGROOT", dir);
        }
        Self { previous }
    }
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
/// dir re-rooted as `<tmp>/etc/portage` (a valid `PORTAGE_CONFIGROOT`,
/// real-root layout) with `ConfigRootOverride` held for the whole
/// resolve. The pulled temp dir is removed best-effort on drop, once
/// the resolve is done (backlog #171c review: both the resolve site
/// and `--remote-binpkg` leaked one
/// `/tmp/portuale-remote-etc-<pid>-<nanos>` per run). The pulled tree
/// additionally carries the server's own `make.globals` seeded under
/// `<tmp>/usr/share/portage/config/` (backlog #171 follow-up 3) -- the
/// pull only carries the client's `/etc/portage` contents, so without
/// the seed the resolve would stack no base layer at all. Shared by
/// `run_remote_resolve` and `run_bundle_stage` -- one placement match,
/// no duplicated pull logic. A dangling pulled `make.profile` symlink is
/// additionally remapped onto the server's repos
/// (`remap_client_make_profile`, backlog #171 follow-up 4) -- same
/// helper, so both paths resolve the client profile the same way.
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
/// the filesystem -- the client tree isn't here, only pulled symlink
/// text, so `canonicalize` would resolve the wrong machine's paths).
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
/// repos (backlog #171 follow-up 4, owner Q10 = b: the client may hold no
/// repo at all, so a symlink into the client's repo path dangles here and
/// the profile chain would silently resolve to `[]`).
///
/// Only dangling symlinks whose target text names a repo profile (a
/// `/profiles/` component in the client-absolute target) are rewritten:
/// to the same-named server repo's `<location>/profiles/<rel>` when the
/// pulled client `repos.conf` (if any) has a repo whose `location` is the
/// link prefix, else to the first server repo -- in `find_repos` priority
/// order, main repo first -- providing `<location>/profiles/<rel>`. A
/// link that already resolves inside the pulled tree (e.g. a relative
/// link to a custom profile shipped under `/etc/portage`, or a loopback
/// absolute link), a directory `make.profile` (its `parent` entries ride
/// the normal profile machinery), a missing one, and a target naming no
/// repo profile are all left alone. Nothing found is a hard `mrg: !!! …`
/// error naming the target, never a silent empty chain.
///
/// Real grounding (`3rdparty/portage` = portage-3.0.82.2): the profile
/// root is `<config_root>/etc/make.profile` followed as a link
/// (`package/ebuild/_config/LocationsManager.py:119-149`, `realpath`ed
/// at `:154-157` for repo matching), and real warns it "should point
/// into a profile within $PORTDIR/profiles/"
/// (`package/ebuild/config.py:1444-1450`) -- the rewrite keeps exactly
/// that shape, server-side, then reuses the existing resolver.
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
    // Already resolving server-side (a shipped in-tree profile, or a
    // loopback absolute link): leave it -- only dangling links need the
    // server-repo mapping.
    if link.is_dir() {
        return Ok(());
    }
    // The link target *text* -- never followed on the client (the pull is
    // tar-only); relative targets resolve against the client's own
    // `/etc/portage` dir, lexically.
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
    // Step 2: the pulled client `repos.conf` (if any) names the repo
    // whose `location` is the link prefix; the same-named server repo
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
    // The same-named server repo first (a step-2 hit), then every server
    // repo in priority order with main first (`find_repos` already sorts
    // that way): the first `<location>/profiles/<rel>` that exists wins.
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
    // Step 5: nothing found -- fail loudly, naming the target.
    let name_note = client_repo_name
        .as_deref()
        .map_or(String::new(), |name| format!(" for client repo {name:?}"));
    Err(format!(
        "mrg: !!! the pulled client make.profile points at {target_text}: no server repo provides profiles/{rel}{name_note} ({} server repo(s) checked)",
        server_repos.len()
    ))
}

/// Place `/etc/portage` per `ConfigPlacement` (see `PlacedConfig`).
/// `Err` is a pull failure (already a full `mrg: …` line); callers
/// print it and exit 1, like the resolve path's own errors.
pub(crate) fn place_config_root(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<PlacedConfig, String> {
    // Backlog #171 follow-up 3 (owner Q9 = b): the server's own
    // `make.globals` -- located exactly the way the local path locates
    // it (`resolve_config` reads
    // `<config_root>/usr/share/portage/config/make.globals`, which is
    // the live `/usr/share/portage/config/make.globals` for a `/` run;
    // real sources it from `global_config_path` regardless of
    // `config_root`, `config.py:446-499,569` +
    // `_config/LocationsManager.py:413-415`). Read before the
    // `ConfigRootOverride` below repoints `PORTAGE_CONFIGROOT` at the
    // placed dir, so this still names the server file. `Server`
    // placements need no seed (the placed root is server-side already);
    // the pulled `Client` tree gets it as the resolve's bottom layer,
    // under the client profile chain and the pulled `make.conf` --
    // real's `config` stacking order. Absent on the server (tests):
    // contributes nothing, deterministically.
    let server_config_root = portage_repo::config_root_from_env();
    let server_globals = server_config_root.join("usr/share/portage/config/make.globals");
    let (dir, tmp): (std::path::PathBuf, Option<std::path::PathBuf>) = match &ctx.etc_portage {
        ConfigPlacement::Server(path) => (std::path::PathBuf::from(path), None),
        ConfigPlacement::Client(path) => {
            let tmp = TempDir::new("portuale-remote-etc").keep();
            // The pulled tree is the *contents* of the client's
            // `/etc/portage`; re-root it as `<tmp>/etc/portage` so the
            // dir is a valid `PORTAGE_CONFIGROOT` (real-root layout).
            let pulled_portage = tmp.join("etc/portage");
            if let Err(message) = pull_dir(ctx, control, path, &pulled_portage) {
                // Best-effort: don't leave a partial pull behind.
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(message);
            }
            if server_globals.is_file() {
                let dest = tmp.join("usr/share/portage/config/make.globals");
                if let Some(parent) = dest.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(format!(
                        "mrg: staging the server make.globals under {}: {e}",
                        tmp.display()
                    ));
                }
                if let Err(e) = std::fs::copy(&server_globals, &dest) {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(format!(
                        "mrg: staging the server make.globals under {}: {e}",
                        tmp.display()
                    ));
                }
            }
            // Backlog #171 follow-up 4 (owner Q10 = b): the pulled
            // `make.profile` symlink dangles when the client holds its
            // repos elsewhere (or none at all) -- resolve its target
            // text against the server's repos before the
            // `ConfigRootOverride` below repoints `PORTAGE_CONFIGROOT`
            // at the pulled root (server repos must be listed from the
            // server root, like the seed above). `Server` placements
            // are untouched.
            if let Err(message) = remap_client_make_profile(&tmp, path, &server_config_root) {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(message);
            }
            // Backlog #171 follow-up 5 (l171f; review of l171d/l171e):
            // the pulled client tree may hold no `repos.conf` at all
            // (owner Q10: the client operates without the gentoo
            // repository) while the server holds the repos -- but
            // `find_repos` on the placed root fails `NoReposConf`
            // without the user `etc/portage/repos.conf` path, so both
            // resolving paths hard-errored after a successful remap.
            // Seed the server's own global `repos.conf` (read from the
            // server root like the `make.globals` seed above, before
            // the `ConfigRootOverride`), but only when the pulled
            // client tree has none: a client `repos.conf`, if present,
            // still wins -- real layers the global file before the
            // user's in one parser, so the user wins per key
            // (`repository/config.py:1488-1508`), and the user slot is
            // never clobbered here. Absent on the server (or a
            // directory rather than a file): contributes nothing, and
            // the loud `NoReposConf` error is kept -- never a silent
            // empty repo set. Same file-copy mechanism (with tmp
            // cleanup) as the `make.globals` seed.
            let server_repos_conf = server_config_root.join("usr/share/portage/config/repos.conf");
            if tmp
                .join("etc/portage/repos.conf")
                .symlink_metadata()
                .is_err()
                && server_repos_conf.is_file()
                && let Err(e) =
                    std::fs::copy(&server_repos_conf, tmp.join("etc/portage/repos.conf"))
            {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(format!(
                    "mrg: staging the server repos.conf under {}: {e}",
                    tmp.display()
                ));
            }
            (tmp.clone(), Some(tmp.clone()))
        }
    };
    let guard = ConfigRootOverride::set(&dir);
    Ok(PlacedConfig {
        dir,
        _config_guard: guard,
        tmp,
    })
}
