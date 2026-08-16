//! Installed packages' metadata as the resolver sees it.

use crate::*;

/// The live ebuild metadata for one installed `cat/pkg-version`, always from the
/// repo the vdb says it was built from, never from a repo-priority search. `None`
/// when the version is gone from the recorded repo. One shared helper, so the four
/// installed-metadata readers can never disagree on which repo is authoritative.
/// Portage: FakeVartree.py, _aux_get_wrapper.
pub(crate) fn live_metadata_for_installed(
    repos: &[RepoConfig],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<std::sync::Arc<HashMap<String, String>>> {
    let pf = format!("{package}-{version}");
    let recorded = read_vdb_string(root, category, package, version, "repository");
    if !recorded.is_empty()
        && let Some(repo) = repos.iter().find(|r| r.name == recorded)
        && let Ok(meta) = repo_aux_metadata(&repo.location, category, &pf)
    {
        return Some(meta);
    }
    // Fallback narrowing: a missing `repository` file (minimal fixture vdbs) or a
    // removed repo falls back to a priority search instead of stranding the package.
    list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .and_then(|c| repo_aux_metadata(&c.repo_location, category, &pf).ok())
        })
}

/// Every atom in one raw (vdb) `*DEPEND` string that is a *built* slot-operator dep
/// (`:=` with a sub-slot, flattened against the recorded USE). A `use_reduce`
/// failure reads as no built atoms. Portage: _slot_operator.py, find_built_slot_operator_atoms.
pub(crate) fn built_slot_operator_atoms(
    raw_depstr: &str,
    use_flags: &HashSet<String>,
) -> Vec<String> {
    let tokens: Vec<String> = raw_depstr.split_whitespace().map(String::from).collect();
    let Ok(flat) = portage_use_reduce::use_reduce_flat(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) else {
        return Vec::new();
    };
    flat.into_iter()
        .filter(|tok| {
            portage_dep::parse_atom(tok).is_some_and(|a| {
                a.slot_operator == Some(portage_dep::SlotOperator::Equals) && a.sub_slot.is_some()
            })
        })
        .collect()
}
