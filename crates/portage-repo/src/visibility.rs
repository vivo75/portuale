//! Keyword, mask and license visibility of a candidate.

use crate::*;

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

pub(crate) fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}
