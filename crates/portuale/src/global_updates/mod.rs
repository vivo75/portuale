//! Portage's `_do_global_updates`: apply `profiles/updates/` `move` and
//! `slotmove` directives to the installed roots (vdb entries, world,
//! `package.*`, binpkgs), then record and force `AUTOCLEAN`.
//! Read-only until every piece lands; the pretend force stays `false`.
//! Portage: lib/portage/_global_updates.py, _do_global_updates

pub(crate) mod binpkg;
pub(crate) mod config_files;
pub(crate) mod vdb;

use portage_repo::{RepoConfig, UpdateCmd};
use std::collections::HashMap;

/// The `_do_global_updates` `repo_match` closure: an entry from
/// `repository` moves under `repo_name`'s updates when it names that
/// repo, or when that repo is the main repo and the entry's repo has
/// no updates of its own (it appears nowhere in the map, so the main
/// repo's updates cover it).
// Portage: lib/portage/_global_updates.py, _do_global_updates
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RepoMatch {
    repos_in_map: Vec<String>,
    master: Option<String>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RepoMatch {
    /// Snapshot the map's repo names plus the main repo's name; one
    /// `matches` call per update repo then needs no other context.
    pub(crate) fn new(map: &[(String, Vec<UpdateCmd>)], master: Option<String>) -> Self {
        Self {
            repos_in_map: map.iter().map(|(name, _)| name.clone()).collect(),
            master,
        }
    }

    /// Whether `repository` counts as `repo_name`'s for one update.
    pub(crate) fn matches(&self, repo_name: &str, repository: &str) -> bool {
        repository == repo_name
            || (Some(repo_name) == self.master.as_deref()
                && !self.repos_in_map.iter().any(|n| n == repository))
    }
}

/// `vartree._aux_get`'s collapse, shared by the vdb and binpkg halves:
/// every whitespace run becomes one blank (the binary database reads
/// through the same rule).
// Portage: lib/portage/dbapi/vartree.py, _aux_get
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One repo's updates, in `repos` order, plus the `DEFAULT` alias for the
/// main repo's list. Repos without an updates directory contribute
/// nothing; repos sharing one directory share its list. Empty lists stay
/// so non-master `repository not in repo_map` checks see every repo.
// Portage: lib/portage/_global_updates.py, _do_global_updates
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn repo_update_map(repos: &[RepoConfig]) -> Vec<(String, Vec<UpdateCmd>)> {
    let mut out: Vec<(String, Vec<UpdateCmd>)> = Vec::new();
    let mut seen: HashMap<std::path::PathBuf, usize> = HashMap::new();
    for repo in repos {
        let updpath = repo.location.join("profiles").join("updates");
        if !updpath.is_dir() {
            continue;
        }
        if let Some(&idx) = seen.get(&updpath) {
            let cmds = out[idx].1.clone();
            out.push((repo.name.clone(), cmds));
            continue;
        }
        let cmds = portage_repo::grab_updates_for_repo(&repo.location);
        seen.insert(updpath, out.len());
        out.push((repo.name.clone(), cmds));
    }
    if let Some(master) = repos.iter().find(|r| r.is_main)
        && let Some(cmds) = out
            .iter()
            .find(|(name, _)| name == &master.name)
            .map(|(_, cmds)| cmds.clone())
    {
        out.push(("DEFAULT".to_string(), cmds));
    }
    out
}
