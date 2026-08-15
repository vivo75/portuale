//! The installed-package database ("VDB") behind one interface.
//!
//! - [`InstalledDb`] is the read side plus [`InstalledDb::begin_write`];
//!   [`WriteTxn`] is the write side.
//! - [`register`] / [`for_root`] is the process-wide registry
//!   (`root → Arc<dyn InstalledDb>`). An unregistered root gets a cached
//!   [`FilesDb`], so `emerge` (which never registers) stays on the
//!   historic `var/db/pkg` layout.
//! - [`FilesDb`] is that layout; the database backends (`SqliteDb`,
//!   `RedbDb`, behind the `vdb-sqlite` / `vdb-redb` features) implement
//!   every method.
//!
//! This crate sits **below** `portage-repo` and depends only on
//! `portage-util`: atom parsing, `USE` reduction, version splitting and
//! the package-move remapping stay above it.
//!
//! Every deviation from the design (`LLM/feat-157-authoritative-vdb-database.md`,
//! `docs/vdb_to_db.md`), the per-backend read/write semantics, and the
//! step history live in `LLM/code-notes/portage-vdb/lib.md`.
//!
//! File contents, `CONTENTS` paths and [`EntryImage`] data are bytes; entry
//! keys, file names, `aux_get` values, world atoms, preserved-libs records
//! and config memory are `String` (decoded, lossy for `aux_get`).

mod error;
mod files;
mod files_write;
#[cfg(feature = "vdb-redb")]
mod redb_db;
#[cfg(feature = "vdb-sqlite")]
mod sqlite;
mod types;
pub use error::{Error, Result};
pub use files_write::{format_preserved_libs, parse_preserved_libs};
pub use types::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The 23 single-line fields of the consolidated `metadata` file, sorted.
/// `CONTENTS` and `NEEDED*` are line-oriented and excluded. The set is part
/// of the format: change it only with [`METADATA_FILE_FORMAT_VERSION`].
// Portage: lib/portage/vartree.py, _METADATA_FILE_FIELDS
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

/// Portage's `_METADATA_FILE_FORMAT_VERSION`.
pub const METADATA_FILE_FORMAT_VERSION: u32 = 1;

/// Whether `name` is one of the [`METADATA_FILE_FIELDS`].
pub fn in_metadata_file(name: &str) -> bool {
    METADATA_FILE_FIELDS.contains(&name)
}
