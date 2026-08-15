//! The preserved-libs registry and its storage.

use crate::ebuild_merge::*;

/// `"cp:slot"` -> `(cpv, counter, paths)`. `orig_entries` snapshots the loaded content
/// so the store step skips the write when nothing changed.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.store
pub(crate) type PlibEntries = BTreeMap<String, (String, String, Vec<String>)>;
