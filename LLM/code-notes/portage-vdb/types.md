# crates/portage-vdb/src/types.rs
Value types of the `InstalledDb` interface: entry keys, file metadata, snapshots, world sets, preserved-libs, config memory, counters.

## enum BackendKind
- `Sqlite` is the `vdb-sqlite` feature's backend, `Redb` the `vdb-redb` one.

## struct FileMeta
- Meaning follows `docs/vdb_to_db.md` section 6.1.

## enum MetadataStamp
- Corpus rule: a stale stamp must stay stale, and a converter never adds a `metadata` file.

## struct EntryStat
- Serves what a read-only view (FUSE) needs for `getattr` and `readdir`.

## struct EntryStat::dir_mode / struct EntryImage::dir_mode
- Every backend reports permission bits only, so a `files` entry and a natively merged sqlite/redb one compare equal (backlog #307).

## struct DepRecord
- `portage-vdb` cannot reduce `USE` conditionals (design section 0.7); the caller does.

## struct PreservedLibsEntry
- Portage: `PreservedLibsRegistry._data` in the Python codebase.

## struct PreservedLibs
- `set_preserved_libs` skips the write when `entries == loaded`, mirroring Portage's `store()` `_data == _data_orig` check.

## struct ConfigMemory
- The config-protect memory (`var/lib/portage/config` on `files`): path → md5 of the last offered source (N7).
