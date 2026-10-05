//! Round trip of the #305 corpus (docs/evidence/305-s0-corpus.md, plan
//! S2.8, S5.3): every corpus member is copied files -> sqlite -> files
//! (feature `vdb-sqlite`), files -> redb -> files (`vdb-redb`) and, with
//! both, files -> sqlite -> redb -> files, and `verify` must find nothing
//! on every leg.
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

/// `verify(src, copy)` finds nothing.
fn assert_equal(name: &str, what: &str, src: &dyn InstalledDb, copy: &dyn InstalledDb) {
    let r = verify(src, copy).unwrap();
    assert!(r.is_equal(), "{name}: {what}: {:#?}", r.differences);
}

/// Copy `from` into the fresh database `to`, then back into a fresh files
/// tree; the files copy must equal `src`. `to` is also compared with
/// `src` directly.
fn via(name: &str, tag: &str, src: &dyn InstalledDb, from: &dyn InstalledDb, to: &dyn InstalledDb) {
    copy_all(from, to, false).unwrap();
    assert_equal(name, &format!("files vs {tag}"), src, to);
    let dir = scratch(&format!("{name}-{tag}-back"));
    let back = FilesDb::new(&dir);
    copy_all(to, &back, false).unwrap();
    assert_equal(name, &format!("files vs files copy via {tag}"), src, &back);
    let _ = std::fs::remove_dir_all(&dir);
}

/// files(`src`) -> each backend -> files, verifying every leg.
fn round_trip(name: &str, src: Arc<dyn InstalledDb>) {
    assert!(
        !src.entries().unwrap().is_empty(),
        "{name}: corpus member has no entries"
    );
    let dir = scratch(name);
    #[cfg(feature = "vdb-sqlite")]
    let sqlite = portage_vdb::SqliteDb::open(dir.join("vdb.sqlite")).unwrap();
    #[cfg(feature = "vdb-sqlite")]
    via(name, "sqlite", src.as_ref(), src.as_ref(), &sqlite);
    #[cfg(feature = "vdb-redb")]
    {
        let redb = portage_vdb::RedbDb::open(dir.join("vdb.redb")).unwrap();
        via(name, "redb", src.as_ref(), src.as_ref(), &redb);
    }
    #[cfg(all(feature = "vdb-sqlite", feature = "vdb-redb"))]
    {
        let redb = portage_vdb::RedbDb::open(dir.join("from-sqlite.redb")).unwrap();
        via(name, "sqlite-redb", src.as_ref(), &sqlite, &redb);
    }
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
