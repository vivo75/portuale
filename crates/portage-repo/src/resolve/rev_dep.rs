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
