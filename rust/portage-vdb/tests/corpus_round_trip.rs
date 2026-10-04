//! Round trip of the #305 corpus (docs/evidence/305-s0-corpus.md, plan
//! S2.8): every corpus member is copied files -> sqlite -> files and
//! `verify` must find nothing, on both legs.
//!
//! Members: the two pmtest fixture VDBs (always, read only through the
//! `fixtures` symlink), plus the host `/` when `PORTUALE_VDB_HOST_CORPUS=1`
//! (read only; the copies go to a scratch directory).
#![cfg(feature = "vdb-sqlite")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use portage_vdb::{FilesDb, InstalledDb, SqliteDb, copy_all, verify};

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

/// files(`src`) -> sqlite -> files, verifying both legs.
fn round_trip(name: &str, src: Arc<dyn InstalledDb>) {
    let dir = scratch(name);
    let sqlite: Arc<dyn InstalledDb> = Arc::new(SqliteDb::open(dir.join("vdb.sqlite")).unwrap());
    copy_all(src.as_ref(), sqlite.as_ref(), false).unwrap();
    let back: Arc<dyn InstalledDb> = Arc::new(FilesDb::new(&dir.join("root")));
    copy_all(sqlite.as_ref(), back.as_ref(), false).unwrap();

    let first = verify(src.as_ref(), sqlite.as_ref()).unwrap();
    assert!(
        first.is_equal(),
        "{name}: files vs sqlite: {:#?}",
        first.differences
    );
    let second = verify(src.as_ref(), back.as_ref()).unwrap();
    assert!(
        second.is_equal(),
        "{name}: files vs files copy: {:#?}",
        second.differences
    );
    assert!(
        !src.entries().unwrap().is_empty(),
        "{name}: corpus member has no entries"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

#[test]
fn fixture_vdb_round_trips() {
    round_trip("fixture", Arc::new(FilesDb::new(&fixtures())));
}

#[test]
fn quickpkgroot_fixture_vdb_round_trips() {
    round_trip(
        "quickpkgroot",
        Arc::new(FilesDb::new(&fixtures().join("quickpkgroot"))),
    );
}

#[test]
fn host_vdb_round_trips_when_enabled() {
    if std::env::var_os("PORTUALE_VDB_HOST_CORPUS").as_deref() != Some("1".as_ref()) {
        eprintln!("skipped: set PORTUALE_VDB_HOST_CORPUS=1 to include the host VDB");
        return;
    }
    round_trip("host", Arc::new(FilesDb::new(Path::new("/"))));
}
