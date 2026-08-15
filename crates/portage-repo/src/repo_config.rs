//! repos.conf discovery and per-repository configuration.

use crate::*;

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub location: PathBuf,
    pub priority: i32,
    /// Whether this is `repos.conf`'s `[DEFAULT] main-repo` -- needed by
    /// callers that read repo-level config keyed by repo identity.
    pub is_main: bool,
    /// Resolved `masters` chain, in declaration order, as absolute repo
    /// locations. An explicit `repos.conf` `masters =` wins; else
    /// `layout.conf`'s; else the main repo alone (empty for the main repo
    /// itself). An unknown master name is silently dropped.
    /// Portage: repository/config.py, RepoConfigLoader.
    pub masters: Vec<PathBuf>,
    /// `metadata/layout.conf`'s `profile-formats`, verbatim. Portuale acts
    /// on one value only: `portage-2` allows a profile `parent` line's
    /// cross-repo syntax; absent means disallowed (a documented
    /// simplification).
    /// Portage: LocationsManager's `_allow_parent_colon` gate.
    pub profile_formats: Vec<String>,
    /// The repo's resolved `cache-formats`: `layout.conf`'s value lowercased
    /// and split, or -- when empty -- auto-detected from directory presence
    /// (`md5-dict` first, then `pms`). See [`pregen_md5_cache_enabled`].
    /// Portage: config.py, parse_layout.
    pub cache_formats: Vec<String>,
    /// This repo's `aliases`: `layout.conf`'s first, then `repos.conf`'s.
    /// Portuale acts on aliases only in `find_repos`' section-name-vs-
    /// repo-name mismatch escape hatch; NOT in `::`-atom matching or
    /// profile parents (a documented cut).
    /// Portage: repository/config.py.
    pub aliases: Vec<String>,
    /// `repos.conf`'s `sync-type` (user value wins over the global
    /// `/usr/share/portage/config/repos.conf`). Display-only
    /// (`emerge --info`'s `Repositories:` block).
    pub sync_type: Option<String>,
    /// `repos.conf`'s `sync-uri`, merged the same way. Display-only.
    pub sync_uri: Option<String>,
    /// `repos.conf`'s `volatile`: the explicit `true`/`yes` value, else
    /// "volatile unless under `/var/db/repos` and owned by
    /// `root`/`portage`". Display-only.
    pub volatile: bool,
    /// Sync-module-specific `repos.conf` options that are set, as ordered
    /// `(key, value)` pairs. Display-only.
    /// Portage: RepoConfig.module_specific_options.
    pub module_specific_options: Vec<(String, String)>,
}

/// Whether `path`'s owning uid is `root` (0) or the `portage` user --
/// the ownership half of the `volatile` heuristic. Best-effort: the
/// `portage` uid is looked up from `/etc/passwd` (no libc dependency);
/// an unreadable path or passwd yields `false`.
/// Portage: repository/config.py.
#[cfg(unix)]
pub(crate) fn path_owned_by_root_or_portage(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let uid = meta.uid();
    if uid == 0 {
        return true;
    }
    fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| {
            passwd.lines().find_map(|line| {
                let mut f = line.split(':');
                (f.next() == Some("portage"))
                    .then(|| f.nth(1).and_then(|u| u.parse::<u32>().ok()))
                    .flatten()
            })
        })
        .map(|portage_uid| portage_uid == uid)
        .unwrap_or(false)
}

/// The sync-module-specific option keys recognized for a given
/// `sync-type`, in a fixed order so `emerge --info`'s `Repositories:`
/// output is deterministic (Portage iterates a set).
pub(crate) fn module_specific_keys(sync_type: &str) -> &'static [&'static str] {
    match sync_type {
        "git" => &[
            "sync-git-clone-env",
            "sync-git-clone-extra-opts",
            "sync-git-env",
            "sync-git-pull-env",
            "sync-git-pull-extra-opts",
            "sync-git-verify-commit-signature",
            "sync-git-verify-max-age-days",
        ],
        "rsync" => &[
            "sync-rsync-extra-opts",
            "sync-rsync-vcs-ignore",
            "sync-rsync-verify-jobs",
            "sync-rsync-verify-max-age",
            "sync-rsync-verify-metamanifest",
        ],
        "webrsync" => &["sync-webrsync-verify-signature"],
        _ => &[],
    }
}

/// Parses a repo's `metadata/layout.conf` -- a section-less `key = value`
/// file. Returns an empty map when the file is absent. Portuale reads five
/// keys only: `masters`, `repo-name`, `profile-formats`, `cache-formats`,
/// `aliases`.
/// Portage: repository/config.py, parse_layout_conf.
pub(crate) fn parse_layout_conf(repo_location: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(repo_location.join("metadata/layout.conf")) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(eq) = line.find('=') {
            out.insert(
                line[..eq].trim().to_string(),
                line[eq + 1..].trim().to_string(),
            );
        }
    }
    out
}

/// Reads `<repo_location>/profiles/repo_name`, the first line trimmed.
/// `None` when the file is absent or empty (the caller falls back to the
/// `repos.conf` `[section]` name).
/// Portage: repository/config.py, _read_repo_name.
pub(crate) fn read_repo_name_file(repo_location: &Path) -> Option<String> {
    let text = fs::read_to_string(repo_location.join("profiles/repo_name")).ok()?;
    let name = text.lines().next().unwrap_or("").trim().to_string();
    (!name.is_empty()).then_some(name)
}
