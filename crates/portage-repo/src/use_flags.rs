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

/// The USE flags in effect for one package: one continuous incremental
/// walk over the modelled `USE_ORDER` layers, weakest first: `env.d`,
/// `repo`, `features`, `pkginternal` (IUSE `+`/`-` markers), `defaults`
/// (per profile level), `conf`, `pkg`, `env`, then `autounmask_use`.
///
/// Every layer replays ordered raw tokens, never a pre-flattened union,
/// so a later `-flag` cancels an earlier `+flag`. `_*` wildcards expand
/// against this candidate's IUSE afterwards; force/mask apply last,
/// per-level interleaved (see `resolved_use_mask_or_force`).
///
/// Only the candidate's identity (`iuse`, `keywords`, `candidate_str`,
/// `category`, `package`) is passed separately.
/// Portage: ebuild/config.py, regenerate over USE_ORDER
pub fn effective_use_flags(args: EffectiveUseFlagsArgs<'_>) -> Rc<HashSet<String>> {
    let EffectiveUseFlagsArgs {
        config,
        iuse,
        keywords,
        candidate_str,
        category,
        package,
    } = args;
    // Memoised per (config USE context, candidate): the reference
    // workload calls this ~20k times for a few hundred distinct
    // candidates. Pure in its six args; thread-local, lock-free.
    // `category`/`package`/`iuse`/`keywords` all join the key even
    // though `candidate_str` usually implies them (`--dynamic-deps=n`
    // can pass vdb-built metadata for a tree cpv).
    type EufCache = HashMap<(u64, String), Rc<HashSet<String>>>;
    thread_local! {
        static EUF_CACHE: RefCell<EufCache> = RefCell::new(HashMap::new());
    }
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use_context_fingerprint(config).hash(&mut h);
        iuse.hash(&mut h);
        keywords.hash(&mut h);
        (h.finish(), candidate_str.to_string())
    };
    if let Some(hit) = EUF_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        // The shared set, not a clone: callers only read (the per-call
        // clone was ~14 M `String` clones on a deep `emerge -pu`).
        return hit;
    }
    let result = Rc::new(effective_use_flags_uncached(
        EffectiveUseFlagsUncachedArgs {
            config,
            iuse,
            keywords,
            candidate_str,
            category,
            package,
        },
    ));
    EUF_CACHE.with(|c| {
        c.borrow_mut().insert(key, Rc::clone(&result));
    });
    result
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

pub(crate) fn effective_use_flags_uncached(
    args: EffectiveUseFlagsUncachedArgs<'_>,
) -> HashSet<String> {
    let EffectiveUseFlagsUncachedArgs {
        config,
        iuse,
        keywords,
        candidate_str,
        category,
        package,
    } = args;
    // The per-package walk, weakest layer first (see the fn doc):
    // env.d -> repo -> features -> pkginternal -> defaults -> conf ->
    // pkg -> env, then autounmask_use and the force/mask step.
    let mut use_flags: HashSet<String> = HashSet::new();

    // `env.d`: the lowest tier (see `Config::envd_use_tokens`).
    for token in &config.envd_use_tokens {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `repo`: `make.defaults` USE then `package.use`, before IUSE
    // defaults. An empty repo name applies to every package; a named
    // one only to that repo's candidates.
    let candidate_repo = candidate_str.rsplit_once("::").map_or("", |(_, r)| r);
    for (repo, tokens) in &config.repo_make_defaults_use {
        if repo.is_empty() || repo == candidate_repo {
            for token in tokens {
                portage_profile::apply_use_incremental(token, &mut use_flags);
            }
        }
    }
    let apply_matching = |flags: &mut HashSet<String>, entries: &[(String, Vec<String>)]| {
        for (_, tokens) in config_entries_matching(entries, candidate_str, category, package) {
            portage_profile::apply_use_incremental_iter(tokens, flags);
        }
    };
    apply_matching(&mut use_flags, &config.package_use_repo);

    // `features`: only `test` is modelled; a masked `test` still
    // drops out in the force/mask step below.
    for token in &config.features_use {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `pkginternal`: only an explicit `+`/`-` marker contributes; a
    // markerless IUSE token is a deliberate no-op.
    let iuse_defaults: String = iuse
        .split_whitespace()
        .filter(|tok| tok.starts_with('+') || tok.starts_with('-'))
        .collect::<Vec<_>>()
        .join(" ");
    portage_profile::apply_use_incremental(&iuse_defaults, &mut use_flags);

    // `defaults`: one profile level at a time (that level's
    // `make.defaults`, then its `package.use`); the flat tokens are the
    // hand-built-`Config` fallback.
    if config.profile_use_layers.is_empty() {
        for token in &config.use_tokens {
            portage_profile::apply_use_incremental(token, &mut use_flags);
        }
        apply_matching(&mut use_flags, &config.package_use);
    } else {
        for layer in &config.profile_use_layers {
            for token in &layer.make_defaults_use {
                portage_profile::apply_use_incremental(token, &mut use_flags);
            }
            apply_matching(&mut use_flags, &layer.package_use);
        }
    }

    // `conf`: `make.conf` USE, then the USE_EXPAND-folded values.
    for token in &config.conf_use_tokens {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `pkg`: `package.env` USE first, then user `package.use` on top.
    apply_matching(&mut use_flags, &config.package_env_use);
    apply_matching(&mut use_flags, &config.package_use_user);

    // `env`: the process-environment `USE`, the strongest layer before
    // force/mask.
    for token in &config.env_use_tokens {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `autounmask_use`: the resolver's already-decided USE changes, as
    // the top tier (normally empty; see `Config::autounmask_use`).
    apply_matching(&mut use_flags, &config.autounmask_use);

    // `_*` wildcards enable every `prefix_*` flag this candidate's
    // own IUSE declares; masked ones drop out below, and the `_*`
    // tokens themselves are stripped after expansion.
    let iuse_names: Vec<String> = iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let wildcard_prefixes: Vec<String> = use_flags
        .iter()
        .filter_map(|f| f.strip_suffix('*').map(|p| p.to_string()))
        .filter(|p| p.ends_with('_'))
        .collect();
    for pfx in &wildcard_prefixes {
        for name in &iuse_names {
            if name.starts_with(pfx) {
                use_flags.insert(name.clone());
            }
        }
    }

    let stable = is_stable(IsStableArgs {
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    });

    // use.force/use.mask (+ `.stable.` variants when stable) as the
    // last step, each from its own per-level interleaved stack (see
    // `resolved_use_mask_or_force`). Uncached-build path only.
    for flag in resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Force,
        config,
        candidate_str,
        category,
        package,
        stable,
    })
    .iter()
    {
        use_flags.insert(flag.clone());
    }
    for flag in resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Mask,
        config,
        candidate_str,
        category,
        package,
        stable,
    })
    .iter()
    {
        use_flags.remove(flag);
    }
    // The `_*` tokens themselves are stripped after expansion
    // (Portage drops them from `PORTAGE_USE`).
    use_flags.retain(|f| !f.ends_with("_*"));
    use_flags
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

/// This candidate's IUSE flags that are force-enabled or
/// mask-disabled (rendered wrapped in `( … )`, not user-controllable).
/// Portage: _display_use, forced_flags = pkg.use.force | pkg.use.mask
pub fn forced_or_masked_flags(args: ForcedOrMaskedFlagsArgs<'_>) -> HashSet<String> {
    let ForcedOrMaskedFlagsArgs {
        iuse,
        keywords,
        candidate_str,
        category,
        package,
        config,
    } = args;
    let iuse_names: HashSet<String> = iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let mut result =
        forced_or_masked_flags_unfiltered(keywords, candidate_str, category, package, config);
    result.retain(|f| iuse_names.contains(f));
    result
}

/// The same force/mask stack without the IUSE intersection (for the
/// reinstall-for-flags path, which must also forgive a masked flag the
/// new ebuild dropped from IUSE).
/// Portage: lib/_emerge/depgraph.py, _reinstall_for_flags inputs
pub(crate) fn forced_or_masked_flags_unfiltered(
    keywords: &[String],
    candidate_str: &str,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> HashSet<String> {
    let stable = is_stable(IsStableArgs {
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    });
    // Owned union of the two shared sets (both usually cache hits).
    let mut result = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Force,
        config,
        candidate_str,
        category,
        package,
        stable,
    })
    .as_ref()
    .clone();
    result.extend(
        resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
            which: MaskOrForce::Mask,
            config,
            candidate_str,
            category,
            package,
            stable,
        })
        .iter()
        .cloned(),
    );
    result
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
