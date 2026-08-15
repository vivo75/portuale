//! Whether an atom is installed or already in the graph.

use crate::*;

/// `best_installed_matching` with the two inputs a caller outside the resolver
/// supplies itself (the native `portageq` `has_version` / `best_version`):
///
/// - `unevaluated`: when `atom_str` is a USE-conditional-evaluated copy of an atom,
///   the original text. The IUSE membership check reads the *unevaluated* atom's
///   required flags, so a flag a dropped `[flag?]` names must still be installed
///   `IUSE`; the dropped conditionals are fed as constraint-free entries for that.
/// - `implicit_iuse`: the profile's `IUSE_EFFECTIVE`. Called at most once and only
///   when the atom has `[use]` deps, so a caller can load the config lazily.
///
/// Returns the highest matching installed version (the `PF` version part).
/// Portage: vartree.py, vardb.match().
pub fn best_installed_match(
    root: &Path,
    atom_str: &str,
    unevaluated: Option<&str>,
    implicit_iuse: &dyn Fn() -> HashSet<String>,
) -> Option<String> {
    let atom = portage_dep::parse_atom(atom_str)?;
    let installed = installed_candidates(root, &atom.category, &atom.package);
    if installed.is_empty() {
        return None;
    }
    let cpv_strs: Vec<String> = installed
        .iter()
        .map(|(v, s, ss)| {
            format!(
                "{}/{}-{v}:{s}/{ss}::{}",
                atom.category,
                atom.package,
                installed_pkg_repo(root, &atom.category, &atom.package, v)
            )
        })
        .collect();
    let refs: Vec<&str> = cpv_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom_str, &refs)?;
    let mut all_deps: Vec<portage_dep::UseDep> = atom.use_deps.clone().unwrap_or_default();
    if let Some(orig) = unevaluated.and_then(portage_dep::parse_atom)
        && let Some(orig_deps) = orig.use_deps
    {
        all_deps.extend(orig_deps);
    }
    let use_deps = Some(all_deps).filter(|d| !d.is_empty());
    let by_str: HashMap<&str, &(String, String, String)> =
        refs.iter().copied().zip(installed.iter()).collect();
    let implicit: std::cell::OnceCell<HashSet<String>> = std::cell::OnceCell::new();
    matched
        .into_iter()
        .filter_map(|m| by_str.get(m).map(|(version, _slot, _sub)| version.clone()))
        .filter(|version| {
            let Some(use_deps) = &use_deps else {
                return true;
            };
            let vdb_iuse = read_vdb_flag_set(root, &atom.category, &atom.package, version, "IUSE");
            let vdb_use = read_vdb_flag_set(root, &atom.category, &atom.package, version, "USE");
            let mut valid = vdb_iuse;
            valid.extend(implicit.get_or_init(implicit_iuse).iter().cloned());
            valid.extend(vdb_use.iter().cloned());
            portage_dep::use_deps_satisfied(use_deps, &valid, &vdb_use)
        })
        .max_by(|a, b| vercmp_ordering(a, b))
}
