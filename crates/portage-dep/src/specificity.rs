//! Config-atom specificity ranking.

use crate::*;

/// Specificity ranking for config atoms, Portage `best_match_to_list`
/// (dep:2796-2884): full operators outrank bare atoms; extended atoms rank
/// below everything (`=*` 0, slotted −1, plain −2 — the code values, which
/// differ from the docstring). Anything unparseable is −2, like before.
pub fn atom_specificity(entry: &str) -> i32 {
    let opts = AtomOptions {
        allow_wildcard: true,
        allow_repo: Some(true),
        ..AtomOptions::default()
    };
    let Ok(atom) = parse_atom_with(entry, &opts, "") else {
        return -2;
    };
    if atom.extended {
        if atom.operator == Operator::EqGlob {
            return 0;
        }
        if atom.slot.is_some() {
            return -1;
        }
        return -2;
    }
    let op_val = match atom.operator {
        Operator::Eq => 6,
        Operator::Tilde => 5,
        Operator::EqGlob => 4,
        Operator::Gt | Operator::Ge | Operator::Lt | Operator::Le => 2,
        Operator::None => 1,
    };
    let slot_val = if atom.slot.is_some() { 3 } else { i32::MIN };
    op_val.max(slot_val)
}
