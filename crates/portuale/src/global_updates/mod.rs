//! Portage's `_do_global_updates`: apply `profiles/updates/` `move` and
//! `slotmove` directives to the installed roots (vdb entries, world,
//! `package.*`, binpkgs), then record and force `AUTOCLEAN`.
//! Read-only until every piece lands; the pretend force stays `false`.
//! Portage: lib/portage/_global_updates.py, _do_global_updates

pub(crate) mod vdb;

use portage_repo::{RepoConfig, UpdateCmd};
use std::collections::HashMap;

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
