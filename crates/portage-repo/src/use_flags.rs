//! Effective USE and USE mask/force resolution.

use crate::*;

/// Arguments of [`effective_use_flags`].
pub struct EffectiveUseFlagsArgs<'a> {
    pub config: &'a portage_profile::Config,
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
}

/// The USE flags in effect for one specific package -- one continuous
/// incremental walk over the real `USE_ORDER` layers portuale models,
/// low priority to high (real `config.py::regenerate()` over the
/// reversed `uvlist`; `USE_ORDER` default
/// `env:pkg:conf:defaults:pkginternal:features:repo:env.d`). The "Config
/// depth" slice (SCOPE_BACKLOG Part 2.C) split the earlier flat model
/// ("IUSE seed, then `use_tokens`, then one flat `package.use` list") so
/// each `package.use` source sits at its own real position. The walk,
/// weakest layer first:
///
/// - `repo` -- each repo's `profiles/make.defaults` USE
///   (`Config::repo_make_defaults_use`, real `_repo_make_defaults`:
///   the main repo's applies to every package, an overlay's only to a
///   candidate from that overlay), then every configured repo's own
///   `profiles/package.use` (`Config::package_use_repo`) -- all *before*
///   the IUSE defaults. The weakest layer portuale models.
/// - `pkginternal` -- `iuse`'s own `+flag`/`-flag` default markers (see
///   the paragraph below for the grounding).
/// - `defaults` -- walked one profile chain level at a time
///   (`Config::profile_use_layers`): that level's `make.defaults` USE,
///   then that level's own `package.use`, before the next level (so a
///   child profile's `make.defaults` can cancel a parent's
///   `package.use`). Falls back to the flat `Config::use_tokens` +
///   `Config::package_use` for a hand-built `Config`.
/// - `conf` -- `make.conf` USE, then the `USE_EXPAND` folded values
///   (`Config::conf_use_tokens`).
/// - `pkg` -- `package.env`'s own `USE=` (`Config::package_env_use`),
///   then the user-level `/etc/portage/package.use`
///   (`Config::package_use_user`) on top. The strongest layer before
///   the final `use.force`/`use.mask` step.
///
/// Every layer is replayed via `apply_use_incremental` directly -- not a
/// pre-flattened set unioned on top (see the `iuse` paragraph below for
/// why that distinction matters). The rest of the `env` layer
/// (`$USE`, `package.env` non-USE vars) and `features`/`env.d` are
/// documented cuts. Unlike `is_visible`'s mask/keywords checks (which
/// only ever add to an accepted set), this can both add and remove
/// flags, and does so per package -- a `package.use` entry never affects
/// any other package's own resolution.
///
/// **`iuse`'s own defaults**: found and grounded by comparing this
/// portuale's own output against the real, installed system `emerge` on a
/// real package (`media-video/ffmpeg`) -- REQUIRED_USE reported violated
/// for a USE combination that's actually fully satisfied once IUSE's own
/// `+`/`-` markers are honored (`ffmpeg`'s own real IUSE declares
/// `+gpl`/`+dav1d`/`+drm`/etc., none of which portuale's prior
/// `effective_use_flags` ever enabled, silently defaulting every one of
/// them to disabled instead). Real `config.py`'s own `_setup_pkg_iuse`
/// (`lib/portage/package/ebuild/config.py`, ~line 1878) builds exactly
/// this from a package's raw `IUSE` string -- `+flag` contributes a bare
/// `flag` (enable) token, `-flag` contributes itself unchanged (disable),
/// a markerless `flag` contributes nothing at all -- and stores it under
/// `self.configdict["pkginternal"]["USE"]`, a real, named `USE_ORDER`
/// component (`lib/_emerge/actions.py`'s own default,
/// `"env:pkg:conf:defaults:pkginternal:features:repo:env.d"`) --
/// confirmed by reading `config.py`'s own `self.uvlist` construction
/// (`for x in self["USE_ORDER"].split(":"): ...; self.uvlist.reverse()`):
/// incremental application walks `uvlist` in *reversed* `USE_ORDER`, so
/// `pkginternal` (position 5 of 8) is applied after `repo` but well
/// *before* `defaults` (profile), `conf` (`make.conf`), and `pkg`
/// (`package.use`) -- real portage's own actual precedence has every one
/// of those three able to override an IUSE default; only `env`/`env.d`
/// (real per-invocation/stacked-profile-env overrides at the far ends of
/// `USE_ORDER`) sit even lower/higher than portuale models at all.
/// Applied here at that same relative position (the `pkginternal` step
/// of the walk above), with the `repo` `package.use` applied first and
/// every later layer replayed directly on top via `apply_use_incremental` --
/// **not** a plain set union of the already-flattened `use_flags`. An
/// earlier version of
/// portuale *did* union a flattened `base` here, which meant `base`
/// could only ever *add* a flag, never explicitly cancel an IUSE
/// `+default` the way real `defaults`/`conf` genuinely can (real
/// `regenerate()` runs one continuous incremental walk across the whole
/// reversed `uvlist` -- `repo` then `pkginternal` then `defaults` then
/// `conf` then `pkg` -- so a `-flag` token in `defaults`/`conf` really
/// does cancel an earlier `pkginternal` `+flag`, exactly like any other
/// incremental variable). Replaying the *ordered raw tokens* instead of
/// the flattened set closes that gap: `portage_profile::resolve_config`
/// exposes both `use_flags` (the flattened result, still used elsewhere
/// for e.g. `--newuse` comparisons) and the per-layer token lists /
/// `package.use` fields that produced it -- see `Config::use_tokens`'s
/// own doc comment for the full grounding. The dominant real-world case
/// -- an ebuild author sets a sensible IUSE default, and nothing else
/// ever mentions the flag at all -- was already correct either way; this
/// closes the narrower case where a profile, `make.conf`, or a
/// wrongly-layered repo/user `package.use` genuinely does mention it.
///
/// `Config::use_stable_force`/`use_stable_mask`/`package_use_stable_force`/
/// `package_use_stable_mask` (`keywords`/`candidate_str` decide, via
/// `is_stable`, whether this candidate even counts as "stable") are the
/// `.stable.` variants of the global/per-package force/mask sources
/// already applied above -- ported from real `getUseMask`/`getUseForce`'s
/// own per-package (`pkg is not None`) branch, which appends the stable
/// variant right alongside the ordinary one at each accumulation step,
/// but *only* when `stable` -- see this module's own `is_stable` doc
/// comment for the real "would masking every keyword make this
/// invisible" definition. Grouped here as force-then-mask, ordinary
/// then stable, within each tier (not real portage's own
/// per-profile-level interleaving, which portuale's own flat global
/// `use_force`/`use_mask` accumulation already doesn't replicate either
/// -- see `portage-profile`'s own `package.use.mask`/`.force` doc
/// comment for that established, confirmed simplification, extended
/// here rather than re-litigated).
///
/// Every layer above comes from `config`; only the candidate's own
/// identity -- `iuse`, `keywords` (its raw `KEYWORDS`), `candidate_str`
/// (`cat/pkg-ver:slot/sub::repo`), `category`, `package` -- is passed
/// separately. `Config::autounmask_use` is applied as the top tier
/// (after `env`, before `use.force`/`use.mask`) so the `'backtrack`
/// loop's in-progress `--autounmask-use` changes flow through -- see
/// that field's doc comment.
pub fn effective_use_flags(args: EffectiveUseFlagsArgs<'_>) -> Rc<HashSet<String>> {
    let EffectiveUseFlagsArgs {
        config,
        iuse,
        keywords,
        candidate_str,
        category,
        package,
    } = args;
    // Memoise per (config USE context, candidate). A deep `emerge -pu`
    // calls this ~20k times for a few hundred distinct candidates -- once
    // in `is_visible`, then again for `keywords_accepted`, `is_stable`,
    // `binpkg_respect_use_ok`, `use_flags_if_conditional`, the slot-op and
    // changed-deps scans... -- each time rebuilding a ~200-entry
    // `HashSet<String>` from scratch (the largest remaining cost after the
    // `parse_atom` / config-bucket / move-chain / md5-cache fixes).
    //
    // The result is a pure function of the six args. `category`/`package`
    // and (in production) `iuse`/`keywords` are all implied by
    // `candidate_str`, but `--dynamic-deps=n` can pass vdb-built
    // `iuse`/`keywords` for a cpv that also exists in the tree, so all
    // three go in the key. `config` is captured by
    // `use_context_fingerprint` -- the exact frozen USE-context digest
    // plus the live `autounmask_use` (the one field the `'backtrack`
    // loop mutates). Thread-local so the lookup stays lock-free.
    // key: (hash of config USE context + iuse + keywords, candidate_str)
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
        // The shared set, not a clone of it: callers only read (backlog
        // #103 -- the per-call `HashSet` clone was ~14 M `String` clones
        // on a deep `emerge -pu`). Clone at the call site only where
        // ownership is genuinely needed.
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
/// [`effective_use_flags`] reads.
///
/// `#105`: the immutable part is frozen by `resolve_config` into
/// [`portage_profile::Config::use_context_base`], so the production path
/// only hashes that `u64` plus the live `autounmask_use` -- the one field
/// the `'backtrack` loop mutates (each tier / `flag_is_settable` probe
/// adds an entry), hashed in full so a single added entry always changes
/// the key. A hand-built `Config` (tests, `Config::default()`) has no
/// base and falls back to the full content hash,
/// `portage_profile::use_context_base_fingerprint`, so an in-place edit
/// of any context field is still seen.
///
/// Backlog #121: on the frozen path a `debug_assert_eq!` recomputes the
/// base and fails loudly if any USE-context field was mutated after
/// `resolve_config` froze its digest -- silent memo poisoning otherwise.
/// Zero cost in release.
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
/// Arguments of [`effective_use_flags_uncached`].
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
    // The per-package `USE_ORDER` walk, low priority -> high (real
    // `regenerate()` over the reversed `uvlist`; see this function's own
    // doc comment for the full grounding). Portuale models, in order:
    //   env.d -> repo -> features -> pkginternal -> defaults -> conf -> pkg -> env
    // then the final `use.force`/`use.mask` step.
    let mut use_flags: HashSet<String> = HashSet::new();

    // `env.d` (real `configdict["env.d"]["USE"]`, from `/etc/profile.env`):
    // the lowest `USE_ORDER` tier -- everything below overrides it.
    // Practically always empty. See `Config::envd_use_tokens`.
    for token in &config.envd_use_tokens {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `repo` (real `configdict["repo"]`): each repo's own
    // `profiles/make.defaults` USE (real `_repo_make_defaults`) first,
    // then every configured repo's own `profiles/package.use` -- all
    // *before* the ebuild's own IUSE defaults, the weakest layer this
    // portuale models. Real `regenerate()` walks `configdict["repo"]["USE"]`
    // (make.defaults) ahead of that tier's package-scoped USE, stacking
    // the candidate's repo's masters chain then the repo itself. Here an
    // entry with an empty repo name applies to every package (the main
    // repo, which masters every overlay); a named entry applies only to
    // a candidate from that repo. `candidate_str`'s `::<repo>` suffix
    // names the candidate's repo.
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

    // `features` (real `configdict["features"]["USE"]`, `config.py`
    // ~2043): `FEATURES=test` appends `test` to this tier, so a package
    // that declares `test` in IUSE builds with it enabled and its
    // `test?( )` deps resolve -- the make.conf/env equivalent of
    // `--with-test-deps` (which only pulls the deps, for top-level atoms,
    // without enabling the flag). Sits between `repo` and `pkginternal`
    // in `USE_ORDER`. Only `test` is modelled (real `feature_use` never
    // holds anything else); `config.features_use` is `["test"]` or empty,
    // derived from portuale's last-wins `FEATURES` scalar. A
    // use-masked `test` is still dropped by the `use.mask` step below,
    // exactly as real portage's own `regenerate()` filtering intends.
    for token in &config.features_use {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `pkginternal`: only a token with an explicit "+"/"-" marker
    // contributes anything at all -- a markerless IUSE token (no
    // declared default) is a real, deliberate no-op here, matching real
    // config.py's own `if x.startswith("+"): ... elif x.startswith("-"):
    // ...` (no `else` branch at all).
    let iuse_defaults: String = iuse
        .split_whitespace()
        .filter(|tok| tok.starts_with('+') || tok.starts_with('-'))
        .collect::<Vec<_>>()
        .join(" ");
    portage_profile::apply_use_incremental(&iuse_defaults, &mut use_flags);

    // `defaults` (real `configdict["defaults"]`): real `regenerate()`
    // walks this tier one profile at a time -- that level's
    // `make.defaults` USE, then that level's own `package.use`, before
    // the next level -- so a child profile's `make.defaults USE="-foo"`
    // can cancel a parent's `package.use foo`. `config.profile_use_layers`
    // carries that per-level structure; the flat `config.use_tokens` +
    // `config.package_use` are the fallback for a hand-built `Config` (a test
    // helper) that never populated the layers.
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

    // `conf` (real `configdict["conf"]`): `make.conf` USE, then the
    // `USE_EXPAND`/`USE_EXPAND_UNPREFIXED` folded values.
    for token in &config.conf_use_tokens {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `pkg` (real `configdict["pkg"]`): `package.env`'s own `USE=` first
    // (real `_grab_pkg_env` fills `pkg_configdict`), then the user-level
    // `/etc/portage/package.use` on top (real `config.py:2042-2048`
    // appends `self.puse` after) -- so a user `package.use` flag wins
    // over a `package.env` one.
    apply_matching(&mut use_flags, &config.package_env_use);
    apply_matching(&mut use_flags, &config.package_use_user);

    // `env` (real `configdict["env"]`, the highest `USE_ORDER` tier):
    // the process-environment `USE="..."` -- `USE="-X" emerge foo`
    // overrides even a user `/etc/portage/package.use` flag. The tokens
    // keep their `+`/`-` incremental syntax. This is the strongest layer
    // before the final `use.force`/`use.mask` step below.
    for token in &config.env_use_tokens {
        portage_profile::apply_use_incremental(token, &mut use_flags);
    }

    // `--autounmask-use` (real `_dynamic_config._needed_use_config_changes`,
    // fed back into the `'backtrack` loop by `_feedback_config`): a USE
    // change the resolver has already decided is needed to make the graph
    // resolve. Applied here -- after `env`, before `use.force`/`use.mask`
    // -- as the top USE tier, so a package under an autounmask flip is
    // walked with the flipped flag and its `flag?`-gated deps appear.
    // Normally empty; see `Config::autounmask_use`.
    apply_matching(&mut use_flags, &config.autounmask_use);

    // `_*` wildcard USE_EXPAND expansion (real `config.py` `setcpv`
    // ~2242): once `package.use` has been applied, a `k_*` flag still in
    // the set (from `USE="linguas_*"`, `LINGUAS="*"` folding, or a
    // `package.use` `LINGUAS: *` shorthand) means "enable every `k_<x>`
    // flag declared in THIS candidate's own `IUSE`" -- the per-package
    // expansion the IUSE-blind global config layer can't do. Any masked
    // `k_<x>` is dropped again by the `use.mask` steps below, exactly as
    // real portage's own `x not in usemask` guard intends. Deliberately
    // NOT guarded on `k` actually being a `USE_EXPAND` variable name
    // (real portage's own `use_expand_split` check): a `_*`-suffixed
    // token in portuale's USE set only ever originates from
    // `USE_EXPAND` folding or `package.use`'s own `USE_EXPAND` shorthand.
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

    // use.force/use.mask (+ .stable. variants when the candidate is
    // stable) layered on top of package.use as the literal last step of
    // the incremental walk, `myflags.update(useforce)` then
    // `myflags.difference_update(usemask)` -- real `regenerate()`'s own
    // final two lines, where `setcpv()` set `self.useforce`/`self.usemask`
    // to the *per-package* `getUseForce(pkg)`/`getUseMask(pkg)`.
    // `resolved_use_mask_or_force` reproduces each of those as its own
    // per-profile-level *interleaved* stack (see its doc comment): a
    // global `use.mask` `-flag` at a later profile level cancels an
    // earlier level's `package.use.mask` entry, which the old flat
    // `config.use_mask` ∪ `package_use_mask` application could not.
    // Uncached-build path only (EUF misses): the shared sets are read,
    // the few forced/masked flags cloned in.
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
    // The `k_*` pseudo-flags themselves are not real USE flags -- real
    // portage strips every `_*`-suffixed token from `PORTAGE_USE`
    // (config.py ~2260) once they've done their expansion job above.
    use_flags.retain(|f| !f.ends_with("_*"));
    use_flags
}

/// Computes the final per-candidate flag set from `entries` (raw
/// `package.use.mask`/`.force`/`package.accept_keywords` `(atom, tokens)`
/// pairs, already in profile-chain / source order): filters to entries
/// whose atom matches `candidate_str`, then applies each match's own
/// tokens in **source order** via the same incremental
/// `-flag`/`flag`/`+flag` semantics `package.use` itself uses
/// (`apply_incremental`), onto `seed` -- so a `-flag` in a *later*
/// (child) profile cancels a `flag` mask/force from an *earlier* (parent)
/// one, mirroring real portage's own `stack_lists(incremental=True)` over
/// `[src0, src1, ...]` (`getUseMask`/`getUseForce`/`getPKeywords`).
///
/// Real `stack_lists` runs `ordered_by_atom_specificity` **within each
/// source** before stacking, so two atoms of differing specificity *in
/// the same file* listed specific-before-general would be reordered.
/// portuale processes plain file order instead (the same shortcut
/// `apply_matching`/`package.use` already takes) -- correct for every
/// real profile, which lists general-before-specific by convention, and
/// crucially correct for the `targets/*` pattern (a broad `cat/pkg
/// -flag` in a child target un-masking a narrow `>=cat/pkg-N flag` from
/// `base`), which a global specificity sort gets backwards.
///
/// `seed` is empty for every `package.use.mask`/`.force` caller;
/// `keywords_accepted` seeds it with `ACCEPT_KEYWORDS` (real
/// `KeywordsManager.getMissingKeywords`'s `pgroups`).
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

/// Arguments of [`resolved_use_mask_or_force`].
pub(crate) struct ResolvedUseMaskOrForceArgs<'a> {
    pub(crate) which: MaskOrForce,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) stable: bool,
}

/// Real `UseManager.getUseMask(pkg)` / `getUseForce(pkg)`: the four
/// (or, for a non-stable candidate, two) sources stacked **interleaved
/// per profile level** and then collapsed with one incremental
/// `stack_lists` -- so a global `use.mask` `-flag` line at a later
/// profile level cancels an earlier level's `package.use.mask` entry
/// (the `dev-libs/glib sysprof` / `arch/amd64 -sysprof` case), which the
/// flat `Config::use_mask` ∪ `package_use_mask` union structurally
/// cannot. Per level, in real's own order: global `use.mask`, global
/// `use.stable.mask` (stable only), `package.use.mask` (atom-specificity
/// ordered within the level via `specificity_ordered_flags`),
/// `package.use.stable.mask` (stable only).
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
    // Memoise per (which, stable, config USE context, candidate): the
    // per-level walk below rebuilds the same small set tens of thousands
    // of times per resolve (29,774 calls on the reference workload, each
    // fanning into `specificity_ordered_flags` /
    // `config_entries_matching`). Sound because `use_mask_force_levels`
    // is built once at config resolution and the `'backtrack` loop
    // mutates only `autounmask_use`, which this function never reads --
    // the fingerprint is included for defence in depth. Thread-local so
    // the lookup stays lock-free. Backlog #122: `category`/`package` are
    // key components in their own right, not merely implied by
    // `candidate_str` -- every current caller builds that string from
    // these same two values, but the coupling was conventional rather
    // than structural.
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
/// Arguments of [`resolved_use_mask_or_force_uncached`].
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

/// Arguments of [`forced_or_masked_flags`].
pub struct ForcedOrMaskedFlagsArgs<'a> {
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
    pub config: &'a portage_profile::Config,
}

/// Real `_display_use`'s own `self.forced_flags = pkg.use.force |
/// pkg.use.mask` (fed to `map_to_use_expand(..., forced_flags=True)`),
/// restricted to `iuse`'s own declared flags: the set of this candidate's
/// IUSE flags that `emerge --pretend -v` wraps in `( … )` because they're
/// force-enabled or mask-disabled and so not actually under the user's
/// control. Built from the exact same `use.force`/`use.mask` +
/// `package.use.force`/`.mask` (+ the stable variants when the candidate
/// is stable) layering `effective_use_flags` already applies -- reusing
/// `specificity_ordered_flags` so a more-specific `-flag` cancels a
/// less-specific force/mask identically.
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

/// The same per-level force/mask stack **without** the IUSE
/// intersection -- real `depgraph.py:8264`/`:8268-8270` passes
/// `chain(myeb.use.force, myeb.use.mask)` / `chain(pkgsettings.useforce,
/// pkgsettings.usemask)` to `_reinstall_for_flags` verbatim
/// (`config.setcpv`'s `getUseForce(cpv_slot)`/`getUseMask(cpv_slot)`,
/// `portage/package/ebuild/config.py:1973-1980`, never filters by the
/// package's IUSE), and `:3151`'s `flags -= forced_flags` must forgive a
/// profile-masked flag the new ebuild's IUSE has dropped (audit D1,
/// `LLM/02.68-74-review-through-B5.md`; probed live against
/// 3.0.82.2). The [`forced_or_masked_flags`] IUSE filter stays for the
/// display paths that render `( … )` over a package's own IUSE.
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
    // Owned union of the two shared sets (both usually cache hits now);
    // the union itself is small (a handful of forced/masked flags).
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

/// Simplified port of real `best_match_to_list`'s own specificity
/// ranking table: versioned/slotted bare atoms and portuale's own
/// bounded wildcard atoms only, matching `portage-dep`'s v1 grammar
/// scope. A bounded wildcard atom (`*/*`, `category/*`, `*/package`)
/// always ranks below every real atom, at `-2` -- real portage's own
/// code has three separate extended-syntax tiers (`0` for a wildcard
/// combined with `=*`, `-1` for one combined with a slot, `-2` for a
/// bare one with neither), and portuale's own wildcard grammar has no
/// slot or glob concept at all, so every wildcard entry always falls
/// into that third, lowest tier.
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
