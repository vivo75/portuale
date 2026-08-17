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
