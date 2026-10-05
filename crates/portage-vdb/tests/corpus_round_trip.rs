//! Corpus round trip: every corpus member is copied files -> sqlite ->
//! files, files -> redb -> files and, with both, files -> sqlite -> redb
//! -> files, and `verify` must find nothing on every leg.
//!
//! Members: the two pmtest fixture VDBs (always, read only through the
//! `fixtures` symlink), plus the host `/` when `PORTUALE_VDB_HOST_CORPUS=1`
//! (read only; the copies go to a scratch directory).
#![cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use portage_vdb::{FilesDb, InstalledDb, copy_all, verify};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "portage-vdb-corpus-{}-{}",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
