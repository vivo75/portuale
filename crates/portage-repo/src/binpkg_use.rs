//! `--binpkg-respect-use` checks and binary metadata readers.

use crate::*;

#[cfg(test)]
thread_local! {
    pub(crate) static BRU_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn binpkg_respect_use_ok_uncached_calls() -> u64 {
    BRU_UNCACHED_CALLS.with(std::cell::Cell::get)
}
