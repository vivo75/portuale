//! Value types of the interface.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use crate::{Error, METADATA_FILE_FIELDS};

/// Which backend an [`InstalledDb`](crate::InstalledDb) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// The historic `var/db/pkg` directory tree.
    Files,
    /// One SQLite file (feature `vdb-sqlite`, S2).
    Sqlite,
    /// One redb file (feature `vdb-redb`, S5).
    Redb,
}

impl BackendKind {
    /// The spelling `--vdb-backend=` and `PORTUALE_VDB_BACKEND` use.
    pub fn as_str(self) -> &'static str {
        match self {
            BackendKind::Files => "files",
            BackendKind::Sqlite => "sqlite",
            BackendKind::Redb => "redb",
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BackendKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        match s {
            "files" => Ok(BackendKind::Files),
            "sqlite" => Ok(BackendKind::Sqlite),
            "redb" => Ok(BackendKind::Redb),
            other => Err(Error::Invalid(format!(
                "unknown VDB backend {other:?} (expected files, sqlite or redb)"
            ))),
        }
    }
}

/// One entry, by its stored name: `<category>/<pf>` (the directory name
/// on `files`). Never a `-MERGING-` name; the pending entry is named by
/// the key it will be published under.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryKey {
    pub category: String,
    /// `<package>-<version>`, e.g. `bash-5.2_p37-r1`.
    pub pf: String,
}

impl EntryKey {
    pub fn new(category: impl Into<String>, pf: impl Into<String>) -> Self {
        EntryKey {
            category: category.into(),
            pf: pf.into(),
        }
    }
}

impl fmt::Display for EntryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.category, self.pf)
    }
}

/// Size, mode and mtime of one stored file of an entry (§6.1 meaning;
/// not a `CONTENTS` line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    /// File name inside the entry (`CONTENTS`, `environment.bz2`, ...).
    pub name: String,
    pub len: u64,
    /// `st_mode` permission and type bits as stored.
    pub mode: u32,
    /// `st_mtime` in nanoseconds since the epoch (signed, like
    /// `st_mtime_ns`).
    pub mtime_ns: i128,
}

/// One file of an [`EntryImage`]: its metadata and exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFile {
    pub meta: FileMeta,
    pub data: Vec<u8>,
}

/// State of the `#dir_mtime=` stamp of an entry's consolidated
/// `metadata` file (S0.3 corpus: a stale stamp must stay stale, and a
/// converter never adds a `metadata` file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataStamp {
    /// The entry has no `metadata` file.
    Absent,
    /// The stamp equals the entry directory's `st_mtime_ns`; a writer
    /// stamps the new directory's own value.
    Valid,
    /// The stamp does not match (or is missing); a writer keeps the
    /// stored bytes, so it still does not match.
    Stale,
}

/// An entry without its bytes ([`InstalledDb::entry_stat`]): what a
/// read-only view (FUSE, S7) needs for `getattr` and `readdir`. The same
/// facts as [`EntryImage`] minus the file contents.
///
/// [`InstalledDb::entry_stat`]: crate::InstalledDb::entry_stat
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryStat {
    /// Every file, sorted by name.
    pub files: Vec<FileMeta>,
    /// The entry directory's mode.
    pub dir_mode: u32,
    /// The entry directory's mtime in nanoseconds.
    pub dir_mtime_ns: i128,
    /// The `metadata` stamp state.
    pub metadata_stamp: MetadataStamp,
}

impl From<EntryImage> for EntryStat {
    fn from(image: EntryImage) -> Self {
        EntryStat {
            files: image.files.into_iter().map(|f| f.meta).collect(),
            dir_mode: image.dir_mode,
            dir_mtime_ns: image.dir_mtime_ns,
            metadata_stamp: image.metadata_stamp,
        }
    }
}

/// A whole entry, for converters ([`InstalledDb::entry_image`],
/// [`WriteTxn::insert_entry`]).
///
/// [`InstalledDb::entry_image`]: crate::InstalledDb::entry_image
/// [`WriteTxn::insert_entry`]: crate::WriteTxn::insert_entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryImage {
    pub key: EntryKey,
    /// Every file, `metadata` included when present, in name order.
    pub files: Vec<EntryFile>,
    /// The entry directory's mode.
    pub dir_mode: u32,
    /// The entry directory's mtime in nanoseconds.
    pub dir_mtime_ns: i128,
    pub metadata_stamp: MetadataStamp,
}

/// The 23 normalised [`METADATA_FILE_FIELDS`] of one entry, as
/// [`InstalledDb::aux_get`](crate::InstalledDb::aux_get) serves them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryFields {
    values: [String; 23],
}

impl EntryFields {
    /// Fields from `(name, value)` pairs; names outside the set are
    /// ignored, missing ones are `""`. Values are stored as given (the
    /// backend normalises).
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, String)>) -> Self {
        let mut out = EntryFields::default();
        for (name, value) in pairs {
            if let Some(i) = field_index(name) {
                out.values[i] = value;
            }
        }
        out
    }

    /// One field; `None` when `name` is not one of the 23.
    pub fn get(&self, name: &str) -> Option<&str> {
        field_index(name).map(|i| self.values[i].as_str())
    }

    /// `(name, value)` in [`METADATA_FILE_FIELDS`] order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &str)> {
        METADATA_FILE_FIELDS
            .iter()
            .copied()
            .zip(self.values.iter().map(String::as_str))
    }
}

fn field_index(name: &str) -> Option<usize> {
    METADATA_FILE_FIELDS.iter().position(|f| *f == name)
}

/// Every live entry with its fields, taken at one
/// [`generation`](crate::InstalledDb::generation).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub entries: Vec<(EntryKey, Arc<EntryFields>)>,
    index: HashMap<EntryKey, usize>,
}

impl Snapshot {
    pub fn new(generation: u64, entries: Vec<(EntryKey, Arc<EntryFields>)>) -> Self {
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (k.clone(), i))
            .collect();
        Snapshot {
            generation,
            entries,
            index,
        }
    }

    /// The fields of one entry.
    pub fn get(&self, key: &EntryKey) -> Option<&Arc<EntryFields>> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }
}

/// A dependency class stored per entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DepClass {
    Depend,
    Rdepend,
    Bdepend,
    Pdepend,
    Idepend,
}

impl DepClass {
    /// The field name (`DEPEND`, ...).
    pub fn field(self) -> &'static str {
        match self {
            DepClass::Depend => "DEPEND",
            DepClass::Rdepend => "RDEPEND",
            DepClass::Bdepend => "BDEPEND",
            DepClass::Pdepend => "PDEPEND",
            DepClass::Idepend => "IDEPEND",
        }
    }
}

/// Raw dependency data of one entry for reverse-dependency matching
/// (§0.7: `portage-vdb` cannot reduce `USE` conditionals; the caller
/// does). Values are normalised like [`EntryFields`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRecord {
    pub key: EntryKey,
    /// The entry's `USE`.
    pub use_flags: String,
    /// The requested classes, in the order asked for.
    pub deps: Vec<(DepClass, String)>,
}

/// What [`WriteTxn::rebuild_index`](crate::WriteTxn::rebuild_index)
/// rebuilt: the live entries it read and the rows it wrote per table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IndexCounts {
    pub entries: usize,
    pub owner: usize,
    pub dep_atom: usize,
    pub needed: usize,
}

/// The `world` store: selected atoms, one per line on `files`. Reading
/// drops blank lines, `#` comments and `@` lines (`read_world_atoms`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct World {
    pub atoms: Vec<String>,
}

/// The `world_sets` store: selected set names **without** the leading
/// `@` (`read_world_sets`); `files` stores them as `@<name>` lines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorldSets {
    pub sets: Vec<String>,
}

/// One preserved-libs registry record, keyed by `cp:slot` in
/// [`PreservedLibs`] (real `PreservedLibsRegistry` `_data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservedLibsEntry {
    pub cpv: String,
    /// The owner's `COUNTER`, as the string real stores.
    pub counter: String,
    pub paths: Vec<String>,
}

/// The preserved-libs registry. `loaded` is the state as read (before any
/// pruning), `entries` the state to write; `set_preserved_libs` skips the
/// write when they are equal (N6, real `store()`'s `_data == _data_orig`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreservedLibs {
    pub entries: BTreeMap<String, PreservedLibsEntry>,
    pub loaded: BTreeMap<String, PreservedLibsEntry>,
}

/// The config-protect memory (`var/lib/portage/config` on `files`):
/// path → md5 of the last offered source (N7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigMemory {
    pub entries: BTreeMap<String, String>,
}

/// A merge counter value (`COUNTER`, the `counter` store). Signed: a
/// missing or corrupt store reads as `-1` in `next_counter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Counter(pub i64);

impl fmt::Display for Counter {
    /// The bytes of a `COUNTER` file / the `counter` store (no newline).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_round_trips_its_spelling() {
        for kind in [BackendKind::Files, BackendKind::Sqlite, BackendKind::Redb] {
            assert_eq!(kind.as_str().parse::<BackendKind>().unwrap(), kind);
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert!(matches!(
            "lmdb".parse::<BackendKind>(),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn entry_fields_cover_the_23_fields_only() {
        let f = EntryFields::from_pairs([
            ("SLOT", "0/1.2".to_string()),
            ("repository", "gentoo".to_string()),
            ("CONTENTS", "obj /x".to_string()),
        ]);
        assert_eq!(f.get("SLOT"), Some("0/1.2"));
        assert_eq!(f.get("repository"), Some("gentoo"));
        assert_eq!(f.get("USE"), Some(""));
        assert_eq!(f.get("CONTENTS"), None);
        let names: Vec<_> = f.iter().map(|(n, _)| n).collect();
        assert_eq!(names, METADATA_FILE_FIELDS);
    }

    #[test]
    fn snapshot_indexes_its_entries() {
        let key = EntryKey::new("sys-libs", "glibc-2.41");
        let fields = Arc::new(EntryFields::from_pairs([("SLOT", "2.2".to_string())]));
        let snap = Snapshot::new(7, vec![(key.clone(), fields)]);
        assert_eq!(snap.get(&key).and_then(|f| f.get("SLOT")), Some("2.2"));
        assert!(snap.get(&EntryKey::new("sys-libs", "musl-1.2")).is_none());
        assert_eq!(key.to_string(), "sys-libs/glibc-2.41");
        assert_eq!(Counter(-1).to_string(), "-1");
    }
}
