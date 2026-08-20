//! The preserved-libs registry and its storage.

use crate::ebuild_merge::*;

/// `"cp:slot"` -> `(cpv, counter, paths)`. `orig_entries` snapshots the loaded content
/// so the store step skips the write when nothing changed.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.store
pub(crate) type PlibEntries = BTreeMap<String, (String, String, Vec<String>)>;

#[derive(Clone)]
pub(crate) struct PlibRegistry {
    pub(crate) entries: PlibEntries,
    pub(crate) orig_entries: PlibEntries,
}

/// Writes one unmerge defers so a database backend can commit them with the entry deletion.
/// `None` everywhere on `files`, where each write happens at once.
#[derive(Default)]
pub(crate) struct RetireWrites {
    pub(crate) registry: Option<PlibRegistry>,
    pub(crate) config_memory: Option<BTreeMap<String, String>>,
    pub(crate) files: Vec<(portage_vdb::EntryKey, String, Vec<u8>)>,
}

impl RetireWrites {
    /// Commits the deferred writes and the deletion of `key` in one transaction.
    pub(crate) fn commit(self, root: &Path, key: &portage_vdb::EntryKey) -> Result<(), String> {
        let db = portage_vdb::for_root(root);
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        for (entry, name, data) in &self.files {
            txn.replace_file(entry, name, data)
                .map_err(|e| e.to_string())?;
        }
        if let Some(libs) = self.registry.as_ref().and_then(plib_store) {
            txn.set_preserved_libs(&libs).map_err(|e| e.to_string())?;
        }
        if let Some(map) = self.config_memory {
            txn.set_config_memory(&portage_vdb::ConfigMemory { entries: map })
                .map_err(|e| e.to_string())?;
        }
        txn.delete_entry(key).map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::publish_hook(root);
        txn.commit().map_err(|e| e.to_string())
    }
}

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

pub(crate) fn plib_entries_to_vdb(
    entries: &PlibEntries,
) -> BTreeMap<String, portage_vdb::PreservedLibsEntry> {
    entries
        .iter()
        .map(|(key, (cpv, counter, paths))| {
            (
                key.clone(),
                portage_vdb::PreservedLibsEntry {
                    cpv: cpv.clone(),
                    counter: counter.clone(),
                    paths: paths.clone(),
                },
            )
        })
        .collect()
}

/// Stores the registry unless unchanged since load (then not even the directory is
/// created); an empty registry serializes as `{}` with no trailing newline.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.store
pub(crate) fn write_plib_registry(root: &Path, registry: &PlibRegistry) -> Result<(), String> {
    let Some(libs) = plib_store(registry) else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_preserved_libs(&libs).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// What [`write_plib_registry`] stores: `None` when unchanged since load.
pub(crate) fn plib_store(registry: &PlibRegistry) -> Option<portage_vdb::PreservedLibs> {
    (registry.entries != registry.orig_entries).then(|| portage_vdb::PreservedLibs {
        entries: plib_entries_to_vdb(&registry.entries),
        loaded: plib_entries_to_vdb(&registry.orig_entries),
    })
}
