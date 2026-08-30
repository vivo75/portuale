//! Binding `:=` dependencies to the selected slot.

use crate::*;

/// The built `:=` binding for `cat/pkg`: the tree sub-slot of the highest
/// installed version, or `None` when there is none.
pub(crate) fn built_equals_binding(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
) -> Option<(String, String)> {
    let inst_ver = installed_candidates(root, category, package)
        .into_iter()
        .map(|(v, _, _)| v)
        .max_by(|x, y| vercmp_ordering(x, y))?;
    let (bound_sub, _, _) = slot_conflict_meta(repos, category, package, &inst_ver);
    if bound_sub.is_empty() {
        return None;
    }
    Some((inst_ver, bound_sub))
}

pub(crate) fn built_equals_shifted_slot(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    existing_sub: &str,
) -> bool {
    built_equals_binding(root, repos, category, package)
        .is_some_and(|(_, bound_sub)| bound_sub != existing_sub)
}

/// Arguments of [`built_equals_shift_unrebuildable`].
pub(crate) struct BuiltEqualsShiftUnrebuildableArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) existing_sub: &'a str,
    pub(crate) pullers: &'a SlotPullers,
    pub(crate) excluded: &'a [String],
}

/// A `:=` shift is only a conflict when the bound consumer cannot be
/// rebuilt (masked or `--exclude`d); a rebuildable consumer rebuilds in
/// place with no conflict.
pub(crate) fn built_equals_shift_unrebuildable(
    args: BuiltEqualsShiftUnrebuildableArgs<'_>,
) -> bool {
    let BuiltEqualsShiftUnrebuildableArgs {
        root,
        repos,
        category,
        package,
        existing_sub,
        pullers,
        excluded,
    } = args;
    if !built_equals_shifted_slot(root, repos, category, package, existing_sub) {
        return false;
    }
    pullers
        .get(&(category.to_string(), package.to_string()))
        .is_some_and(|ps| {
            ps.iter().any(|(pcat, ppkg, pver, _)| {
                let cpv = format!("{pcat}/{ppkg}-{pver}");
                excluded
                    .iter()
                    .any(|ex| matches_config_entry(ex, &cpv, pcat, ppkg))
                    || useoldpkg_atom_matches(pcat, ppkg, pver)
            })
        })
}

/// Removes every atom on an installed libc provider from a flattened set.
// Portage: dep/libc.py, strip_libc_deps
pub(crate) fn strip_libc_atoms(
    mut atoms: HashSet<String>,
    libc_cps: &HashSet<(String, String)>,
) -> HashSet<String> {
    if libc_cps.is_empty() {
        return atoms;
    }
    atoms.retain(|t| {
        portage_dep::parse_atom(t).is_none_or(|a| !libc_cps.contains(&(a.category, a.package)))
    });
    atoms
}

/// The tree ebuild's md5-cache metadata for `cp` at `version`.
/// `None` when it cannot be read: the rebuild is kept (conservative).
pub(crate) fn tree_metadata_for(
    repos: &[RepoConfig],
    cp: &(String, String),
    version: &str,
) -> Option<std::sync::Arc<HashMap<String, String>>> {
    let candidates = list_candidates(repos, &cp.0, &cp.1).ok()?;
    let resolved = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)?;
    repo_aux_metadata(
        &resolved.repo_location,
        &cp.0,
        &format!("{}-{version}", cp.1),
    )
    .ok()
}
