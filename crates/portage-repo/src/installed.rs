//! Installed packages read from the VDB.

use crate::*;

/// Whether `depstr`, flattened against `use_flags`, has a non-blocker atom that
/// `candidate` (`cat/pkg-ver:slot/subslot`) satisfies. Shared by both halves of
/// [`installed_reverse_dependents`].
pub(crate) fn depstr_names_consumer(
    depstr: &str,
    use_flags: &HashSet<String>,
    consumer_category: &str,
    consumer_package: &str,
    candidate: &str,
) -> bool {
    if depstr.trim().is_empty() {
        return false;
    }
    let Some(atoms) = flat_dep_atoms(depstr, use_flags) else {
        return false;
    };
    atoms.iter().any(|atom_str| {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            return false;
        };
        atom.blocker == portage_dep::Blocker::None
            && atom.category == consumer_category
            && atom.package == consumer_package
            && portage_dep::match_from_list(atom_str, &[candidate]).is_some_and(|m| !m.is_empty())
    })
}

/// The database-backend half of [`installed_reverse_dependents`]: the backend's
/// raw records for the consumer's `category/package` (a superset), put through
/// the same per-entry steps as the scan. `None` when the backend cannot answer,
/// so the caller falls back to the scan.
pub(crate) fn indexed_reverse_dependents(
    db: &dyn portage_vdb::InstalledDb,
    consumer_category: &str,
    consumer_package: &str,
    consumer_version: &str,
    candidate: &str,
) -> Option<Vec<String>> {
    use portage_vdb::DepClass;
    let cp = format!("{consumer_category}/{consumer_package}");
    let records = db
        .reverse_dependents(
            &cp,
            &[
                DepClass::Depend,
                DepClass::Rdepend,
                DepClass::Bdepend,
                DepClass::Pdepend,
            ],
        )
        .ok()?;
    let mut out = std::collections::BTreeSet::new();
    for rec in records {
        let Some((name, version)) = split_pf(&rec.key.pf) else {
            continue;
        };
        let (category, package) = apply_updates_to_cp(&rec.key.category, &name);
        if category == consumer_category
            && package == consumer_package
            && version == consumer_version
        {
            continue;
        }
        let use_flags: HashSet<String> = rec
            .use_flags
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        if rec.deps.iter().any(|(_, depstr)| {
            depstr_names_consumer(
                depstr,
                &use_flags,
                consumer_category,
                consumer_package,
                candidate,
            )
        }) {
            out.insert(format!("{category}/{package}-{version}"));
        }
    }
    Some(out.into_iter().collect())
}

/// Flattens `depstr` against `use_flags` into dependency-atom tokens (`||`
/// dropped) for order-independent comparison. `None` if `depstr` doesn't parse.
pub(crate) fn flat_dep_atoms(depstr: &str, use_flags: &HashSet<String>) -> Option<HashSet<String>> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let flat = portage_use_reduce::use_reduce_flat(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .ok()?;
    Some(flat.into_iter().filter(|t| t != "||").collect())
}

/// One installed package in the vdb, `category/package-version` plus its recorded
/// main `SLOT`.
// Portage: vartree.dbapi.cpv_all, _pkg_str
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub category: String,
    pub package: String,
    pub version: String,
    pub slot: String,
}
