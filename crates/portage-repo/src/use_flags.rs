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

/// A content fingerprint of every `Config` field
/// [`effective_use_flags`] reads: the frozen `use_context_base` plus
/// the live `autounmask_use` (production), or the full content hash
/// (hand-built configs). A post-freeze mutation fails loudly in debug.
pub(crate) fn use_context_fingerprint(config: &portage_profile::Config) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let base = match config.use_context_base {
        Some(base) => {
            debug_assert_eq!(
                base,
                portage_profile::use_context_base_fingerprint(config),
                "a USE-context field was mutated after resolve_config froze its digest"
            );
            base
        }
        None => portage_profile::use_context_base_fingerprint(config),
    };
    base.hash(&mut h);
    config.autounmask_use.hash(&mut h);
    h.finish()
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

/// Whether `mask` (rather than `force`) sources are being resolved --
/// picks the file set inside `resolved_use_mask_or_force`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MaskOrForce {
    Mask,
    Force,
}

pub struct ForcedOrMaskedFlagsArgs<'a> {
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
    pub config: &'a portage_profile::Config,
}

/// Specificity ranking for config atoms: versioned/slotted atoms outrank
/// bare ones; bounded wildcards (`*/*`, `category/*`) always rank lowest.
/// Portage: best_match_to_list specificity table
pub(crate) fn atom_specificity(entry: &str) -> i32 {
    let Some(atom) = portage_dep::parse_atom(entry) else {
        return -2;
    };
    let op_val = match atom.operator {
        portage_dep::Operator::Eq => 6,
        portage_dep::Operator::Tilde => 5,
        portage_dep::Operator::EqGlob => 4,
        portage_dep::Operator::Gt
        | portage_dep::Operator::Ge
        | portage_dep::Operator::Lt
        | portage_dep::Operator::Le => 2,
        portage_dep::Operator::None => 1,
    };
    let slot_val = if atom.slot.is_some() { 3 } else { i32::MIN };
    op_val.max(slot_val)
}
