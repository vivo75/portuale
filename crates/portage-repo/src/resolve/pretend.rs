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

// Whether a string constraint is a binary-only `slot_operator_mask_built` negative:
// every `extra_constraints` filter and the missing-dep probe skip these (the
// same-version ebuild stays visible); only the binary-pool filter honours them.
pub(crate) fn is_binary_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[binary]"))
}

// The installed-only half of the same per-instance distinction
// (`!=cat/pkg-ver[installed]`, filed by the missing-dep trigger for an installed
// parent with no merge-bound version): tree readers skip marked entries, only the
// installed-instance selection honours them. A marked negative never parses as a
// version, so the missed-update collection skips it too.
// Portage: depgraph.py, _add_dep missing-dependency mask
pub(crate) fn is_installed_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[installed]"))
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

// Every per-instance string negative (`[binary]` and `[installed]` marks):
// tree-candidate readers skip them all; instance pools keep honouring their own mark.
// Portage keys every such mask by package object.
pub(crate) fn is_instance_only_mask(constraint: &str) -> bool {
    is_binary_only_mask(constraint) || is_installed_only_mask(constraint)
}
