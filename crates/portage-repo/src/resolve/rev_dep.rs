//! Constraints that installed reverse dependencies put on an upgrade.

use crate::*;

/// Whether `atom` is a **built** slot-operator atom (`cat/pkg:S/SS=`).
/// Slot dropped textually (equivalent); `[use-dep]` dropped too (allow-only).
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
pub(crate) fn is_built_slot_op(atom: &portage_dep::Atom) -> bool {
    atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
        && atom.slot.is_some()
        && atom.sub_slot.is_some()
}

/// Portage `Atom.with_slot("=")`: a built atom's `:S/SS=` binding relaxes to
/// bare `:=` (rebuild, never withhold). Non-built/unparseable returns verbatim.
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
pub(crate) fn relax_built_slot_operator_atom(atom_str: &str) -> String {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return atom_str.to_string();
    };
    if !is_built_slot_op(&atom) {
        return atom_str.to_string();
    }
    // Both slots are guaranteed present, so the needle occurs verbatim.
    let needle = format!(
        ":{}/{}=",
        atom.slot.as_deref().unwrap_or_default(),
        atom.sub_slot.as_deref().unwrap_or_default()
    );
    match atom_str.find(&needle) {
        Some(idx) => format!("{}:={}", &atom_str[..idx], &atom_str[idx + needle.len()..]),
        None => atom_str.to_string(),
    }
}

/// Whether an installed-settling edge's atom is a slot-operator rebuild trigger
/// (a built atom accepting the resolved merge once relaxed) rather than a slot
/// conflict. A version veto still records normally; unparseable never triggers.
// Portage: depgraph.py, _minimize_children (update probe)
pub(crate) fn built_slot_operator_rebuild_trigger(atom_str: &str, existing_str: &str) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if !is_built_slot_op(&atom) {
        return false;
    }
    let relaxed = relax_built_slot_operator_atom(atom_str);
    portage_dep::match_from_list(&relaxed, &[existing_str]).is_some_and(|m| !m.is_empty())
}

/// One installed consumer's recorded atom broken by this pass's upgrade, with
/// the consumer attached (Portage's nomerge node, as a triple).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RevDepPin {
    /// The upgraded `category/package`.
    pub(crate) cp: (String, String),
    /// The consumer's recorded atom, normalised by
    /// `reverse_dep_constraint_atom`.
    pub(crate) atom: String,
    /// Verbatim text (with `[use]` deps), for notices only.
    pub(crate) raw_atom: String,
    /// The installed consumer `(category, package, version)`.
    pub(crate) consumer: (String, String, String),
}

/// The decided merge USE/IUSE for `category/package-version`. `None` when no
/// readable candidate exists; callers fall back to ignoring USE.
pub(crate) fn merge_use_state(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(HashSet<String>, HashSet<String>)> {
    let cands = list_candidates(repos, category, package).ok()?;
    let cand = cands.iter().find(|c| c.version == version)?;
    let (iuse, use_flags) = candidate_iuse_and_use(cand, category, package, config)?;
    Some(((*use_flags).clone(), iuse))
}

/// Arguments of [`merge_use_changed_vs_installed`].
pub(crate) struct MergeUseChangedVsInstalledArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
}
