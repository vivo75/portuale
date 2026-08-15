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

#[cfg(test)]
thread_local! {
    pub(crate) static IV_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn is_visible_uncached_calls() -> u64 {
    IV_UNCACHED_CALLS.with(std::cell::Cell::get)
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

pub(crate) fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}
