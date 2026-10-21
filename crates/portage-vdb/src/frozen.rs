//! Per-thread "the VDB does not change now" scope.
//!
//! The `files` backend re-`statx`es a package or category directory on every
//! read to validate its memo. A resolve never writes the VDB, so inside a
//! [`freeze`] scope each memo entry is validated once per epoch instead.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static EPOCH: Cell<u64> = const { Cell::new(0) };
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Guard returned by [`freeze`]; leaves the scope when dropped.
#[must_use]
pub struct FrozenScope(());

/// Enter a scope in which the VDB is assumed unchanged on this thread.
/// Nested scopes share the outer epoch.
pub fn freeze() -> FrozenScope {
    DEPTH.with(|d| {
        if d.get() == 0 {
            EPOCH.with(|e| e.set(NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)));
        }
        d.set(d.get() + 1);
    });
    FrozenScope(())
}

impl Drop for FrozenScope {
    fn drop(&mut self) {
        DEPTH.with(|d| {
            d.set(d.get() - 1);
            if d.get() == 0 {
                EPOCH.with(|e| e.set(0));
            }
        });
    }
}

/// The current epoch, `0` outside a scope (always revalidate).
pub(crate) fn epoch() -> u64 {
    EPOCH.with(Cell::get)
}

/// A VDB write ran on this thread: start a new epoch so every memo
/// revalidates once more.
pub(crate) fn invalidate() {
    if DEPTH.with(Cell::get) > 0 {
        EPOCH.with(|e| e.set(NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_zero_outside_and_stable_inside_nested_scopes() {
        assert_eq!(epoch(), 0);
        let outer = freeze();
        let e = epoch();
        assert_ne!(e, 0);
        {
            let _inner = freeze();
            assert_eq!(epoch(), e);
        }
        assert_eq!(epoch(), e);
        invalidate();
        assert_ne!(epoch(), e);
        drop(outer);
        assert_eq!(epoch(), 0);
    }
}
