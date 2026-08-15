//! The preserved-libs registry and its storage.

use crate::ebuild_merge::*;

/// `"cp:slot"` -> `(cpv, counter, paths)`. `orig_entries` snapshots the loaded content
/// so the store step skips the write when nothing changed.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.store
pub(crate) type PlibEntries = BTreeMap<String, (String, String, Vec<String>)>;

/// Files-backend registry path. Tests only.
#[cfg(test)]
pub(crate) fn plib_registry_path(root: &Path) -> PathBuf {
    root.join("var/lib/portage/preserved_libs_registry")
}

/// Parses the registry file format. Tests only.
#[cfg(test)]
pub(crate) fn parse_plib_registry(text: &str) -> Option<PlibEntries> {
    portage_vdb::parse_preserved_libs(text).map(plib_entries_from_vdb)
}

pub(crate) fn plib_entries_from_vdb(
    entries: BTreeMap<String, portage_vdb::PreservedLibsEntry>,
) -> PlibEntries {
    entries
        .into_iter()
        .map(|(key, e)| (key, (e.cpv, e.counter, e.paths)))
        .collect()
}
