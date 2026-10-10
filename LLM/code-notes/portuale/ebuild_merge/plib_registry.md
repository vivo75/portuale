# crates/portuale/src/ebuild_merge/plib_registry.rs
The preserved-libs registry: in-memory shape, deferred unmerge writes, load/prune/store and small path/inode helpers.

## module
- Ports Portage's `PreservedLibsRegistry` (lib/portage/util/_dyn_libs/PreservedLibsRegistry.py) and `portage.abssymlink` (lib/portage/__init__.py).

## type PlibEntries
- In-memory shape `"cp:slot"` -> `(cpv, counter, paths)`; `preserved_libs()` mirrors `getPreservedLibs()` (cpv -> paths, last entry wins on a duplicate cpv across keys -- a corner case with no relevance here).

## struct PlibRegistry
- `orig_entries` is the file content as parsed, snapshotted before `prune_non_existing` runs, for `store()`'s `self._data == self._data_orig` early return (PreservedLibsRegistry.py): `write_plib_registry` skips the write -- not even creating the parent directory -- when `entries` still equals it, so a merge/unmerge with no preserved-lib activity leaves the file's bytes **and** mtime untouched (backlog #167).
- One deliberate difference from Portage's literal comparison: Portage compares loaded JSON lists against live tuples, which are never `==` in Python, so it rewrites a non-empty registry (identical bytes -- a tuple serializes exactly like the list it was loaded from -- but a touched mtime) on every `store()`. Portuale compares semantically and skips that mtime-only rewrite; the observable bytes stay identical to Portage's.

## struct RetireWrites
- The writes of one standalone unmerge that a database backend commits together with the row's deletion (feat#157 S4.2, `retire_entry`): collected while the unmerge runs, applied in one transaction after `pkg_postrm`. `None` everywhere on `files`, where every write is made when the unmerge makes it (the S1.5 syscall sequence).
- `registry`: the preserved-libs registry as the unmerge left it in memory. `preserve_libs_on_unmerge` and the prune that follows read it back from here instead of the store (the store still holds the state before the unmerge); its `orig_entries` stay the loaded store, so the one `set_preserved_libs` writes nothing when the unmerge changed nothing.
- `config_memory`: the pruned config memory (`stale_confmem`).
- `files`: the W4 rewrites (`remove_from_contents`) of the *other* installed entries that owned preserved libraries pruned here.

## impl RetireWrites::for_root
- `Some` when `root`'s backend commits a retirement as one transaction (the database backends); `None` on `files`.

## impl RetireWrites::commit
- One transaction: the W4 rewrites, the registry (only if it changed), the config memory, and the deletion of `key`.

## fn plib_registry_for
- The registry an unmerge in progress sees: the in-memory one when an earlier step of this unmerge deferred its write, else the stored one (pruned of vanished paths either way, like `read_plib_registry`).

## impl PlibRegistry::preserved_libs
- `getPreservedLibs()` mirror (cpv -> paths); last entry wins on a duplicate cpv across keys (see `PlibEntries`).

## fn plib_registry_path
- `lib/portage/const.py`'s `PRIVATE_PATH` (`"var/lib/portage"`) joined with `PreservedLibsRegistry`'s hardcoded filename: where the `files` backend keeps the registry. Tests only; production code goes through `portage_vdb::InstalledDb::preserved_libs` (S1.4).

## fn parse_plib_registry
- The registry's file format (Portage's `json.dumps` shape) is parsed by `portage_vdb::parse_preserved_libs`. Tests only: production reads go through `read_plib_registry`.

## fn plib_entries_from_vdb
- Converts `portage_vdb::PreservedLibsEntry` records to this module's `(cpv, counter, paths)` tuples.

## fn plib_entries_to_vdb
- The reverse of `plib_entries_from_vdb`.

## fn read_plib_registry
- Ports `load()`: a missing or unparseable registry file degrades gracefully to an empty registry rather than an error. Like `load()` (PreservedLibsRegistry.py:load), the parsed snapshot is kept as `orig_entries` and `prune_non_existing` runs immediately -- every consumer below therefore sees the pruned registry, exactly as Portage's consumers of `load()` do. The stored registry comes from the root's `portage_vdb::InstalledDb::preserved_libs` (S1.4; on `files` the same one read of `var/lib/portage/preserved_libs_registry`); the `lstat`-based prune stays here (N6).

## fn norm_posix_path
- Lexical POSIX `normpath` (Portage's `os.path.normpath` use in `plib_abssymlink`): collapses `.`/`..`/duplicate separators without touching the filesystem.

## fn plib_abssymlink
- Ports `portage.abssymlink(symlink, target)` (lib/portage/__init__.py): the absolute path of a symlink's target -- the target itself when absolute, otherwise resolved against the symlink's own directory and normalized. `path` is the stored registry path (always absolute).

## fn prune_non_existing
- Ports `PreservedLibsRegistry.pruneNonExisting` (PreservedLibsRegistry.py:180-219): drops every recorded path that no longer exists on disk (`lstat` failure), rebuilds each surviving entry as regular files first (in stored order), then the symlinks whose `abssymlink` target is one of those regular files (a tool like `eselect-opengl` may have repointed a soname symlink elsewhere -- bug #406837 -- and the orphaned hardlink is found separately), and drops the entry entirely when nothing survives. Only symlinks and regular files count (Portage's `S_ISLNK`/`S_ISREG`); anything else is neither.

## fn write_plib_registry
- Ports `store()`'s `json.dumps(..., ensure_ascii=False, indent="\t", sort_keys=True)` layout (PreservedLibsRegistry.store; serialised by `portage_vdb::format_preserved_libs` and stored by the root's `WriteTxn::set_preserved_libs`, S1.4) -- `BTreeMap` already keeps keys sorted -- written via a plain `fs::write` (Portage's `atomic_ofstream` atomicity is a portuale-wide cut, not this slice's). An empty dict serializes as exactly `{}` and carries no trailing newline, matching Python's output byte-for-byte. And like `store()`, an unchanged registry (`entries == orig_entries`, see `PlibRegistry`) is not rewritten at all: not even the parent directory is created, so a packaged 0-byte file stays 0 bytes and a missing file stays missing.
- `SANDBOX_ON` is not honored: Portage checks it because its registry mutations run inside sandboxed phases, while portuale's run in portuale's own unsandboxed process -- the phases it spawns are separate bash children.

## fn plib_store
- What `write_plib_registry` stores: `None` when the registry is unchanged since it was loaded (Portage's `store()` writes nothing then).

## fn plib_inode_map
- Ports `dblink._lstat_inode_map` (vartree.py): `(st_dev, st_ino)` -> every registered `(cpv, path)` pair currently lstat-able at that inode (multiple paths may share an inode via hardlinks). A path the registry names but that no longer exists on disk is silently skipped, matching `_lstat_inode_map`'s `except OSError` -> `continue`.
