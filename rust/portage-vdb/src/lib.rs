//! The installed-package database ("VDB") behind one interface
//! (feat#157, backlog #305).
//!
//! Design: `docs/feat-157-authoritative-vdb-database.md` (authoritative)
//! and `docs/vdb_to_db.md` §6–§11. Plan: `docs/02.305-vdb-backends.opus.md`.
//! The method list comes from the S0 inventory
//! (`docs/evidence/305-s0-path-inventory.md` §4, needs N1–N16 in §5) and
//! the write order from `docs/evidence/305-s0-vartree-write-order.md`.
//!
//! - [`InstalledDb`] is the read side plus [`InstalledDb::begin_write`];
//!   [`WriteTxn`] is the write side.
//! - [`register`] / [`for_root`] is the process-wide registry
//!   (`root → Arc<dyn InstalledDb>`). An unregistered root gets a cached
//!   [`FilesDb`], so `emerge` (which never registers) stays on the
//!   historic `var/db/pkg` layout.
//! - [`FilesDb`] is that layout. In S1.1 it is a stub: every method that
//!   does I/O returns [`Error::Unsupported`]. S1.2–S1.5 move today's code
//!   behind it.
//!
//! This crate sits **below** `portage-repo` and depends on nothing. Atom
//! parsing, `USE` reduction, version splitting (`split_pf`) and the
//! package-move remapping (`installed_cp_sources`) stay above it.
//!
//! # Changes from the feat#157 §6.1 first cut
//!
//! Every deviation from `vdb_to_db.md` §6.1, with its reason. Later steps
//! build on this list.
//!
//! 1. **`Cpv` is [`EntryKey`] `{ category, pf }`**, the stored directory
//!    name, not a split `cat/pkg-ver`. Splitting needs `ververify`
//!    (`portage-versions`/`portage-repo`), and several sites only hold the
//!    `pf` (`owns_path_pf`, `find_owners`). The key is always the stored
//!    (pre-`move`) name; the move fallback stays in `portage-repo` and
//!    calls the trait with each candidate name (N13).
//! 2. **`begin_entry(image) -> EntryId` is split into steps that keep
//!    today's write order** (S0.1 rows 3–16):
//!    [`WriteTxn::begin_entry`] creates the empty pending entry (it runs
//!    before `pkg_preinst`: the `l32` C4 killed-mid-merge invariant, #183);
//!    [`WriteTxn::copy_entry_file`] / [`WriteTxn::put_entry_file`] add
//!    files (N3); [`WriteTxn::next_counter`] ticks the counter (N1);
//!    [`WriteTxn::seal_entry`] writes the consolidated `metadata` file,
//!    stamp last; [`WriteTxn::finish_entry`] publishes. N1 asked for
//!    `begin_entry` to return the counter, but the counter is ticked
//!    after the build-info copy (`populate_vdb_tmp`), which is after
//!    `pkg_preinst`, so one call cannot do both without moving a write.
//!    `next_counter` returns the value before any payload lands, which
//!    is what N1 needs.
//! 3. **The pending entry is named by its [`EntryKey`], not an
//!    `EntryId`.** It outlives a transaction: the database backends
//!    commit the `merging` state first and finish in a later transaction
//!    (design §9), and `files` has at most one `-MERGING-<pf>` per key.
//! 4. **Replace order is delete, then publish** (§0.7, `vartree.py:
//!    5219-5238`): the merge calls [`WriteTxn::delete_entry`] for the old
//!    instance before [`WriteTxn::finish_entry`] for the new one.
//! 5. **`snapshot()` is not the hot read path.** A whole snapshot on
//!    `files` would open about 23 files per entry for the 1,500 of 2,130
//!    host entries that have no `metadata` file. The resolver reads
//!    through [`InstalledDb::entries`] (R3), [`InstalledDb::category_entries`]
//!    (R1), [`InstalledDb::aux_get`] (R2, lazy, one field) and
//!    [`InstalledDb::has_entry`] (N13), which keep today's `openat` and
//!    `statx` counts (N8b). [`InstalledDb::snapshot`] stays for the
//!    generation-keyed caches of the database backends (S3.2).
//! 6. **Two cache keys** (N8a): [`InstalledDb::generation`] (files: the
//!    `vdb_fingerprint` of `all_installed_packages`) and
//!    [`InstalledDb::category_generation`] (files: one category
//!    directory's mtime, the `installed_candidates` key, 1–2 `statx` per
//!    call instead of about 30).
//! 7. **Extra reads:** [`InstalledDb::read_pending_file`] (N2; the caller
//!    composes "live, then pending"), [`InstalledDb::file_meta`] (the
//!    exists-tests of `unmerge_one_installed` and `entries_not_merged`
//!    are one `stat`, not a read), [`InstalledDb::read_file_all`] (N11;
//!    it returns every live entry, with `None` where the file is
//!    missing, so `read_all_needed_entries` keeps its empty rows without
//!    a second walk).
//! 8. **`reverse_dependents` returns raw records** (§0.7, N15 option b):
//!    [`DepRecord`] holds the entry's normalised `USE` and the requested
//!    `*DEPEND` strings. The caller (`portage-repo`) reduces and matches
//!    them. The result is a superset of the real dependents: `files`
//!    returns every entry, an indexed backend only the entries whose
//!    reduced atoms name `cp`. The classes are a parameter because
//!    today's scan reads four keys, not five (no `IDEPEND`).
//! 9. **`world()` / `set_world` are split** into `world` + `world_sets`
//!    (N5): each file is read and written alone, and a transaction that
//!    only sets one of them is legal (N4).
//! 10. **[`PreservedLibs`] carries `loaded`** (N6): `set_preserved_libs`
//!     writes nothing when `entries == loaded`.
//! 11. **Counter store access for converters:** [`InstalledDb::counter`]
//!     and [`WriteTxn::set_counter`]. [`Counter`] is `i64`: a missing or
//!     corrupt `counter` file reads as `-1` today, so the first merge
//!     gets `0` (`next_counter`). S1 keeps portuale's rule (file only, no
//!     max over entry `COUNTER`s, no lock); §0.7 files that as a residue.
//! 12. **Whole-entry copy for converters:** [`InstalledDb::entry_image`]
//!     and [`WriteTxn::insert_entry`] with [`EntryImage`], which carries
//!     exact bytes, modes, mtimes and the `metadata` stamp state
//!     ([`MetadataStamp`]). A stale stamp stays stale and no `metadata`
//!     file is ever added (S0.3 corpus).
//! 13. **Paths for the `files` backend:** [`InstalledDb::vdb_dir`] and
//!     [`InstalledDb::entry_path`] (N9: `PORTAGE_UPDATE_ENV`, saved-env
//!     phases; N10: `emerge -C /var/db/pkg/cat/pf`). Both are `None` on
//!     the database backends, and callers then use a scratch copy plus
//!     [`WriteTxn::replace_file`].
//! 14. **[`FileMeta`] keeps the §6.1 meaning** (one stored file: name,
//!     size, mode, mtime) for `list_files`, convert and FUSE. N16's
//!     `CONTENTS`-line record is not a trait type (see "Rejected").
//! 15. **[`aux_get`](InstalledDb::aux_get) serves only the 23
//!     [`METADATA_FILE_FIELDS`]**, normalised like real `_aux_get`
//!     (`" ".join(v.split())`, invalid `SLOT` → `"0"`). Any other key
//!     (`CONTENTS`, `NEEDED.*`) is [`InstalledDb::read_file`], raw bytes.
//!     `None` means "no such entry", so the move fallback above can tell
//!     it from an empty field.
//! 16. **Errors** are one hand-written [`Error`] enum (`#[non_exhaustive]`,
//!     no `thiserror`, which is only a transitive dependency of the
//!     workspace). [`Error::Io`] displays as `"<path>: <io error>"`, the
//!     format every caller prints today.
//! 17. **One copy of the `metadata` format.** [`METADATA_FILE_FIELDS`],
//!     [`METADATA_FILE_FORMAT_VERSION`] and [`in_metadata_file`] are
//!     defined here. `portage-repo` still has its own copy until S1.2,
//!     which must replace it with a re-export (the field set is part of
//!     the format and two copies must not drift).
//!
//! **Rejected from N1–N16:** N10 `cpv_for_path` (the caller prints the
//! canonical VDB directory in its errors, so it needs
//! [`InstalledDb::vdb_dir`], not a key); N16's `CONTENTS`-shaped
//! `FileMeta` (today's `CONTENTS` readers differ: token rules, the six
//! kinds of `installed_contents_files`, whole-file UTF-8 failure in
//! `read_contents_pf`, so a shared parsed type would change one of them;
//! they keep parsing [`InstalledDb::read_file`] bytes, and the indexed
//! backends parse `CONTENTS` internally for [`InstalledDb::owners`]);
//! N8b's `entry(cpv)` returning all 23 fields (replaced by per-field
//! `aux_get`, item 5). N1 and N2 are taken in a different shape (items 2
//! and 7). N4, N5, N6, N7, N9, N11, N12, N13, N14 and N15 are taken
//! (N14 is internal to `FilesDb::delete_entry` / `finish_entry`).
//!
//! # Text and bytes
//!
//! File contents, `CONTENTS` paths ([`InstalledDb::owners`]) and
//! [`EntryImage`] data are bytes (design §7.3). Entry keys, file names
//! inside an entry, `aux_get` values, world atoms, preserved-libs records
//! and config memory are `String`, because today's readers decode them
//! (lossy for `aux_get`, like real's `errors="replace"`; `read_to_string`
//! for the others). An entry file whose name is not UTF-8 is reported as
//! [`Error::Invalid`] by `list_files` / `entry_image`; none exists in the
//! S0.3 corpus.

mod error;
mod files;
mod registry;
mod types;

pub use error::{Error, Result};
pub use files::FilesDb;
pub use registry::{for_root, register, reset};
pub use types::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Real `_METADATA_FILE_FIELDS` (`vartree.py:78-104`): the 23 single-line
/// fields of the consolidated `metadata` file, sorted. `CONTENTS` and
/// `NEEDED*` are line-oriented and excluded. The set is part of the
/// format: change it only together with [`METADATA_FILE_FORMAT_VERSION`].
pub const METADATA_FILE_FIELDS: &[&str] = &[
    "BDEPEND",
    "BUILD_ID",
    "BUILD_TIME",
    "CHOST",
    "COUNTER",
    "DEFINED_PHASES",
    "DEPEND",
    "DESCRIPTION",
    "EAPI",
    "HOMEPAGE",
    "IDEPEND",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "PROVIDES",
    "RDEPEND",
    "REQUIRES",
    "RESTRICT",
    "SLOT",
    "USE",
    "repository",
];

// [`EntryFields`] stores exactly this many values.
const _: () = assert!(METADATA_FILE_FIELDS.len() == 23);

/// Real `_METADATA_FILE_FORMAT_VERSION` (`vartree.py:105`).
pub const METADATA_FILE_FORMAT_VERSION: u32 = 1;

/// Real `_in_metadata_file(fname)`: whether `name` is one of the
/// [`METADATA_FILE_FIELDS`].
pub fn in_metadata_file(name: &str) -> bool {
    METADATA_FILE_FIELDS.contains(&name)
}

/// The read side of one installed-package database. One instance serves
/// one root (or, for [`FilesDb::open_vdb_dir`], one VDB directory).
///
/// Entries in the `merging` state (`files`: `-MERGING-<pf>`) are never
/// returned by the listing and per-entry methods; only
/// [`InstalledDb::read_pending_file`] sees them. Listing order is the
/// backend's stable order (`files`: `portage_util::read_dir_entries`,
/// sorted by name, as today).
pub trait InstalledDb: Send + Sync {
    /// Which backend this is.
    fn kind(&self) -> BackendKind;

    /// `files` only: the VDB directory (`<root>/var/db/pkg`, not
    /// canonicalised). `None` on the database backends (N10, N12).
    fn vdb_dir(&self) -> Option<PathBuf>;

    /// `files` only: the directory of a live entry, for code that must
    /// hand a path to bash or a copier (N9). Pure path arithmetic, no
    /// I/O; the entry need not exist.
    fn entry_path(&self, key: &EntryKey) -> Option<PathBuf> {
        self.vdb_dir()
            .map(|vdb| vdb.join(&key.category).join(&key.pf))
    }

    /// Cache key for "anything in the database changed". Compare for
    /// equality only. `files`: the `vdb_fingerprint` (VDB dir mtime plus
    /// every category dir mtime, folded), `0` when the VDB is missing.
    /// Database backends: `meta.generation`, bumped by every commit.
    fn generation(&self) -> Result<u64>;

    /// Cheaper cache key for one category (N8a). `files`: that category
    /// directory's mtime in nanoseconds, `0` when missing (the
    /// `installed_candidates` key). Database backends may return
    /// [`InstalledDb::generation`].
    fn category_generation(&self, category: &str) -> Result<u64>;

    /// Every live entry (R3), in listing order.
    fn entries(&self) -> Result<Vec<EntryKey>>;

    /// The `pf` of every live entry in `category` (R1), in listing order;
    /// empty when the category does not exist. The caller applies its
    /// own `<package>-<version>` prefix rule.
    fn category_entries(&self, category: &str) -> Result<Vec<String>>;

    /// Whether `key` is a live entry (N13).
    fn has_entry(&self, key: &EntryKey) -> Result<bool>;

    /// One of the [`METADATA_FILE_FIELDS`] of a live entry, normalised
    /// like real `_aux_get`: whitespace runs collapsed to one space,
    /// invalid UTF-8 decoded lossy, a present but invalid `SLOT` served
    /// as `"0"`, a missing field as `""`. `Ok(None)` when the entry does
    /// not exist. A key outside the set is [`Error::Invalid`].
    fn aux_get(&self, key: &EntryKey, field: &str) -> Result<Option<String>>;

    /// Every live entry with all 23 normalised fields. Expensive on
    /// `files` (see the module doc, item 5).
    fn snapshot(&self) -> Result<Arc<Snapshot>>;

    /// The files of a live entry, `None` when the entry does not exist.
    fn list_files(&self, key: &EntryKey) -> Result<Option<Vec<FileMeta>>>;

    /// Size, mode and mtime of one file of a live entry, `None` when the
    /// entry or the file does not exist (`files`: one `stat`).
    fn file_meta(&self, key: &EntryKey, name: &str) -> Result<Option<FileMeta>>;

    /// The exact bytes of one file of a live entry (R5), `None` when the
    /// entry or the file does not exist.
    fn read_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>>;

    /// Up to `len` bytes of one file from offset `off` (FUSE). Short at
    /// end of file; `None` when the entry or the file does not exist.
    fn read_file_at(
        &self,
        key: &EntryKey,
        name: &str,
        off: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>>;

    /// One file of the **pending** entry for `key` (`files`:
    /// `-MERGING-<pf>`), `None` when there is no pending entry or no
    /// such file (N2). Callers that want "live, then pending" call
    /// [`InstalledDb::read_file`] first.
    fn read_pending_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>>;

    /// One named file of every live entry, in listing order (N11). Every
    /// live entry is listed; the value is `None` where the file is
    /// missing.
    fn read_file_all(&self, name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>>;

    /// A whole live entry for a converter: every file with its bytes,
    /// mode and mtime, the directory's mode and mtime, and the
    /// `metadata` stamp state. `None` when the entry does not exist.
    fn entry_image(&self, key: &EntryKey) -> Result<Option<EntryImage>>;

    /// Candidate reverse dependents of `cp` (R4): one [`DepRecord`] per
    /// entry with its `USE` and the `classes` asked for, in that order.
    /// A superset; the caller reduces and matches (module doc, item 8).
    fn reverse_dependents(&self, cp: &str, classes: &[DepClass]) -> Result<Vec<DepRecord>>;

    /// File owners (R6): every `(path, entry)` pair where a live entry's
    /// `CONTENTS` records one of `paths` (absolute, as `CONTENTS` writes
    /// them). Entries in listing order; within an entry, `CONTENTS`
    /// order.
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>>;

    /// The `world` file's atoms (N5). Missing store: empty.
    fn world(&self) -> Result<World>;

    /// The `world_sets` file's set names, without the `@` (N5). Missing
    /// store: empty.
    fn world_sets(&self) -> Result<WorldSets>;

    /// The preserved-libs registry, with `loaded == entries` (N6).
    /// Missing or unparsable store: empty.
    fn preserved_libs(&self) -> Result<PreservedLibs>;

    /// The config-protect memory (N7). Missing store: empty.
    fn config_memory(&self) -> Result<ConfigMemory>;

    /// The counter store (W5), `None` when missing or unparsable. Not
    /// ticked; [`WriteTxn::next_counter`] ticks.
    fn counter(&self) -> Result<Option<Counter>>;

    /// Start a write transaction. Database backends take the write lock
    /// here (sqlite `BEGIN IMMEDIATE`).
    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>>;
}

/// One write transaction.
///
/// **`files` applies every call at once, in call order, and `commit` does
/// nothing** (N4): nothing is atomic, a failed merge leaves what was
/// written, exactly as today. The database backends apply everything at
/// `commit`, and dropping the transaction without committing rolls it
/// back. Callers therefore make the calls in today's write order
/// (S0.1) and must not rely on rollback.
///
/// A transaction may hold any subset of calls; one with only
/// [`WriteTxn::set_world`] is legal (N4).
pub trait WriteTxn {
    /// Create the empty pending entry for `key` (`files`: remove a stale
    /// `-MERGING-<pf>`, then create it). Readers do not see it.
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Write one file of the pending entry for `key` (N3).
    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()>;

    /// Copy the regular file `src` into the pending entry for `key` as
    /// `name` (N3). `files` copies with `std::fs::copy`, as
    /// `populate_vdb_tmp` does (the mode comes with it).
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()>;

    /// Tick the counter store and return the new value (W5, N1). `files`:
    /// read the `counter` file (`-1` when missing or unparsable), add
    /// one, write it back, no lock. The caller writes the entry's
    /// `COUNTER` file with [`WriteTxn::put_entry_file`].
    fn next_counter(&mut self) -> Result<Counter>;

    /// Write the consolidated `metadata` file of the pending entry: body,
    /// then `stat` of the directory, then the `#dir_mtime=` stamp
    /// appended last (`vartree.py:188-229`). No-op when the entry holds
    /// none of the [`METADATA_FILE_FIELDS`]. No file may be added to the
    /// pending entry afterwards.
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Publish the pending entry for `key` (`files`: remove a live entry
    /// with the same `pf`, then rename `-MERGING-<pf>` into place).
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Insert a whole live entry from an image (converters, W1 in one
    /// call). Counters are kept as they are. `files`: writes the files,
    /// applies modes and mtimes, then writes the `metadata` stamp last as
    /// [`EntryImage::metadata_stamp`] says (S2.6).
    fn insert_entry(&mut self, image: &EntryImage) -> Result<()>;

    /// Delete a live entry (W2). `files`: remove the directory, then the
    /// category directory if it is now empty, best effort (N14).
    fn delete_entry(&mut self, key: &EntryKey) -> Result<()>;

    /// Rewrite one file of a live entry (W4: `CONTENTS`, `NEEDED.ELF.2`,
    /// `environment.bz2` after `pkg_postinst`). `files`: in place, so the
    /// entry directory's mtime and its `metadata` stamp stay valid.
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()>;

    /// Replace the `world` store (N5). `files`: `create_dir_all` of the
    /// parent, then the atoms one per line, each followed by `\n`; an
    /// empty list writes an empty file.
    fn set_world(&mut self, world: &World) -> Result<()>;

    /// Replace the `world_sets` store (N5), same rules as
    /// [`WriteTxn::set_world`]; `files` writes each name as `@<name>`.
    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()>;

    /// Replace the preserved-libs registry. Writes nothing when
    /// `libs.entries == libs.loaded` (N6).
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()>;

    /// Replace the config-protect memory (N7), unconditionally.
    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()>;

    /// Set the counter store (converters; a merge uses
    /// [`WriteTxn::next_counter`]).
    fn set_counter(&mut self, counter: Counter) -> Result<()>;

    /// Make the transaction durable. A no-op on `files`.
    fn commit(self: Box<Self>) -> Result<()>;
}
