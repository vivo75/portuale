//! The `preserve-libs` gates: `FEATURES` and the installed instances' `RESTRICT`.

use crate::ebuild_merge::*;

/// Whether the installed `category/pf`'s `RESTRICT` (reduced with its own recorded
/// `USE`, as `setcpv(mydb=vardb)` does) names `preserve-libs`.
// Portage: vartree.py, dblink.treewalk and dblink.unmerge (`PORTAGE_RESTRICT`)
pub(crate) fn installed_restricts_preserve_libs(root: &Path, category: &str, pf: &str) -> bool {
    let Some(raw) = read_entry_text(root, category, pf, "RESTRICT") else {
        return false;
    };
    let use_set: std::collections::HashSet<String> = read_entry_text(root, category, pf, "USE")
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect();
    crate::ebuild_phases::flat_field_on(&raw, &use_set)
        .split_whitespace()
        .any(|token| token == "preserve-libs")
}

/// The installed same-slot instances of `category/package`, as vdb `pf` names.
pub(crate) fn installed_same_slot_pfs(
    root: &Path,
    category: &str,
    package: &str,
    slot: &str,
) -> Vec<String> {
    let Ok(names) = portage_vdb::for_root(root).category_entries(category) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| {
            name.strip_prefix(&format!("{package}-"))
                .is_some_and(|version| version.starts_with(|c: char| c.is_ascii_digit()))
        })
        .filter(|name| {
            let version = &name[package.len() + 1..];
            read_installed_slot(root, category, package, version).as_deref() == Some(slot)
        })
        .collect()
}

/// `dblink._preserve_libs` for a merge: `preserve-libs` in `FEATURES` and no replaced
/// same-slot instance restricted it. One restricted instance restricts them all.
/// `features_enabled` is the `FEATURES` half; the new package's own `RESTRICT` is never read.
// Portage: vartree.py, dblink.__init__ and dblink.treewalk
pub(crate) fn preserve_libs_for_merge(
    features_enabled: bool,
    root: &Path,
    category: &str,
    package: &str,
    main_slot: &str,
) -> bool {
    features_enabled
        && !installed_same_slot_pfs(root, category, package, main_slot)
            .iter()
            .any(|pf| installed_restricts_preserve_libs(root, category, pf))
}
