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

// ------------------------------------------------------------ S8 indexes

const CLASSES: [portage_vdb::DepClass; 5] = [
    portage_vdb::DepClass::Depend,
    portage_vdb::DepClass::Rdepend,
    portage_vdb::DepClass::Bdepend,
    portage_vdb::DepClass::Pdepend,
    portage_vdb::DepClass::Idepend,
];

/// The `category/package` strings a raw dependency token can stand for,
/// read naively: blocker stripped; without a version operator the token
/// (cut at `[` and `:`) itself, with one every prefix that ends before a
/// `-<digit>`.
fn naive_cps(tok: &str) -> Vec<String> {
    let t = tok.trim_start_matches(['!']);
    let has_op = t.starts_with(['<', '>', '=', '~']);
    let t = t.trim_start_matches(['<', '>', '=', '~']);
    let t = t.split(['[', ':']).next().unwrap_or(t);
    if t.matches('/').count() != 1 {
        return Vec::new();
    }
    let mut out = Vec::new();
    if !has_op {
        out.push(t.to_string());
    }
    for (i, _) in t.match_indices('-') {
        if has_op && t[i + 1..].starts_with(|c: char| c.is_ascii_digit()) {
            out.push(t[..i].to_string());
        }
    }
    out
}

/// S8.1 and S8.2 on the corpus: for every `cp` any entry's deps mention,
/// `reverse_dependents` of each database backend is a superset of the
/// entries whose raw deps (read from `files`) have a token for that `cp`,
/// and `owners` equals the `files` answer for every `CONTENTS` path (all
/// at once, and each alone).
fn index_queries_match_the_scan(name: &str, src: &FilesDb, dbs: &[(&str, &dyn InstalledDb)]) {
    use std::collections::{BTreeMap, BTreeSet};
    let mut mentions: BTreeMap<String, BTreeSet<portage_vdb::EntryKey>> = BTreeMap::new();
    for class in CLASSES {
        for (k, data) in src.read_file_all(class.field()).unwrap() {
            let text = String::from_utf8_lossy(&data.unwrap_or_default()).into_owned();
            for tok in text.split_whitespace() {
                for cp in naive_cps(tok) {
                    mentions.entry(cp).or_default().insert(k.clone());
                }
            }
        }
    }
    assert!(!mentions.is_empty(), "{name}: the corpus has no deps");
    let mut paths: Vec<Vec<u8>> = Vec::new();
    for (_, data) in src.read_file_all("CONTENTS").unwrap() {
        for line in String::from_utf8_lossy(&data.unwrap_or_default()).lines() {
            let mut w = line.split_whitespace();
            if let (Some("obj" | "sym" | "dir" | "dev" | "fif" | "bin"), Some(p)) =
                (w.next(), w.next())
            {
                paths.push(p.as_bytes().to_vec());
            }
        }
    }
    paths.sort();
    paths.dedup();
    let refs: Vec<&[u8]> = paths.iter().map(Vec::as_slice).collect();
    let want_all = src.owners(&refs).unwrap();
    for (tag, db) in dbs {
        for (cp, want) in &mentions {
            let got: BTreeSet<_> = db
                .reverse_dependents(cp, &CLASSES)
                .unwrap()
                .into_iter()
                .map(|r| r.key)
                .collect();
            assert!(
                want.is_subset(&got),
                "{name}/{tag}: {cp}: missing {:?}",
                want.difference(&got).collect::<Vec<_>>()
            );
        }
        assert_eq!(db.owners(&refs).unwrap(), want_all, "{name}/{tag}: owners");
        // With a leading `/` dropped, and each path alone (a sample on a
        // large corpus).
        let stripped: Vec<&[u8]> = paths
            .iter()
            .map(|p| p.strip_prefix(b"/").unwrap_or(p))
            .collect();
        assert_eq!(
            db.owners(&stripped).unwrap().len(),
            src.owners(&stripped).unwrap().len(),
            "{name}/{tag}: owners without the leading slash"
        );
        for p in refs.iter().step_by((refs.len() / 300).max(1)) {
            assert_eq!(
                db.owners(&[*p]).unwrap(),
                src.owners(&[*p]).unwrap(),
                "{name}/{tag}: owners of {}",
                String::from_utf8_lossy(p)
            );
        }
    }
}

fn index_queries(name: &str, src: Arc<FilesDb>) {
    let dir = scratch(&format!("{name}-index"));
    #[cfg(feature = "vdb-sqlite")]
    let sqlite = portage_vdb::SqliteDb::open(dir.join("vdb.sqlite")).unwrap();
    #[cfg(feature = "vdb-sqlite")]
    copy_all(src.as_ref(), &sqlite, false).unwrap();
    #[cfg(feature = "vdb-redb")]
    let redb = portage_vdb::RedbDb::open(dir.join("vdb.redb")).unwrap();
    #[cfg(feature = "vdb-redb")]
    copy_all(src.as_ref(), &redb, false).unwrap();
    let dbs: Vec<(&str, &dyn InstalledDb)> = vec![
        #[cfg(feature = "vdb-sqlite")]
        ("sqlite", &sqlite),
        #[cfg(feature = "vdb-redb")]
        ("redb", &redb),
    ];
    index_queries_match_the_scan(name, &src, &dbs);
    drop(dbs);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fixture_index_queries_match_the_scan() {
    index_queries("fixture", Arc::new(FilesDb::new(&fixtures())));
}

#[test]
fn quickpkgroot_index_queries_match_the_scan() {
    index_queries(
        "quickpkgroot",
        Arc::new(FilesDb::new(&fixtures().join("quickpkgroot"))),
    );
}

#[test]
fn host_index_queries_match_the_scan_when_enabled() {
    if std::env::var_os("PORTUALE_VDB_HOST_CORPUS").as_deref() != Some("1".as_ref()) {
        eprintln!("skipped: set PORTUALE_VDB_HOST_CORPUS=1 to include the host VDB");
        return;
    }
    index_queries("host", Arc::new(FilesDb::new(Path::new("/"))));
}
