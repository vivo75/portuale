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
    /// `Some` on database backends (one-transaction retirement); `None` on `files`.
    pub(crate) fn for_root(root: &Path) -> Option<Self> {
        portage_vdb::for_root(root)
            .replace_in_publish()
            .then(Self::default)
    }

    pub(crate) fn set_config_memory(&mut self, map: BTreeMap<String, String>) {
        self.config_memory = Some(map);
    }

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

/// The registry an unmerge in progress sees: the deferred in-memory one when present,
/// else the stored one; pruned of vanished paths either way.
pub(crate) fn plib_registry_for(root: &Path, retire: Option<&RetireWrites>) -> PlibRegistry {
    match retire.and_then(|r| r.registry.as_ref()) {
        Some(registry) => {
            let mut registry = registry.clone();
            prune_non_existing(root, &mut registry);
            registry
        }
        None => read_plib_registry(root),
    }
}

impl PlibRegistry {
    pub(crate) fn preserved_libs(&self) -> BTreeMap<String, Vec<String>> {
        let mut out = BTreeMap::new();
        for (cpv, _counter, paths) in self.entries.values() {
            out.insert(cpv.clone(), paths.clone());
        }
        out
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

/// Loads the stored registry and prunes vanished paths; a missing or unparseable
/// file degrades to an empty registry.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.load
pub(crate) fn read_plib_registry(root: &Path) -> PlibRegistry {
    let parsed: PlibEntries = portage_vdb::for_root(root)
        .preserved_libs()
        .map(|libs| plib_entries_from_vdb(libs.entries))
        .unwrap_or_default();
    let mut registry = PlibRegistry {
        orig_entries: parsed.clone(),
        entries: parsed,
    };
    prune_non_existing(root, &mut registry);
    registry
}

pub(crate) fn norm_posix_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// The absolute path of a symlink's target: the target itself when absolute, else
/// resolved against the symlink's directory.
// Portage: lib/portage/__init__.py, portage.abssymlink
pub(crate) fn plib_abssymlink(path: &str, target: &str) -> String {
    if target.starts_with('/') {
        norm_posix_path(target)
    } else {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        norm_posix_path(&format!("{dir}/{target}"))
    }
}

/// Drops recorded paths no longer on disk, rebuilding survivors regular-files-first and
/// keeping only symlinks pointing at a surviving file; drops entries left with nothing.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.pruneNonExisting
pub(crate) fn prune_non_existing(root: &Path, registry: &mut PlibRegistry) {
    let mut dead_keys = Vec::new();
    for (cps, (_cpv, _counter, paths)) in registry.entries.iter_mut() {
        let mut ordered: Vec<String> = Vec::new();
        let mut hardlinks: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut symlinks: Vec<(String, String)> = Vec::new();
        for f in paths.iter() {
            let full = root.join(f.trim_start_matches('/'));
            let meta = match std::fs::symlink_metadata(&full) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                match std::fs::read_link(&full) {
                    Ok(target) => symlinks.push((f.clone(), target.to_string_lossy().into_owned())),
                    Err(_) => continue,
                }
            } else if meta.file_type().is_file() {
                hardlinks.insert(f.clone());
                ordered.push(f.clone());
            }
        }
        for (f, target) in &symlinks {
            if hardlinks.contains(&plib_abssymlink(f, target)) {
                ordered.push(f.clone());
            }
        }
        if ordered.is_empty() {
            dead_keys.push(cps.clone());
        } else {
            *paths = ordered;
        }
    }
    for key in dead_keys {
        registry.entries.remove(&key);
    }
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

/// `(st_dev, st_ino)` -> registered `(cpv, path)` pairs; un-lstat-able paths are skipped.
// Portage: vartree.py, dblink._lstat_inode_map
pub(crate) fn plib_inode_map(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
) -> HashMap<(u64, u64), Vec<(String, String)>> {
    let mut map: HashMap<(u64, u64), Vec<(String, String)>> = HashMap::new();
    for (cpv, paths) in preserved {
        for p in paths {
            let full = root.join(p.trim_start_matches('/'));
            if let Ok(meta) = std::fs::symlink_metadata(&full) {
                map.entry((meta.dev(), meta.ino()))
                    .or_default()
                    .push((cpv.clone(), p.clone()));
            }
        }
    }
    map
}
