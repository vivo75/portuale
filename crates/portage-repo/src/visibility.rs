//! Keyword, mask and license visibility of a candidate.

use crate::*;

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

/// Whether every token in `value_str` (a candidate's `PROPERTIES` /
/// `RESTRICT` metadata) is accepted. Unlike `LICENSE` there is no
/// any-of semantics: every flattened token individually needs accepting.
/// Portage: config.py _getMissingProperties/_getMissingRestrict
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
    // Memoised per (which, USE context + candidate IUSE/KEYWORDS,
    // candidate, value, accept lists); thread-local, lock-free. The
    // empty-value early return stays before the lookup (cheaper than
    // hashing). `iuse`/`keywords` join the key because `candidate_str`
    // doesn't encode them (a binary shares it with its ebuild).
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

/// Which accept-list pair a [`metadata_key_accepted`] call resolves
/// (and the memo's identity for them).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MetadataKey {
    Properties,
    Restrict,
}

/// A content fingerprint of the two accept-list arguments (the USE
/// fingerprint doesn't cover them; without this, configs differing only
/// in accept lists would collide).
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

/// The config side of the `is_visible` memo key: the frozen visibility
/// base plus the live `autounmask_use`. A post-resolve mutation fails
/// loudly in debug instead of poisoning the memo.
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

/// A candidate is visible if it isn't `package.mask`ed (without
/// `package.unmask`), its KEYWORDS are accepted (global plus matching
/// `package.accept_keywords` entries; `"**"` accepts unconditionally),
/// and its `LICENSE`/`PROPERTIES`/`RESTRICT` are all fully accepted.
///
/// Memoised per (visibility fingerprint, candidate identity and full
/// metadata); thread-local, lock-free. The metadata joins the key
/// because a binary and the ebuild for the same string can differ: the
/// `invalid` verdict reads the instance's own dep keys (`repo_location`
/// for ebuilds, `binary_deps` for binaries) under its own `eapi`, so all
/// three join the key or a masked binary poisons its ebuild twin's lookup
/// (and vice versa).
/// Portage: Package.py, _masks
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
        // The `invalid` verdict depends on the instance's own dep keys,
        // so same-version binary instances (distinct `BUILD_ID`s) must
        // not share a memo entry — and neither may an ebuild with its
        // binary twin: ebuild deps re-read from `repo_location` while
        // binary deps come from `binary_deps`, each under its own `eapi`.
        candidate.build_id.hash(&mut h);
        candidate.build_time.hash(&mut h);
        (candidate.source == CandidateSource::Binary).hash(&mut h);
        candidate.repo_location.hash(&mut h);
        candidate.eapi.hash(&mut h);
        // Sorted so iteration order never perturbs the key.
        let mut dep_keys: Vec<(&String, &String)> = candidate.binary_deps.iter().collect();
        dep_keys.sort_unstable();
        dep_keys.hash(&mut h);
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

    // The mask check sees the candidate's binary build-id: an `=`+build-id
    // `package.mask` line masks only the equal instance (dep:2994-3008, S9).
    let masked = any_config_entry_matches_with_build_id(
        &config.package_mask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    ) && !any_config_entry_matches_with_build_id(
        &config.package_unmask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    );
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

    // A candidate whose own dep conditionals reference flags outside
    // its effective IUSE is masked (`invalid`); checked last, never
    // relaxed (see `visible_with_relax`).
    invalid_dep_reasons(candidate, category, package, config).is_empty()
}

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
/// categories are set aside. With all three `false` this is exactly
/// `is_visible`. `resolve_pretend` walks these levels in sequence, so a
/// candidate blocked by two categories still resolves once both relax.
/// Portage: _pkg_visibility_check with _AutounmaskLevel
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
        // The mask check sees the candidate's binary build-id (S9), as in
        // [`is_visible_uncached`] above.
        let masked = any_config_entry_matches_with_build_id(
            &config.package_mask,
            &candidate_str,
            candidate.build_id.as_deref(),
            category,
            package,
        ) && !any_config_entry_matches_with_build_id(
            &config.package_unmask,
            &candidate_str,
            candidate.build_id.as_deref(),
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

    // The `invalid` mask is never relaxed (Portage offers no autounmask
    // for it).
    if !invalid_dep_reasons(candidate, category, package, config).is_empty() {
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

/// True iff `candidate` would be visible except for its own KEYWORDS.
/// Duplicates `is_visible`'s body rather than returning a reason enum
/// (only this one question is needed).
/// Portage: _get_masking_status (keywords-only slice)
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

    // The mask check sees the candidate's binary build-id: an `=`+build-id
    // `package.mask` line masks only the equal instance (dep:2994-3008, S9).
    let masked = any_config_entry_matches_with_build_id(
        &config.package_mask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    ) && !any_config_entry_matches_with_build_id(
        &config.package_unmask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    );
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
    // "Masked by X alone" also requires the candidate not be `invalid`.
    invalid_dep_reasons(candidate, category, package, config).is_empty()
}

/// True iff `candidate` is masked by `package.mask` alone (every other
/// check passes). Only `--autounmask-keep-masks=n` unmasks (masks are
/// kept by default); the suggestion carries no source-file comments.
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
    // The mask check sees the candidate's binary build-id: an `=`+build-id
    // `package.mask` line masks only the equal instance (dep:2994-3008, S9).
    let masked = any_config_entry_matches_with_build_id(
        &config.package_mask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    ) && !any_config_entry_matches_with_build_id(
        &config.package_unmask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    );
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
    // "Masked by X alone" also requires the candidate not be `invalid`.
    invalid_dep_reasons(candidate, category, package, config).is_empty()
}

/// The best `--autounmask-keep-masks=n` candidate for
/// `category/package`, if any: the highest-versioned one masked by
/// `package.mask` alone.
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

/// True iff `candidate` would be visible except for its own LICENSE
/// (the `license_masked_only` analogue of `keyword_masked_only`).
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

    // The mask check sees the candidate's binary build-id: an `=`+build-id
    // `package.mask` line masks only the equal instance (dep:2994-3008, S9).
    let masked = any_config_entry_matches_with_build_id(
        &config.package_mask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    ) && !any_config_entry_matches_with_build_id(
        &config.package_unmask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    );
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
    // "Masked by X alone" also requires the candidate not be `invalid`.
    invalid_dep_reasons(candidate, category, package, config).is_empty()
}

/// The keyword `--autounmask` would suggest for `candidate`: the first
/// non-`-`-prefixed `KEYWORDS` token (`None` if empty or all `-`). A
/// `-`-prefixed token means "explicitly unsupported", never a suggestion.
pub(crate) fn suggested_keyword(candidate: &Candidate) -> Option<&str> {
    candidate
        .keywords
        .iter()
        .find(|k| !k.starts_with('-'))
        .map(String::as_str)
}

/// The best `--autounmask` keyword suggestion for `category/package`,
/// if any: among the candidates masked by KEYWORDS alone, the
/// highest-versioned one (repo priority breaking ties) with its
/// `suggested_keyword`. Shared by the top-level and dependency
/// `NoVisibleCandidate` call sites.
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

/// The `--autounmask-license` analogue: among the candidates masked
/// by LICENSE alone, the highest-versioned one with its missing
/// licenses.
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

/// True iff `candidate` is visible but the atom's use-deps don't match
/// its IUSE/effective-USE. A candidate also masked by KEYWORDS gets no
/// suggestion here (only a change that would actually fix it is
/// suggested).
/// Portage: --autounmask-use (candidate-side mechanism)
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

pub(crate) struct KeywordsAcceptedArgs<'a> {
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) accept_keywords: &'a HashSet<String>,
    pub(crate) package_accept_keywords: &'a [(String, Vec<String>)],
}

/// The keyword-matching half of `is_visible` (everything except the
/// mask check), factored out so `is_stable` can reuse it against an
/// artificially-unstabilized keyword list. Takes pre-extracted fields
/// rather than a whole `Config` so `effective_use_flags` can call
/// `is_stable` without one.
///
/// A `package.accept_keywords` entry folds `-token`/`-*` removals over
/// the combined list (global first, then matching entries in order),
/// so it can revoke a globally-granted keyword, not just add new ones;
/// `"**"` folds like any other token and still means unconditional
/// accept. A bare entry with no keyword list arrives already substituted
/// with the `~`-unstable form of every accepted keyword (see
/// `parse_package_accept_keywords_lines`, portage-profile).
///
/// `"*"`/`"~*"` in the accepted set mean "any stable/testing keyword";
/// each candidate keyword is checked for a direct match first, `-`
/// tokens never match and don't count toward either class.
/// Portage: KeywordsManager.getMissingKeywords/_getEgroups
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

    // Per-keyword loop: a `-`-prefixed token never matches and counts
    // toward neither class; a direct match wins immediately.
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

pub(crate) struct IsStableArgs<'a> {
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) accept_keywords: &'a HashSet<String>,
    pub(crate) package_accept_keywords: &'a [(String, Vec<String>)],
}

/// Whether a candidate's KEYWORDS count as "stable" for the
/// `use.stable.mask`/`.force` purposes: stable iff replacing every
/// keyword with its `~`-unstable form would make it invisible (so
/// force/mask settings don't change on stabilization). Reuses
/// `keywords_accepted` against that unstabilized list.
/// Portage: KeywordsManager.isStable
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
