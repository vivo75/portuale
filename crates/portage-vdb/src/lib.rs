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

#[cfg(feature = "vdb-redb")]
mod redb_db;
mod types;
pub use types::*;

use std::path::{Path, PathBuf};
use std::sync::Arc;
