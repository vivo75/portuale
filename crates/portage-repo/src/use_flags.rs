//! Effective USE and USE mask/force resolution.

use crate::*;

pub struct EffectiveUseFlagsArgs<'a> {
    pub config: &'a portage_profile::Config,
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
}

/// Folds matching `entries` (raw `(atom, tokens)` pairs, already in
/// profile-chain / source order) onto `seed` in source order, with
/// `package.use` incremental semantics -- so a later `-flag` cancels an
/// earlier `flag`. Plain file order, not a specificity sort (correct
/// for the `targets/*` child-unmask pattern).
/// Portage: stack_lists(incremental=True) over getUseMask/getUseForce
pub(crate) fn specificity_ordered_flags(
    entries: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
    mut seed: HashSet<String>,
) -> HashSet<String> {
    for (_, tokens) in config_entries_matching(entries, candidate_str, category, package) {
        portage_profile::apply_incremental_iter(tokens, &mut seed);
    }
    seed
}

pub struct ForcedOrMaskedFlagsArgs<'a> {
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
    pub config: &'a portage_profile::Config,
}
