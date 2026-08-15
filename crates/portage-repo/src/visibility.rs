//! Keyword, mask and license visibility of a candidate.

use crate::*;

#[cfg(test)]
thread_local! {
    pub(crate) static MKA_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn metadata_key_accepted_uncached_calls() -> u64 {
    MKA_UNCACHED_CALLS.with(std::cell::Cell::get)
}

#[cfg(test)]
thread_local! {
    pub(crate) static IV_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn is_visible_uncached_calls() -> u64 {
    IV_UNCACHED_CALLS.with(std::cell::Cell::get)
}

pub(crate) fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}
