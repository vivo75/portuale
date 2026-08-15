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
pub(crate) struct EffectiveUseFlagsUncachedArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) iuse: &'a str,
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
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

pub(crate) struct ResolvedUseMaskOrForceArgs<'a> {
    pub(crate) which: MaskOrForce,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) stable: bool,
}

/// The four (two when not stable) mask/force sources stacked
/// interleaved per profile level -- so a later level's global `-flag`
/// cancels an earlier level's `package.use.mask` entry. Per level:
/// global, global-stable (stable only), package, package-stable.
/// Portage: UseManager.getUseMask/getUseForce
pub(crate) fn resolved_use_mask_or_force(
    args: ResolvedUseMaskOrForceArgs<'_>,
) -> Rc<HashSet<String>> {
    let ResolvedUseMaskOrForceArgs {
        which,
        config,
        candidate_str,
        category,
        package,
        stable,
    } = args;
    // Memoised per (which, stable, USE context, candidate); the levels
    // are fixed at config resolution, so the walk result is stable.
    // Thread-local, lock-free.
    type RumfCache = HashMap<(MaskOrForce, bool, u64, String, String, String), Rc<HashSet<String>>>;
    thread_local! {
        static RUMF_CACHE: RefCell<RumfCache> = RefCell::new(HashMap::new());
    }
    let key = (
        which,
        stable,
        use_context_fingerprint(config),
        candidate_str.to_string(),
        category.to_string(),
        package.to_string(),
    );
    if let Some(hit) = RUMF_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let result = Rc::new(resolved_use_mask_or_force_uncached(
        ResolvedUseMaskOrForceUncachedArgs {
            which,
            config,
            candidate_str,
            category,
            package,
            stable,
        },
    ));
    RUMF_CACHE.with(|c| {
        c.borrow_mut().insert(key, Rc::clone(&result));
    });
    result
}
pub(crate) struct ResolvedUseMaskOrForceUncachedArgs<'a> {
    pub(crate) which: MaskOrForce,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) stable: bool,
}

pub(crate) fn resolved_use_mask_or_force_uncached(
    args: ResolvedUseMaskOrForceUncachedArgs<'_>,
) -> HashSet<String> {
    let ResolvedUseMaskOrForceUncachedArgs {
        which,
        config,
        candidate_str,
        category,
        package,
        stable,
    } = args;
    let mut acc: HashSet<String> = HashSet::new();
    for level in &config.use_mask_force_levels {
        let (global, global_stable, pkg, pkg_stable) = match which {
            MaskOrForce::Mask => (
                &level.use_mask,
                &level.use_stable_mask,
                &level.package_use_mask,
                &level.package_use_stable_mask,
            ),
            MaskOrForce::Force => (
                &level.use_force,
                &level.use_stable_force,
                &level.package_use_force,
                &level.package_use_stable_force,
            ),
        };
        portage_profile::apply_incremental_iter(global, &mut acc);
        if stable {
            portage_profile::apply_incremental_iter(global_stable, &mut acc);
        }
        acc = specificity_ordered_flags(pkg, candidate_str, category, package, acc);
        if stable {
            acc = specificity_ordered_flags(pkg_stable, candidate_str, category, package, acc);
        }
    }
    acc
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
