//! Selecting one atom's candidate (`resolve_pretend`).

use crate::*;

// Reads one `slot_operator_mask_built` negative in binary-marked form
// (`!=cat/pkg-ver[binary]`). Portage keys the mask by package object so only the
// binary instance hides; portuale's string readers match both, so the marker keeps
// them apart: tree readers skip marked entries, only the binary-pool filter honours
// them (via this function).
// Portage: depgraph.py, _slot_change_backtrack; MaskReason::SlotOperatorBuilt
pub(crate) fn slot_operator_binary_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[binary]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}

// Reads one installed-only missing-dependency negative. Mirror of
// [`slot_operator_binary_masked_version`] for the other instance kind.
pub(crate) fn slot_operator_installed_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[installed]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}
