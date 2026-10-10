//! Keyword, mask and license visibility of a candidate.

use crate::*;

/// Arguments of [`metadata_key_accepted`].
pub(crate) struct MetadataKeyAcceptedArgs<'a> {
    pub(crate) value_str: &'a str,
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) candidate_str: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) which: MetadataKey,
    pub(crate) global_accept: &'a [String],
    pub(crate) package_accept: &'a [(String, Vec<String>)],
}

/// Whether every token in `value_str` (a candidate's own real
/// `PROPERTIES`/`RESTRICT` metadata) is accepted -- real
/// `_getMissingProperties`/`_getMissingRestrict`, ported as a bool.
/// Unlike `LICENSE` (which needs `||`-group *structure* -- see
/// `has_masked_license`'s own doc comment), `PROPERTIES`/`RESTRICT` have
/// no any-of semantics at all: real config.py's own comment says it
/// plainly, "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without
/// groups" -- every flattened token individually needs to be accepted,
/// so this reuses `use_reduce_flat` directly instead of the bespoke
/// `LicenseNode` tree (no `||`-structure to lose in the first place).
pub(crate) fn metadata_key_accepted(args: MetadataKeyAcceptedArgs<'_>) -> bool {
    let MetadataKeyAcceptedArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
        which,
        global_accept,
        package_accept,
    } = args;
    if value_str.trim().is_empty() {
        return true;
    }
    // Memoised per (which, config USE context **and the candidate's own
    // IUSE/KEYWORDS**, candidate, value, accept lists): the body runs
    // `use_flags_if_conditional` (-> the already memoised
    // `effective_use_flags`), `resolve_accept_tokens`,
    // `all_mentioned_tokens` (a `MatchMode::All` use-reduce),
    // `resolve_acceptable_tokens` and `use_reduce_flat`, and it was the
    // last remaining hot function of that shape after #102-#108 (23.30 %
    // of the reference workload with children in S0's re-profile). Pure
    // in its arguments; thread-local so the lookup stays lock-free, the
    // `RUMF_CACHE` shape (#104). The empty-value early return above stays
    // *before* the lookup: it is cheaper than hashing the key.
    //
    // `iuse`/`keywords` go in the key for the same reason `EUF_CACHE`
    // hashes them: `candidate_str` does not encode them, and a binary
    // candidate from a `Packages` index shares it with the ebuild for the
    // same CPV while carrying its build-time IUSE/KEYWORDS.
    type MkaCache = HashMap<(MetadataKey, u64, String, String, u64), bool>;
    thread_local! {
        static MKA_CACHE: RefCell<MkaCache> = RefCell::new(HashMap::new());
    }
    let use_context = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use_context_fingerprint(config).hash(&mut h);
        candidate.iuse.hash(&mut h);
        candidate.keywords.hash(&mut h);
        h.finish()
    };
    let key = (
        which,
        use_context,
        candidate_str.to_string(),
        value_str.to_string(),
        accept_lists_fingerprint(global_accept, package_accept),
    );
    if let Some(hit) = MKA_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return hit;
    }
    let result = metadata_key_accepted_uncached(MetadataKeyAcceptedUncachedArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
        global_accept,
        package_accept,
    });
    MKA_CACHE.with(|c| {
        c.borrow_mut().insert(key, result);
    });
    result
}

/// Which accept-list pair a [`metadata_key_accepted`] call resolves:
/// `PROPERTIES` uses `accept_properties`/`package_properties`,
/// `RESTRICT` uses `accept_restrict`/`package_accept_restrict`. Every
/// call site passes exactly one of the two pairs (audited for #110), so
/// this discriminant -- rather than inferring the pair from the slices --
/// is the memo's identity for them.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MetadataKey {
    Properties,
    Restrict,
}

/// A content fingerprint of the two accept-list arguments. The
/// `MKA_CACHE` key hashes them because `use_context_fingerprint` covers
/// only the USE-relevant fields: two configs that differ *only* in
/// `accept_properties` (or `accept_restrict`) share a fingerprint, and
/// the crate's own masking unit tests build exactly such pairs in one
/// process -- without this they would collide and become test-order
/// dependent. Production has one config per process, so the cost is a
/// handful of short strings per call; hashing fully (not sampled) keeps
/// the key exact for the sizes these lists actually have.
pub(crate) fn accept_lists_fingerprint(
    global_accept: &[String],
    package_accept: &[(String, Vec<String>)],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    global_accept.hash(&mut h);
    package_accept.hash(&mut h);
    h.finish()
}

#[cfg(test)]
thread_local! {
    pub(crate) static MKA_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn metadata_key_accepted_uncached_calls() -> u64 {
    MKA_UNCACHED_CALLS.with(std::cell::Cell::get)
}
/// Arguments of [`metadata_key_accepted_uncached`].
pub(crate) struct MetadataKeyAcceptedUncachedArgs<'a> {
    pub(crate) value_str: &'a str,
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) candidate_str: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) global_accept: &'a [String],
    pub(crate) package_accept: &'a [(String, Vec<String>)],
}

pub(crate) fn metadata_key_accepted_uncached(args: MetadataKeyAcceptedUncachedArgs<'_>) -> bool {
    let MetadataKeyAcceptedUncachedArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
        global_accept,
        package_accept,
    } = args;
    #[cfg(test)]
    MKA_UNCACHED_CALLS.with(|c| c.set(c.get() + 1));
    let use_flags = use_flags_if_conditional(UseFlagsIfConditionalArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
    });
    let accept_tokens = resolve_accept_tokens(
        global_accept,
        package_accept,
        candidate_str,
        category,
        package,
    );
    let Ok(all_mentioned) = all_mentioned_tokens(value_str) else {
        return false;
    };
    let acceptable = resolve_acceptable_tokens(&accept_tokens, &all_mentioned);

    let tokens: Vec<String> = value_str.split_whitespace().map(String::from).collect();
    match portage_use_reduce::use_reduce_flat(
        &tokens,
        &use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) {
        Ok(flat) => flat.iter().all(|t| acceptable.contains(t)),
        Err(_) => false,
    }
}

/// #114: the config side of the `is_visible` memo key -- the frozen
/// visibility base (`portage_profile::is_visible_base_fingerprint`:
/// masks, accept lists, `package.accept_keywords`, `license_groups`,
/// plus the USE-context base) and the live `autounmask_use`, which the
/// visibility check reaches through `use_flags_if_conditional`. A
/// hand-built config without a base falls back to the live digest.
/// Backlog #121: same frozen-base `debug_assert_eq!` as
/// `use_context_fingerprint` -- a post-resolve mutation of any covered
/// field fails loudly instead of poisoning the memo.
pub(crate) fn is_visible_fingerprint(config: &portage_profile::Config) -> u64 {
    use std::hash::{Hash, Hasher};
    let base = match config.is_visible_base {
        Some(base) => {
            debug_assert_eq!(
                base,
                portage_profile::is_visible_base_fingerprint(config),
                "a visibility field was mutated after resolve_config froze its digest"
            );
            base
        }
        None => portage_profile::is_visible_base_fingerprint(config),
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    base.hash(&mut h);
    config.autounmask_use.hash(&mut h);
    h.finish()
}

/// A candidate is visible if it isn't masked (matches a `package.mask`
/// entry and no `package.unmask` entry), its KEYWORDS intersect the
/// accepted set -- the global `config.accept_keywords`, plus any extra
/// keywords contributed by a matching `package.accept_keywords` entry,
/// with a `"**"` token in such an entry meaning "accept unconditionally"
/// for matching candidates (even ones with empty/no KEYWORDS) -- and its
/// own declared `LICENSE`/`PROPERTIES`/`RESTRICT` are all fully accepted
/// (see `license_accepted`/`metadata_key_accepted`) -- real `Package.py`'s
/// own `_masks` dict collects `package.mask`, `LICENSE`, `PROPERTIES`,
/// and `RESTRICT` as four independent masking reasons the same way.
///
/// Memoised per (config visibility fingerprint, candidate identity and
/// full metadata): the reference workload calls it ~40 k times against
/// 1,503 distinct inputs (`#114`), and each call re-formats the
/// candidate string, walks `package.mask`/`.unmask` and re-derives the
/// license/keyword/property/restrict verdicts. Pure in its arguments;
/// thread-local so the lookup stays lock-free (`EUF_CACHE`'s shape). The
/// candidate's `license`/`properties`/`restrict`/`iuse`/`keywords` are
/// part of the key because a `Packages`-index binary and the ebuild for
/// the same `candidate_str` can carry different metadata. Backlog #122:
/// the literal `candidate_str` bounds any 64-bit digest collision to
/// same-candidate inputs (`EUF_CACHE`'s shape).
pub fn is_visible(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    type IvCache = HashMap<(u64, u64, String), bool>;
    thread_local! {
        static IV_CACHE: RefCell<IvCache> = RefCell::new(HashMap::new());
    }
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let key = {
        use std::hash::{Hash, Hasher};
        let config_fp = is_visible_fingerprint(config);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        config_fp.hash(&mut h);
        category.hash(&mut h);
        package.hash(&mut h);
        candidate.version.hash(&mut h);
        candidate.slot.hash(&mut h);
        candidate.sub_slot.hash(&mut h);
        candidate.repo_name.hash(&mut h);
        candidate.license.hash(&mut h);
        candidate.properties.hash(&mut h);
        candidate.restrict.hash(&mut h);
        candidate.iuse.hash(&mut h);
        candidate.keywords.hash(&mut h);
        // Backlog #153: the `invalid` IUSE-conditional verdict depends on
        // the instance's own dep keys, so same-version binary instances
        // (distinct `BUILD_ID`s, possibly distinct dep metadata) must not
        // share a memo entry. Ebuild candidates need nothing extra: one
        // version is one ebuild is one DEPEND.
        candidate.build_id.hash(&mut h);
        candidate.build_time.hash(&mut h);
        (config_fp, h.finish(), candidate_str.clone())
    };
    if let Some(hit) = IV_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return hit;
    }
    let result = is_visible_uncached(candidate, category, package, config);
    IV_CACHE.with(|c| {
        c.borrow_mut().insert(key, result);
    });
    result
}

#[cfg(test)]
thread_local! {
    pub(crate) static IV_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn is_visible_uncached_calls() -> u64 {
    IV_UNCACHED_CALLS.with(std::cell::Cell::get)
}

pub(crate) fn is_visible_uncached(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    #[cfg(test)]
    IV_UNCACHED_CALLS.with(|c| c.set(c.get() + 1));
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    let masked = any_config_entry_matches(&config.package_mask, &candidate_str, category, package)
        && !any_config_entry_matches(&config.package_unmask, &candidate_str, category, package);
    if masked {
        return false;
    }

    if !license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        return false;
    }

    if !keywords_accepted(KeywordsAcceptedArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    }) {
        return false;
    }

    // Backlog #153: real masks a candidate whose own dep conditionals
    // reference flags outside its effective IUSE (`invalid`), after
    // every `_getmaskingstatus` reason -- so this check sits last here
    // too. Not relaxable (real has no autounmask for it): see
    // `visible_with_relax`, which enforces it unconditionally as well.
    invalid_use_conditional_reasons(candidate, category, package, config).is_empty()
}

/// Arguments of [`visible_with_relax`].
pub(crate) struct VisibleWithRelaxArgs<'a> {
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) relax_license: bool,
    pub(crate) relax_keywords: bool,
    pub(crate) relax_masks: bool,
}

/// Whether `candidate` would pass [`is_visible`] once the named masking
/// categories are set aside -- real `_pkg_visibility_check(pkg,
/// autounmask_level)`. `relax_license` / `relax_keywords` / `relax_masks`
/// mirror `_AutounmaskLevel`'s `allow_license_changes` /
/// `allow_unstable_keywords` / `allow_unmasks`. With all three `false`
/// this is exactly `is_visible`; the single-flag `*_masked_only` fns are
/// the "and *only* this category is wrong" special cases (kept for their
/// other callers). `resolve_pretend` walks the `_autounmask_levels`
/// sequence of these, re-scanning every version at each level, so a
/// candidate blocked by *two* categories (e.g. `~arch` **and** `LICENSE`)
/// still resolves once both relaxations are in play.
pub(crate) fn visible_with_relax(args: VisibleWithRelaxArgs<'_>) -> bool {
    let VisibleWithRelaxArgs {
        candidate,
        category,
        package,
        config,
        relax_license,
        relax_keywords,
        relax_masks,
    } = args;
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    if !relax_masks {
        let masked =
            any_config_entry_matches(&config.package_mask, &candidate_str, category, package)
                && !any_config_entry_matches(
                    &config.package_unmask,
                    &candidate_str,
                    category,
                    package,
                );
        if masked {
            return false;
        }
    }

    if !relax_license && !license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        return false;
    }

    // Backlog #153: the `invalid` IUSE-conditional mask is never
    // relaxed (real offers no autounmask for it) -- enforced at every
    // autounmask level exactly like the unrelaxed `is_visible` does.
    if !invalid_use_conditional_reasons(candidate, category, package, config).is_empty() {
        return false;
    }

    relax_keywords
        || keywords_accepted(KeywordsAcceptedArgs {
            keywords: &candidate.keywords,
            candidate_str: &candidate_str,
            category,
            package,
            accept_keywords: &config.accept_keywords,
            package_accept_keywords: &config.package_accept_keywords,
        })
}

/// `--autounmask`'s own keyword-suggestion sub-feature (real
/// `--autounmask-keep-keywords=n`, see `resolve_pretend_graph`'s doc
/// comment for the full on/off default-resolution logic portuale
/// ported): true iff `candidate` would be `is_visible` except for its
/// own KEYWORDS -- every other check `is_visible` makes (package.mask,
/// license, properties, restrict) passes, only `keywords_accepted`
/// fails. Duplicates `is_visible`'s own body rather than refactoring it
/// to return a reason enum -- real portage's own `_get_masking_status`
/// is considerably more elaborate (distinguishing package.mask/license/
/// keyword/REQUIRED_USE/etc. reasons, each with its own "unmask hint"),
/// and portuale only needs the single "keywords, and only keywords"
/// question for its own deliberately narrow v1 (see
/// `resolve_pretend_graph`'s own doc comment for the full scope
/// writeup, including what's deliberately still out: package.mask/
/// license/USE suggestions, real portage's own exact suggested-atom
/// syntax and dependency-chain-comment formatting).
pub(crate) fn keyword_masked_only(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    let masked = any_config_entry_matches(&config.package_mask, &candidate_str, category, package)
        && !any_config_entry_matches(&config.package_unmask, &candidate_str, category, package);
    if masked {
        return false;
    }

    if !license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        return false;
    }

    if keywords_accepted(KeywordsAcceptedArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    }) {
        return false;
    }
    // Backlog #153: "masked by X alone" also requires the candidate not
    // be `invalid` -- an invalid package is never visible-except-X.
    invalid_use_conditional_reasons(candidate, category, package, config).is_empty()
}

/// `--autounmask-keep-masks=n`'s own v1 slice, the `package.mask`
/// analogue of `keyword_masked_only`: true iff `candidate` matches a
/// `package.mask` entry (and no `package.unmask`) but every *other*
/// `is_visible` check (KEYWORDS, LICENSE, PROPERTIES, RESTRICT) passes.
/// Real portage keeps masks by default (`autounmask_keep_masks` defaults
/// `True`); only `--autounmask-keep-masks=n` unmasks -- and even then
/// portuale's v1 doesn't reproduce real's `# <filename>:` + masking-
/// comment lines (portuale's `package_mask` list carries no source-file
/// / comment provenance), only the `#required by` dep chain + `=<cpv>`.
pub(crate) fn mask_masked_only(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let masked = any_config_entry_matches(&config.package_mask, &candidate_str, category, package)
        && !any_config_entry_matches(&config.package_unmask, &candidate_str, category, package);
    if !masked {
        return false;
    }
    if !keywords_accepted(KeywordsAcceptedArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    }) {
        return false;
    }
    if !license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }
    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        return false;
    }
    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        return false;
    }
    // Backlog #153: "masked by X alone" also requires the candidate not
    // be `invalid` -- an invalid package is never visible-except-X.
    invalid_use_conditional_reasons(candidate, category, package, config).is_empty()
}

/// The best `--autounmask-keep-masks=n` candidate for `category/package`,
/// if any: the highest-versioned candidate masked by `package.mask`
/// alone (`mask_masked_only`), as its version string.
pub(crate) fn suggested_mask_candidate(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<String> {
    list_candidates(repos, category, package)
        .ok()?
        .iter()
        .filter(|c| mask_masked_only(c, category, package, config))
        .max_by(|a, b| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
        .map(|c| c.version.clone())
}

/// `--autounmask-license`'s own v1 slice, the LICENSE analogue of
/// `keyword_masked_only`: true iff `candidate` would be `is_visible`
/// except for its own `LICENSE` -- every other `is_visible` check
/// (package.mask, KEYWORDS, PROPERTIES, RESTRICT) passes, only
/// `license_accepted` fails. Same deliberate body-duplication rationale
/// as `keyword_masked_only`.
pub(crate) fn license_masked_only(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    let masked = any_config_entry_matches(&config.package_mask, &candidate_str, category, package)
        && !any_config_entry_matches(&config.package_unmask, &candidate_str, category, package);
    if masked {
        return false;
    }

    if !keywords_accepted(KeywordsAcceptedArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    }) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        return false;
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        return false;
    }

    if license_accepted(candidate, category, package, &candidate_str, config) {
        return false;
    }
    // Backlog #153: "masked by X alone" also requires the candidate not
    // be `invalid` -- an invalid package is never visible-except-X.
    invalid_use_conditional_reasons(candidate, category, package, config).is_empty()
}

/// The keyword portuale's own `--autounmask` v1 would suggest adding
/// to `package.accept_keywords` for `candidate` -- the first of its own
/// (non-`-`-prefixed; a `-`-prefixed KEYWORDS token means "explicitly
/// unsupported here," never a valid suggestion) `KEYWORDS` tokens.
/// Deliberately simpler than real portage's own `_get_masking_status`
/// (which picks specifically between the unstable (`~arch`) form, a
/// different arch entirely, or `**`, based on exactly what's already
/// accepted) -- see `keyword_masked_only`'s own doc comment for the
/// full scope writeup this shares. `None` only if `KEYWORDS` is empty or
/// every token is `-`-prefixed (unusual, but not impossible).
pub(crate) fn suggested_keyword(candidate: &Candidate) -> Option<&str> {
    candidate
        .keywords
        .iter()
        .find(|k| !k.starts_with('-'))
        .map(String::as_str)
}

/// The best `--autounmask` keyword suggestion for `category/package`, if
/// any: among every candidate masked by `KEYWORDS` alone (`keyword_
/// masked_only`), the highest-versioned one (repo priority breaking a
/// tie), paired with its own `suggested_keyword`. `None` if `category/
/// package` isn't listable at all, or no candidate is masked by
/// `KEYWORDS` alone. Shared by both call sites that need this exact
/// "what would I suggest here" question -- a top-level atom's own fatal
/// `NoVisibleCandidate` (which turns it into part of an `Err` message)
/// and a *dependency's* own `NoVisibleCandidate` (which attaches it to
/// that `GraphEntry` instead, see `GraphEntry::keyword_suggestion`'s own
/// doc comment) -- unlike `is_visible`/`keyword_masked_only`'s own
/// deliberate duplication (which trade off different bools entirely),
/// these two calls want the exact same "best near-miss" computation, so
/// factoring it out is the right call here.
pub(crate) fn suggested_keyword_candidate(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<(String, String)> {
    let candidates = list_candidates(repos, category, package).ok()?;
    candidates
        .iter()
        .filter(|c| keyword_masked_only(c, category, package, config))
        .filter_map(|c| suggested_keyword(c).map(|k| (c, k)))
        .max_by(|(a, _), (b, _)| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
        .map(|(c, k)| (c.version.clone(), k.to_string()))
}

/// The `--autounmask-license` analogue of `suggested_keyword_candidate`:
/// among the candidates masked by `LICENSE` alone (`license_masked_only`),
/// the highest-versioned one, paired with its space-joined sorted
/// missing-license names. `None` if none is license-masked-only.
pub(crate) fn suggested_license_candidate(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<(String, String)> {
    let candidates = list_candidates(repos, category, package).ok()?;
    candidates
        .iter()
        .filter(|c| license_masked_only(c, category, package, config))
        .max_by(|a, b| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
        .map(|c| {
            let cs = format!(
                "{category}/{package}-{}:{}/{}::{}",
                c.version, c.slot, c.sub_slot, c.repo_name
            );
            (
                c.version.clone(),
                missing_licenses(c, category, package, &cs, config).join(" "),
            )
        })
}

/// Real `--autounmask-use`'s own v1 slice: true iff `candidate` would be
/// `is_visible` (package.mask/license/properties/restrict/KEYWORDS all
/// pass -- unlike `keyword_masked_only`, which explicitly *skips* the
/// keywords check, this one requires it) but the atom's own `use_deps`
/// don't match its current IUSE/effective-USE state
/// (`use_deps_satisfied`). KEYWORDS and USE-deps are two genuinely
/// independent reasons a candidate can be rejected; a candidate masked
/// by KEYWORDS too gets no USE suggestion here, matching real portage's
/// own "only suggest a change that would actually fix it" spirit
/// `keyword_masked_only`'s own doc comment already established.
pub(crate) fn use_masked_only(
    candidate: &Candidate,
    category: &str,
    package: &str,
    use_deps: &[portage_dep::UseDep],
    config: &portage_profile::Config,
) -> bool {
    if !is_visible(candidate, category, package, config) {
        return false;
    }
    let Some((iuse, use_flags)) = candidate_iuse_and_use(candidate, category, package, config)
    else {
        return false;
    };
    !portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
}

/// Arguments of [`keywords_accepted`].
pub(crate) struct KeywordsAcceptedArgs<'a> {
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) accept_keywords: &'a HashSet<String>,
    pub(crate) package_accept_keywords: &'a [(String, Vec<String>)],
}

/// The keyword-matching half of `is_visible` (everything except the
/// `package.mask`/`.unmask` check), factored out so `is_stable` below
/// can reuse it against an artificially-unstabilized keyword list
/// instead of `candidate.keywords` itself -- real `KeywordsManager.
/// isStable`/`getMissingKeywords` share this exact same matching logic
/// with real visibility checking too, just against a different input
/// keyword set, not a separate algorithm. Takes the two config pieces
/// it actually needs directly (rather than a whole `Config`) so
/// `effective_use_flags` -- which, like the rest of this file's
/// established style, takes individual pre-extracted fields rather
/// than a `Config` reference -- can call `is_stable` without needing
/// one either.
///
/// Grounded against real `KeywordsManager.getMissingKeywords`/
/// `_getEgroups` (`lib/portage/package/ebuild/_config/
/// KeywordsManager.py`): a `package.accept_keywords` entry doesn't just
/// *add* keywords on top of the global `ACCEPT_KEYWORDS` set -- real
/// `_getEgroups` folds `-token`/`-*` removals too, over the *combined*
/// list (global keywords first, then each matching entry's own tokens,
/// in atom-specificity order), so a more-specific `package.accept_
/// keywords` line can revoke a keyword the global set already granted,
/// not just add new ones. Ported here via `specificity_ordered_flags`
/// (already established for `package.use.mask`/`.force`'s own identical
/// "specificity-ordered incremental fold" shape) seeded with
/// `accept_keywords` itself, rather than the previous, incorrect
/// "union everything a matching entry ever mentions, ignore any `-`
/// prefix" accumulation. `"**"` is folded in exactly like any other
/// token now (removable by a later `-*`/`-**`), rather than a separate
/// unconditional-accept pre-scan that ignored fold order entirely --
/// once folded, its presence in the final accepted set still means
/// "accept any KEYWORDS state, even empty," the same real `"**" in
/// pgroups` unconditional-match rule portuale already documented. A
/// bare `package.accept_keywords` atom with no keyword list at all no
/// longer reaches this function empty: `resolve_config`
/// (`portage-profile`) already substitutes real `accept_keywords_
/// defaults`'s own implicit meaning -- the `~`-prefixed unstable form of
/// every currently-accepted keyword -- before this function ever sees
/// it, so it folds in through `specificity_ordered_flags` exactly like
/// any other entry's own explicit tokens would (see
/// `parse_package_accept_keywords_lines`'s own doc comment,
/// portage-profile).
///
/// A second real mechanism, previously unhandled: a literal `"*"`/`"~*"`
/// token in the accepted set means "accept any stable keyword"/"accept
/// any testing keyword" respectively -- distinct from `"**"` (accept
/// even an *empty* `KEYWORDS`) and from a plain keyword name, which
/// `apply_incremental` would otherwise insert as an inert string that
/// can never equal a real `KEYWORDS` entry. Ported from real
/// `_getMissingKeywords`'s own per-candidate-keyword loop
/// (`lib/portage/package/ebuild/_config/KeywordsManager.py`, lines
/// ~273-300): each of the candidate's own `keywords` is checked for a
/// direct match first (short-circuiting immediately, same as real
/// `match = True; break`); a `-`-prefixed one (explicit "not supported
/// here", distinct from simply absent) never matches and is excluded
/// from classification entirely, matching real portage's own elif
/// chain; anything else is classified stable or testing (`~`-prefixed)
/// for the final fallback -- `"*"` grants acceptance if *any* declared
/// keyword was stable-classified, `"~*"` if any was testing-classified,
/// matching real `(hastesting and "~*" in pgroups) or (hasstable and
/// "*" in pgroups)` exactly (the third real disjunct, `"**" in pgroups`,
/// is the unconditional check already handled above).
pub(crate) fn keywords_accepted(args: KeywordsAcceptedArgs<'_>) -> bool {
    let KeywordsAcceptedArgs {
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    } = args;
    let accepted = specificity_ordered_flags(
        package_accept_keywords,
        candidate_str,
        category,
        package,
        accept_keywords.clone(),
    );
    if accepted.contains("**") {
        return true;
    }

    // Real _getMissingKeywords's own per-keyword loop: a "-"-prefixed
    // KEYWORDS token (explicit "not supported here", distinct from
    // simply absent) never itself matches and never counts toward
    // has_stable/has_testing either -- it's excluded from every branch
    // of real portage's own elif chain the same way. A direct match
    // (the accepted set literally contains this exact keyword) wins
    // immediately; otherwise this keyword only contributes to the
    // has_stable/has_testing tally used by the "*"/"~*" fallback below.
    let mut has_stable = false;
    let mut has_testing = false;
    for k in keywords {
        if k.starts_with('-') {
            continue;
        }
        if accepted.contains(k) {
            return true;
        }
        if k.starts_with('~') {
            has_testing = true;
        } else {
            has_stable = true;
        }
    }
    (has_testing && accepted.contains("~*")) || (has_stable && accepted.contains("*"))
}

/// Arguments of [`is_stable`].
pub(crate) struct IsStableArgs<'a> {
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) accept_keywords: &'a HashSet<String>,
    pub(crate) package_accept_keywords: &'a [(String, Vec<String>)],
}

/// Whether `keywords` (a candidate's own KEYWORDS) count as "stable"
/// for the purposes of `use.stable.mask`/`.force`/`package.use.stable.
/// mask`/`.force` -- ported from real `KeywordsManager.isStable`
/// (`lib/portage/package/ebuild/_config/KeywordsManager.py`): NOT a raw
/// "no `~` prefix" check. A candidate counts as stable if replacing
/// *every* one of its own keywords with its `~`-prefixed unstable form
/// would make it invisible under the current `ACCEPT_KEYWORDS`/
/// `package.accept_keywords` -- real portage's own comment explains why:
/// "this guarantees that the effective use.force/mask settings for a
/// particular ebuild do not change when that ebuild is stabilized."
/// Reuses `keywords_accepted` (the same matching logic `is_visible`
/// itself uses for its own KEYWORDS half) against that artificially-
/// unstabilized list rather than reimplementing keyword matching a
/// second time.
pub(crate) fn is_stable(args: IsStableArgs<'_>) -> bool {
    let IsStableArgs {
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    } = args;
    let unstable: Vec<String> = keywords
        .iter()
        .map(|k| {
            if k.starts_with('~') {
                k.clone()
            } else {
                format!("~{k}")
            }
        })
        .collect();
    !keywords_accepted(KeywordsAcceptedArgs {
        keywords: &unstable,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    })
}

pub(crate) fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}
