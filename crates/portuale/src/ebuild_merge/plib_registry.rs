//! The preserved-libs registry and its storage.

use crate::ebuild_merge::*;

/// Real `PreservedLibsRegistry`'s own in-memory shape
/// (`lib/portage/util/_dyn_libs/PreservedLibsRegistry.py`): `"cp:slot"`
/// -> `(cpv, counter, paths)`. `preserved_libs()` mirrors real
/// `getPreservedLibs()` (cpv -> paths, last entry wins on a duplicate
/// cpv across keys -- a corner case with no real relevance here).
///
/// `orig_entries` is the file content as parsed, snapshotted before
/// `prune_non_existing` runs, for real `store()`'s own
/// `self._data == self._data_orig` early return (`store()` in
/// `PreservedLibsRegistry.py`): `write_plib_registry` skips the write
/// -- not even creating the parent directory -- when `entries` still
/// equals it, so a merge/unmerge with no preserved-lib activity leaves
/// the file's bytes **and** mtime untouched (backlog #167).
///
/// One deliberate difference from real's literal comparison: real
/// compares loaded JSON lists against live tuples, which are never `==`
/// in Python, so real rewrites a non-empty registry (identical bytes --
/// a tuple serializes exactly like the list it was loaded from -- but a
/// touched mtime) on every `store()`. Portuale compares semantically
/// and skips that mtime-only rewrite; the observable bytes stay
/// identical to real's.
pub(crate) type PlibEntries = BTreeMap<String, (String, String, Vec<String>)>;

#[derive(Clone)]
pub(crate) struct PlibRegistry {
    pub(crate) entries: PlibEntries,
    pub(crate) orig_entries: PlibEntries,
}

/// The writes of one standalone unmerge that a database backend commits
/// together with the row's deletion (feat#157 S4.2, `retire_entry`):
/// collected while the unmerge runs, applied in one transaction after
/// `pkg_postrm`. `None` everywhere on `files`, where every write is made
/// when the unmerge makes it (the S1.5 syscall sequence).
///
/// - `registry`: the preserved-libs registry as the unmerge left it
///   in memory. `preserve_libs_on_unmerge` and the prune that follows
///   read it back from here instead of the store (the store still holds
///   the state before the unmerge); its `orig_entries` stay the loaded
///   store, so the one `set_preserved_libs` writes nothing when the
///   unmerge changed nothing.
/// - `config_memory`: the pruned config memory (`stale_confmem`).
/// - `files`: the W4 rewrites (`remove_from_contents`) of the *other*
///   installed entries that owned preserved libraries pruned here.
#[derive(Default)]
pub(crate) struct RetireWrites {
    pub(crate) registry: Option<PlibRegistry>,
    pub(crate) config_memory: Option<BTreeMap<String, String>>,
    pub(crate) files: Vec<(portage_vdb::EntryKey, String, Vec<u8>)>,
}

impl RetireWrites {
    /// `Some` when `root`'s backend commits a retirement as one
    /// transaction (the database backends); `None` on `files`.
    pub(crate) fn for_root(root: &Path) -> Option<Self> {
        portage_vdb::for_root(root)
            .replace_in_publish()
            .then(Self::default)
    }

    pub(crate) fn set_config_memory(&mut self, map: BTreeMap<String, String>) {
        self.config_memory = Some(map);
    }

    /// One transaction: the W4 rewrites, the registry (only if it
    /// changed), the config memory, and the deletion of `key`.
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

/// The registry an unmerge in progress sees: the in-memory one when an
/// earlier step of this unmerge deferred its write, else the stored one
/// (pruned of vanished paths either way, like `read_plib_registry`).
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

/// Real `lib/portage/const.py`'s own `PRIVATE_PATH` (`"var/lib/portage"`)
/// joined with `PreservedLibsRegistry`'s own hardcoded filename: where
/// the `files` backend keeps the registry. Tests only; production code
/// goes through [`portage_vdb::InstalledDb::preserved_libs`] (S1.4).
#[cfg(test)]
pub(crate) fn plib_registry_path(root: &Path) -> PathBuf {
    root.join("var/lib/portage/preserved_libs_registry")
}

/// The registry's file format (real `json.dumps` shape) is parsed by
/// [`portage_vdb::parse_preserved_libs`]. Tests only: production reads go
/// through [`read_plib_registry`].
#[cfg(test)]
pub(crate) fn parse_plib_registry(text: &str) -> Option<PlibEntries> {
    portage_vdb::parse_preserved_libs(text).map(plib_entries_from_vdb)
}

/// [`portage_vdb::PreservedLibsEntry`] records as this module's
/// `(cpv, counter, paths)` tuples.
pub(crate) fn plib_entries_from_vdb(
    entries: BTreeMap<String, portage_vdb::PreservedLibsEntry>,
) -> PlibEntries {
    entries
        .into_iter()
        .map(|(key, e)| (key, (e.cpv, e.counter, e.paths)))
        .collect()
}

/// The reverse of [`plib_entries_from_vdb`].
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

/// Real `load()`: a missing or unparseable registry file degrades
/// gracefully to an empty registry rather than an error. Like real
/// `load()` (`PreservedLibsRegistry.py:load`), the parsed snapshot is
/// kept as `orig_entries` and `prune_non_existing` runs immediately --
/// every consumer below therefore sees the pruned registry, exactly as
/// real consumers of `load()` do. The stored registry comes from the
/// root's [`portage_vdb::InstalledDb::preserved_libs`] (S1.4; on `files`
/// the same one read of `var/lib/portage/preserved_libs_registry`); the
/// `lstat`-based prune stays here (N6).
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

/// Lexical POSIX `normpath` (real `os.path.normpath`), for
/// `plib_abssymlink` below: collapses `.`/`..`/duplicate separators
/// without touching the filesystem.
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

/// Real `portage.abssymlink(symlink, target)` (`lib/portage/__init__.py`):
/// the absolute path of a symlink's target -- the target itself when
/// absolute, otherwise resolved against the symlink's own directory and
/// normalized. `path` is the stored registry path (always absolute).
pub(crate) fn plib_abssymlink(path: &str, target: &str) -> String {
    if target.starts_with('/') {
        norm_posix_path(target)
    } else {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        norm_posix_path(&format!("{dir}/{target}"))
    }
}

/// Real `PreservedLibsRegistry.pruneNonExisting`
/// (`PreservedLibsRegistry.py:180-219`): drop every recorded path that
/// no longer exists on disk (`lstat` failure), rebuild each surviving
/// entry as regular files first (in stored order), then the symlinks
/// whose `abssymlink` target is one of those regular files (a tool like
/// `eselect-opengl` may have repointed a soname symlink elsewhere -- bug
/// #406837 -- and the orphaned hardlink is found separately), and drop
/// the entry entirely when nothing survives. Only symlinks and regular
/// files count (real `S_ISLNK`/`S_ISREG`); anything else is neither.
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

/// Real `store()`'s own `json.dumps(..., ensure_ascii=False,
/// indent="\t", sort_keys=True)` layout (`PreservedLibsRegistry.store`;
/// serialised by [`portage_vdb::format_preserved_libs`] and stored by
/// the root's `WriteTxn::set_preserved_libs`, S1.4)
/// -- `BTreeMap` already keeps keys sorted -- written via a plain
/// `fs::write` (real `atomic_ofstream`'s own atomicity is a
/// portuale-wide cut, not this slice's). An empty dict serializes as
/// exactly `{}` and carries no trailing newline, matching Python's own
/// output byte-for-byte. And like real `store()`, an unchanged registry
/// (`entries == orig_entries`, see `PlibRegistry`) is not rewritten at
/// all: not even the parent directory is created, so a packaged 0-byte
/// file stays 0 bytes and a missing file stays missing.
///
/// (`SANDBOX_ON` is not honored: real checks it because its own
/// registry mutations run inside sandboxed phases, while portuale's run
/// in portuale's own unsandboxed process -- the phases it spawns are
/// separate bash children.)
pub(crate) fn write_plib_registry(root: &Path, registry: &PlibRegistry) -> Result<(), String> {
    let Some(libs) = plib_store(registry) else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_preserved_libs(&libs).map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// What [`write_plib_registry`] stores: `None` when the registry is
/// unchanged since it was loaded (real `store()` writes nothing then).
pub(crate) fn plib_store(registry: &PlibRegistry) -> Option<portage_vdb::PreservedLibs> {
    (registry.entries != registry.orig_entries).then(|| portage_vdb::PreservedLibs {
        entries: plib_entries_to_vdb(&registry.entries),
        loaded: plib_entries_to_vdb(&registry.orig_entries),
    })
}

/// Real `_lstat_inode_map`: `(st_dev, st_ino)` -> every registered
/// `(cpv, path)` pair currently lstat-able at that inode (multiple paths
/// may share an inode via hardlinks). A path the registry names but that
/// no longer exists on disk is silently skipped, matching real
/// `_lstat_inode_map`'s own `except OSError` -> `continue`.
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
