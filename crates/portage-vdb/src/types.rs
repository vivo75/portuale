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
    /// One SQLite file.
    Sqlite,
    /// One redb file.
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

/// State of the `#dir_mtime=` stamp of an entry's consolidated
/// `metadata` file.
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

/// Mask that turns a directory's `st_mode` into [`EntryImage::dir_mode`].
pub(crate) const DIR_MODE_MASK: u32 = 0o7777;

/// The 23 normalised [`METADATA_FILE_FIELDS`] of one entry, as
/// [`InstalledDb::aux_get`](crate::InstalledDb::aux_get) serves them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryFields {
    values: [String; 23],
}

/// Every live entry with its fields, taken at one
/// [`generation`](crate::InstalledDb::generation).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub entries: Vec<(EntryKey, Arc<EntryFields>)>,
    index: HashMap<EntryKey, usize>,
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

/// Raw dependency data of one entry for reverse-dependency matching
/// (the caller reduces `USE` conditionals). Values are normalised
/// like [`EntryFields`].
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
/// [`PreservedLibs`] (Portage's `PreservedLibsRegistry` `_data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservedLibsEntry {
    pub cpv: String,
    /// The owner's `COUNTER`, as the string Portage stores.
    pub counter: String,
    pub paths: Vec<String>,
}

/// The preserved-libs registry. `loaded` is the state as read (before any
/// pruning), `entries` the state to write; `set_preserved_libs` skips the
/// write when they are equal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreservedLibs {
    pub entries: BTreeMap<String, PreservedLibsEntry>,
    pub loaded: BTreeMap<String, PreservedLibsEntry>,
}

/// The config-protect memory: path → md5 of the last offered source.
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
