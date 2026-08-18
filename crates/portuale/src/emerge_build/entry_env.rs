//! The per-entry build environment.

use crate::emerge_build::*;

/// The `cat/pkg-ver:slot/sub` identity a build-bound entry is matched
/// against. Missing `SLOT` means slot `0`; a missing sub-slot repeats
/// the slot.
pub(crate) fn entry_cpv_slot(entry: &GraphEntry, version: &str) -> String {
    let slot = entry.slot.as_deref().unwrap_or("0");
    let sub_slot = entry.sub_slot.as_deref().unwrap_or(slot);
    format!(
        "{}/{}-{version}:{slot}/{sub_slot}",
        entry.category, entry.package
    )
}
