//! depclean, prune and clean selection.

use crate::*;

/// One cp's `emerge --prune --nodeps` split: `best_version` is
/// `protected` (kept), `other_versions` (ascending) are all `selected`
/// for removal.
#[derive(Debug, Clone)]
pub struct PruneNodepsCp {
    pub category: String,
    pub package: String,
    pub best_version: String,
    pub other_versions: Vec<String>,
}

/// Portage `strip_slots` (Portage dep) for one atom:
/// rewrites a "built" slot-operator atom (`cat/pkg:2=`, the concrete slot
/// Portage records in the vdb) back to `cat/pkg:=`, so `deps_changed`'s
/// vdb-vs-ebuild comparison doesn't flag every `:=` dep as changed. Only
/// `slot_operator == "="` with an explicit slot; the slot expression is unique
/// in an atom string, so a single leftmost replacement is safe.
pub(crate) fn strip_slot_operator_one(tok: &str) -> String {
    let Some(atom) = portage_dep::parse_atom(tok) else {
        return tok.to_string();
    };
    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
        return tok.to_string();
    }
    let Some(slot) = atom.slot.as_deref() else {
        return tok.to_string();
    };
    let slot_text = match atom.sub_slot.as_deref() {
        Some(sub) => format!(":{slot}/{sub}="),
        None => format!(":{slot}="),
    };
    tok.replacen(&slot_text, ":=", 1)
}
