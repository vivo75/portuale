//! Backend conformance suite for `portage-vdb` (feat#157 / #305, plan S2.2).
//!
//! Every test body is generic over the backend: it gets a factory
//! (`fn(&Path) -> Arc<dyn InstalledDb>`, one fresh database per call) and a
//! [`Caps`] table, builds its data through the backend's **own**
//! [`WriteTxn`] (`begin_entry`, `put_entry_file`, `seal_entry`,
//! `finish_entry`, ...), and reads it back through [`InstalledDb`]. Nothing
//! here touches a backend's storage directly, except in the clearly
//! separated `files_only` module.
//!
//! [`conformance_suite!`] instantiates every generic test for one backend.
//! The later steps add one line each:
//!
//! ```ignore
//! #[cfg(feature = "vdb-sqlite")]
//! conformance_suite!(sqlite, |root| Arc::new(SqliteDb::open(root)...), Caps { .. });
//! ```
//!
//! What is deliberately NOT asserted generically (because only `files` has
//! it; see `files_only`): directory layout (an emptied category directory
//! is removed), exact `generation` values, the first counter value being
//! `0`, on-disk bytes of the D4 stores, `-MERGING-` directory names.
//!
//! Each test owns a private temporary root (std only, removed on drop).

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use portage_vdb::{
    ConfigMemory, Counter, DepClass, EntryFile, EntryImage, EntryKey, EntryStat, Error, FileMeta,
    FilesDb, IndexCounts, InstalledDb, MetadataStamp, PreservedLibs, PreservedLibsEntry, Result,
    World, WorldSets,
};

/// One database per call, rooted at (or stored under) the given directory.
type Factory = fn(&Path) -> Arc<dyn InstalledDb>;

/// What a backend does not implement yet. `Some(step)`: the method must
/// fail with [`Error::Unsupported`] and the message must contain `step`
/// (the plan step that adds it). `None`: the method is implemented and the
/// generic test of its behaviour runs.
struct Caps {
    /// `WriteTxn::set_counter`.
    set_counter: Option<&'static str>,
    /// `InstalledDb::reverse_dependents`.
    reverse_dependents: Option<&'static str>,
    /// `InstalledDb::snapshot`.
    snapshot: Option<&'static str>,
    /// `InstalledDb::read_file_at`.
    read_file_at: Option<&'static str>,
    /// `InstalledDb::entry_image` and `WriteTxn::insert_entry`.
    entry_image: Option<&'static str>,
    /// `WriteTxn::rebuild_index` (only a backend with a derived index).
    rebuild_index: Option<&'static str>,
    /// `seal_entry` stores a readable consolidated `metadata` file (files:
    /// yes; a database backend may keep the fields only in its tables).
    seal_stores_metadata_file: bool,
}

// ---------------------------------------------------------------- helpers

/// A private scratch root plus the database opened on it. Removed on drop
/// (also when the test panics).
struct Ctx {
    root: PathBuf,
    factory: Factory,
    db: Arc<dyn InstalledDb>,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

impl Ctx {
    fn new(factory: Factory, tag: &str) -> Ctx {
        let root = std::env::temp_dir().join(format!(
            "portage-vdb-conf-{}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let db = factory(&root);
        Ctx { root, factory, db }
    }

    /// A second handle on the same storage (persistence checks).
    fn reopen(&self) -> Arc<dyn InstalledDb> {
        (self.factory)(&self.root)
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn key(category: &str, pf: &str) -> EntryKey {
    EntryKey::new(category, pf)
}

/// Directory mtimes (the `files` generation) are filesystem timestamps;
/// sleep so that two writes never share one.
fn settle() {
    std::thread::sleep(Duration::from_millis(25));
}

/// Publish one entry through the backend's own write path, in the merge's
/// order, in one transaction.
fn put(ctx: &Ctx, k: &EntryKey, files: &[(&str, &[u8])], seal: bool) {
    let mut txn = ctx.db.begin_write().unwrap();
    txn.begin_entry(k).unwrap();
    for (name, data) in files {
        txn.put_entry_file(k, name, data).unwrap();
    }
    if seal {
        txn.seal_entry(k).unwrap();
    }
    txn.finish_entry(k).unwrap();
    txn.commit().unwrap();
}

fn read(ctx: &Ctx, k: &EntryKey, name: &str) -> Option<Vec<u8>> {
    ctx.db.read_file(k, name).unwrap()
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v
}

fn aux(ctx: &Ctx, k: &EntryKey, field: &str) -> Option<String> {
    ctx.db.aux_get(k, field).unwrap()
}

fn assert_unsupported<T: Debug>(r: Result<T>, step: &str, what: &str) {
    match r {
        Err(Error::Unsupported(msg)) => assert!(
            msg.contains(step),
            "{what}: Unsupported message must name {step}: {msg}"
        ),
        other => panic!("{what}: expected Unsupported({step}), got {other:?}"),
    }
}

/// Bytes that are not UTF-8.
const BAD_UTF8: &[u8] = b"\xff\xfe\x00\x80 caf\xe9\n";

// ------------------------------------------------------------ the suite

mod suite {
    use super::*;

    pub fn empty_database_has_no_entries_stores_or_counter(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "empty");
        let db = &ctx.db;
        assert!(db.entries().unwrap().is_empty());
        assert!(db.categories().unwrap().is_empty());
        assert!(db.category_entries("dev-libs").unwrap().is_empty());
        assert!(!db.has_entry(&key("dev-libs", "a-1")).unwrap());
        assert_eq!(db.aux_get(&key("dev-libs", "a-1"), "SLOT").unwrap(), None);
        assert_eq!(db.read_file(&key("dev-libs", "a-1"), "SLOT").unwrap(), None);
        assert!(db.list_files(&key("dev-libs", "a-1")).unwrap().is_none());
        assert!(db.read_file_all("CONTENTS").unwrap().is_empty());
        assert!(db.owners(&[b"/usr/bin/x".as_slice()]).unwrap().is_empty());
        assert_eq!(db.world().unwrap(), World::default());
        assert_eq!(db.world_sets().unwrap(), WorldSets::default());
        assert_eq!(db.preserved_libs().unwrap(), PreservedLibs::default());
        assert_eq!(db.config_memory().unwrap(), ConfigMemory::default());
        assert_eq!(db.counter().unwrap(), None);
        // The generation is a stable cache key while nothing is written.
        assert_eq!(db.generation().unwrap(), db.generation().unwrap());
        assert_eq!(
            db.category_generation("dev-libs").unwrap(),
            db.category_generation("dev-libs").unwrap()
        );
    }

    pub fn insert_entry_reads_back_exact_bytes_including_empty_and_invalid_utf8(
        f: Factory,
        _c: &Caps,
    ) {
        let ctx = Ctx::new(f, "bytes");
        let k = key("dev-libs", "a-1.0");
        let contents: &[u8] = b"dir /usr\nobj /usr/bin/a d41d8cd98f00b204e9800998ecf8427e 1\n";
        put(
            &ctx,
            &k,
            &[
                ("CONTENTS", contents),
                ("USE", b""),
                ("IUSE", b""),
                ("environment.bz2", BAD_UTF8),
                ("SLOT", b"0\n"),
                ("NUL", b"a\0b\0"),
            ],
            false,
        );
        assert!(ctx.db.has_entry(&k).unwrap());
        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(contents));
        assert_eq!(read(&ctx, &k, "USE").as_deref(), Some(&b""[..]));
        assert_eq!(read(&ctx, &k, "IUSE").as_deref(), Some(&b""[..]));
        assert_eq!(read(&ctx, &k, "environment.bz2").as_deref(), Some(BAD_UTF8));
        assert_eq!(read(&ctx, &k, "SLOT").as_deref(), Some(&b"0\n"[..]));
        assert_eq!(read(&ctx, &k, "NUL").as_deref(), Some(&b"a\0b\0"[..]));
        // Visible through every listing.
        assert_eq!(ctx.db.entries().unwrap(), vec![k.clone()]);
        assert_eq!(ctx.db.categories().unwrap(), vec!["dev-libs".to_string()]);
        assert_eq!(
            ctx.db.category_entries("dev-libs").unwrap(),
            vec!["a-1.0".to_string()]
        );
        assert!(ctx.db.category_entries("app-misc").unwrap().is_empty());
        // Persisted, not cached in the handle.
        let again = ctx.reopen();
        assert_eq!(
            again.read_file(&k, "environment.bz2").unwrap().as_deref(),
            Some(BAD_UTF8)
        );
        assert_eq!(again.entries().unwrap(), vec![k]);
    }

    pub fn list_files_and_file_meta_describe_stored_files(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "meta");
        let k = key("dev-libs", "a-1.0");
        put(
            &ctx,
            &k,
            &[
                ("CONTENTS", b"12345"),
                ("EMPTY", b""),
                ("RDEPEND", b"x/y\n"),
            ],
            false,
        );
        let files = ctx.db.list_files(&k).unwrap().expect("entry exists");
        let names: Vec<&str> = files.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(sorted(names), ["CONTENTS", "EMPTY", "RDEPEND"]);
        let by_name: BTreeMap<&str, &FileMeta> =
            files.iter().map(|m| (m.name.as_str(), m)).collect();
        assert_eq!(by_name["CONTENTS"].len, 5);
        assert_eq!(by_name["EMPTY"].len, 0);
        assert_eq!(by_name["RDEPEND"].len, 4);
        for m in &files {
            assert!(
                m.mode & 0o400 != 0,
                "{}: owner-readable, mode {:o}",
                m.name,
                m.mode
            );
            assert!(m.mtime_ns > 0, "{}: mtime set", m.name);
            // file_meta agrees with list_files.
            assert_eq!(ctx.db.file_meta(&k, &m.name).unwrap().as_ref(), Some(m));
        }
        assert_eq!(ctx.db.file_meta(&k, "NOPE").unwrap(), None);
        assert_eq!(
            ctx.db
                .file_meta(&key("dev-libs", "zz-1"), "CONTENTS")
                .unwrap(),
            None
        );
        assert!(
            ctx.db
                .list_files(&key("dev-libs", "zz-1"))
                .unwrap()
                .is_none()
        );
    }

    pub fn missing_entry_or_file_reads_as_none(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "missing");
        let k = key("dev-libs", "a-1.0");
        put(&ctx, &k, &[("CONTENTS", b"")], false);
        assert_eq!(ctx.db.read_file(&k, "NOPE").unwrap(), None);
        assert_eq!(
            ctx.db
                .read_file(&key("dev-libs", "b-1.0"), "CONTENTS")
                .unwrap(),
            None
        );
        assert_eq!(
            ctx.db
                .read_file(&key("no-cat", "b-1.0"), "CONTENTS")
                .unwrap(),
            None
        );
        assert_eq!(ctx.db.read_pending_file(&k, "CONTENTS").unwrap(), None);
        assert!(!ctx.db.has_entry(&key("dev-libs", "b-1.0")).unwrap());
        assert!(!ctx.db.has_entry(&key("no-cat", "a-1.0")).unwrap());
    }

    pub fn aux_get_normalises_whitespace_and_invalid_slot(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "aux");
        let k = key("dev-libs", "a-1.0");
        put(
            &ctx,
            &k,
            &[
                ("RDEPEND", b"  a\n\tb   c \n\n"),
                ("DESCRIPTION", b"line one\r\nline  two\n"),
                ("SLOT", b"!bad\n"),
                ("EAPI", b"8\n"),
                ("USE", b""),
                ("KEYWORDS", b"\n"),
            ],
            false,
        );
        assert_eq!(aux(&ctx, &k, "RDEPEND").as_deref(), Some("a b c"));
        assert_eq!(
            aux(&ctx, &k, "DESCRIPTION").as_deref(),
            Some("line one line two")
        );
        assert_eq!(aux(&ctx, &k, "EAPI").as_deref(), Some("8"));
        // Present but invalid SLOT is served as "0".
        assert_eq!(aux(&ctx, &k, "SLOT").as_deref(), Some("0"));
        // Present but empty / whitespace-only is "".
        assert_eq!(aux(&ctx, &k, "USE").as_deref(), Some(""));
        assert_eq!(aux(&ctx, &k, "KEYWORDS").as_deref(), Some(""));
        // The stored bytes are untouched (aux_get normalises on read only).
        assert_eq!(
            read(&ctx, &k, "RDEPEND").as_deref(),
            Some(&b"  a\n\tb   c \n\n"[..])
        );
    }

    pub fn aux_get_serves_valid_slots_unchanged(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "slot");
        for (i, (raw, want)) in [
            (&b"0\n"[..], "0"),
            (b"2.2/2.2.1\n", "2.2/2.2.1"),
            (b"_x+y.z-w\n", "_x+y.z-w"),
            (b"-bad\n", "0"),
            (b"a/-b\n", "0"),
            (b"a b\n", "0"),
        ]
        .into_iter()
        .enumerate()
        {
            let k = key("dev-libs", &format!("s{i}-1"));
            put(&ctx, &k, &[("SLOT", raw)], false);
            assert_eq!(aux(&ctx, &k, "SLOT").as_deref(), Some(want), "SLOT {raw:?}");
        }
    }

    pub fn aux_get_missing_field_is_empty_and_missing_entry_is_none(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "auxmiss");
        let k = key("dev-libs", "a-1.0");
        put(&ctx, &k, &[("SLOT", b"1\n")], false);
        for &field in portage_vdb::METADATA_FILE_FIELDS {
            let v = aux(&ctx, &k, field).unwrap_or_else(|| panic!("{field}: entry exists"));
            if field == "SLOT" {
                assert_eq!(v, "1");
            } else {
                assert_eq!(v, "", "{field}: missing field is empty");
            }
            assert_eq!(
                ctx.db.aux_get(&key("dev-libs", "zz-1"), field).unwrap(),
                None,
                "{field}: missing entry"
            );
        }
        assert_eq!(portage_vdb::METADATA_FILE_FIELDS.len(), 23);
    }

    pub fn aux_get_rejects_keys_outside_the_23_fields(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "auxkey");
        let k = key("dev-libs", "a-1.0");
        put(
            &ctx,
            &k,
            &[("CONTENTS", b"obj /x\n"), ("SLOT", b"0\n")],
            false,
        );
        for bad in ["CONTENTS", "NEEDED.ELF.2", "metadata", "slot", "", "SLOT "] {
            assert!(
                matches!(ctx.db.aux_get(&k, bad), Err(Error::Invalid(_))),
                "aux_get({bad:?}) must be Invalid"
            );
        }
        // Even for a missing entry the key is validated first or the
        // answer is None; both are acceptable, an Ok(Some) is not.
        assert!(!matches!(
            ctx.db.aux_get(&key("dev-libs", "zz-1"), "CONTENTS"),
            Ok(Some(_))
        ));
    }

    pub fn aux_get_is_the_same_with_and_without_seal(f: Factory, c: &Caps) {
        let ctx = Ctx::new(f, "seal");
        let files: &[(&str, &[u8])] = &[
            ("COUNTER", b"7"),
            ("EAPI", b"8\n"),
            ("RDEPEND", b" a\n  b\n"),
            ("SLOT", b"3/4\n"),
            ("USE", b"x  y\n"),
            ("repository", b"gentoo\n"),
            ("CONTENTS", b"obj /x abc 1\n"),
        ];
        let plain = key("dev-libs", "plain-1");
        let sealed = key("dev-libs", "sealed-1");
        put(&ctx, &plain, files, false);
        put(&ctx, &sealed, files, true);
        for &field in portage_vdb::METADATA_FILE_FIELDS {
            assert_eq!(
                aux(&ctx, &plain, field),
                aux(&ctx, &sealed, field),
                "{field}: sealed and unsealed entries answer alike"
            );
        }
        assert_eq!(aux(&ctx, &sealed, "RDEPEND").as_deref(), Some("a b"));
        assert_eq!(aux(&ctx, &sealed, "SLOT").as_deref(), Some("3/4"));
        assert_eq!(aux(&ctx, &sealed, "USE").as_deref(), Some("x y"));
        assert_eq!(aux(&ctx, &sealed, "IUSE").as_deref(), Some(""));
        // Every payload file survives the seal untouched.
        for (name, data) in files {
            assert_eq!(read(&ctx, &sealed, name).as_deref(), Some(*data), "{name}");
        }
        let has_metadata = read(&ctx, &sealed, "metadata").is_some();
        assert_eq!(
            has_metadata, c.seal_stores_metadata_file,
            "sealed entry's metadata file"
        );
        // An unsealed entry never gains one.
        assert_eq!(read(&ctx, &plain, "metadata"), None);
    }

    pub fn seal_of_an_entry_without_metadata_fields_adds_no_metadata_file(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "sealnone");
        let k = key("dev-libs", "a-1");
        put(
            &ctx,
            &k,
            &[("CONTENTS", b""), ("NEEDED.ELF.2", b"x\n")],
            true,
        );
        assert_eq!(read(&ctx, &k, "metadata"), None);
        assert_eq!(aux(&ctx, &k, "SLOT").as_deref(), Some(""));
        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(&b""[..]));
    }

    pub fn pending_entry_is_invisible_until_finish(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "pending");
        let k = key("dev-libs", "a-1.0");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "CONTENTS", b"obj /x\n").unwrap();
        txn.put_entry_file(&k, "EMPTY", b"").unwrap();
        txn.commit().unwrap();

        // The pending entry survives the transaction (module doc, item 3).
        assert!(!ctx.db.has_entry(&k).unwrap());
        assert!(ctx.db.entries().unwrap().is_empty());
        assert!(ctx.db.category_entries("dev-libs").unwrap().is_empty());
        assert_eq!(ctx.db.aux_get(&k, "SLOT").unwrap(), None);
        assert_eq!(ctx.db.read_file(&k, "CONTENTS").unwrap(), None);
        assert!(ctx.db.list_files(&k).unwrap().is_none());
        assert!(ctx.db.read_file_all("CONTENTS").unwrap().is_empty());
        assert!(ctx.db.owners(&[b"/x".as_slice()]).unwrap().is_empty());
        // ... but read_pending_file sees its files, empty ones included.
        assert_eq!(
            ctx.db.read_pending_file(&k, "CONTENTS").unwrap().as_deref(),
            Some(&b"obj /x\n"[..])
        );
        assert_eq!(
            ctx.db.read_pending_file(&k, "EMPTY").unwrap().as_deref(),
            Some(&b""[..])
        );
        assert_eq!(ctx.db.read_pending_file(&k, "NOPE").unwrap(), None);
        // Another handle on the same storage agrees (it is stored, not
        // held in the first handle's memory).
        assert!(!ctx.reopen().has_entry(&k).unwrap());
        assert!(
            ctx.reopen()
                .read_pending_file(&k, "CONTENTS")
                .unwrap()
                .is_some()
        );

        let mut txn = ctx.db.begin_write().unwrap();
        txn.finish_entry(&k).unwrap();
        txn.commit().unwrap();
        assert!(ctx.db.has_entry(&k).unwrap());
        assert_eq!(ctx.db.entries().unwrap(), vec![k.clone()]);
        assert_eq!(
            read(&ctx, &k, "CONTENTS").as_deref(),
            Some(&b"obj /x\n"[..])
        );
        assert_eq!(read(&ctx, &k, "EMPTY").as_deref(), Some(&b""[..]));
        // No pending copy is left behind.
        assert_eq!(ctx.db.read_pending_file(&k, "CONTENTS").unwrap(), None);
    }

    pub fn begin_entry_discards_a_stale_pending_entry(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "stale");
        let k = key("dev-libs", "a-1.0");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "STALE", b"x").unwrap();
        txn.commit().unwrap();
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "FRESH", b"y").unwrap();
        txn.finish_entry(&k).unwrap();
        txn.commit().unwrap();
        assert_eq!(read(&ctx, &k, "STALE"), None);
        assert_eq!(read(&ctx, &k, "FRESH").as_deref(), Some(&b"y"[..]));
    }

    pub fn pending_replacement_leaves_the_live_entry_until_finish(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "pendlive");
        let k = key("dev-libs", "a-1.0");
        put(&ctx, &k, &[("CONTENTS", b"old\n"), ("OLD", b"1")], false);
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "CONTENTS", b"new\n").unwrap();
        txn.commit().unwrap();
        // Live copy untouched, pending copy readable.
        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(&b"old\n"[..]));
        assert_eq!(read(&ctx, &k, "OLD").as_deref(), Some(&b"1"[..]));
        assert_eq!(
            ctx.db.read_pending_file(&k, "CONTENTS").unwrap().as_deref(),
            Some(&b"new\n"[..])
        );
        assert_eq!(ctx.db.entries().unwrap(), vec![k.clone()]);
        // Same-pf finish replaces the live entry as a whole.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.finish_entry(&k).unwrap();
        txn.commit().unwrap();
        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(&b"new\n"[..]));
        assert_eq!(
            read(&ctx, &k, "OLD"),
            None,
            "files of the old instance are gone"
        );
        assert_eq!(ctx.db.entries().unwrap(), vec![k]);
    }

    pub fn discard_pending_removes_an_orphan_and_never_an_installed_entry(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "discard");
        let k = key("dev-libs", "a-1.0");
        let orphan = key("dev-libs", "b-1.0");
        put(&ctx, &k, &[("CONTENTS", b"old\n")], false);
        // A merge killed before finish_entry: the entry stays pending.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&orphan).unwrap();
        txn.put_entry_file(&orphan, "CONTENTS", b"half\n").unwrap();
        // The same key as the installed one, pending as well (a replace).
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "CONTENTS", b"new\n").unwrap();
        txn.commit().unwrap();
        assert_eq!(
            sorted(ctx.db.pending_entries().unwrap()),
            vec![k.clone(), orphan.clone()]
        );

        let mut txn = ctx.db.begin_write().unwrap();
        txn.discard_pending(&orphan).unwrap();
        // Not pending any more: refused, and nothing else is touched.
        assert!(matches!(
            txn.discard_pending(&orphan),
            Err(portage_vdb::Error::Invalid(_))
        ));
        txn.commit().unwrap();
        assert_eq!(ctx.db.pending_entries().unwrap(), vec![k.clone()]);
        assert_eq!(ctx.db.read_pending_file(&orphan, "CONTENTS").unwrap(), None);

        // Discarding the pending twin of an installed key keeps the install.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.discard_pending(&k).unwrap();
        txn.commit().unwrap();
        assert!(ctx.db.pending_entries().unwrap().is_empty());
        assert_eq!(ctx.db.entries().unwrap(), vec![k.clone()]);
        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(&b"old\n"[..]));

        // An installed entry with nothing pending is not discardable.
        let mut txn = ctx.db.begin_write().unwrap();
        assert!(matches!(
            txn.discard_pending(&k),
            Err(portage_vdb::Error::Invalid(_))
        ));
        drop(txn);
        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(&b"old\n"[..]));
    }

    pub fn replace_in_slot_leaves_only_the_new_entry(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "slot-replace");
        let old = key("dev-libs", "a-1.0");
        let new = key("dev-libs", "a-2.0");
        let other = key("dev-libs", "b-1.0");
        put(
            &ctx,
            &old,
            &[("SLOT", b"0\n"), ("CONTENTS", b"old\n")],
            false,
        );
        put(&ctx, &other, &[("SLOT", b"0\n")], false);
        let before = ctx.db.generation().unwrap();
        settle();

        // The merge's order (module doc, item 4): pending entry, delete the
        // old instance, publish the new one.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&new).unwrap();
        txn.put_entry_file(&new, "SLOT", b"0\n").unwrap();
        txn.put_entry_file(&new, "CONTENTS", b"new\n").unwrap();
        txn.delete_entry(&old).unwrap();
        txn.finish_entry(&new).unwrap();
        txn.commit().unwrap();

        assert!(!ctx.db.has_entry(&old).unwrap());
        assert!(ctx.db.has_entry(&new).unwrap());
        assert_eq!(read(&ctx, &old, "CONTENTS"), None);
        assert_eq!(read(&ctx, &new, "CONTENTS").as_deref(), Some(&b"new\n"[..]));
        assert_eq!(sorted(ctx.db.entries().unwrap()), vec![new, other]);
        assert_ne!(ctx.db.generation().unwrap(), before, "generation changed");
    }

    pub fn delete_entry_removes_only_that_entry(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "delete");
        let a = key("dev-libs", "a-1");
        let b = key("dev-libs", "b-1");
        let c = key("app-misc", "c-1");
        put(&ctx, &a, &[("CONTENTS", b"x")], false);
        put(&ctx, &b, &[("CONTENTS", b"y")], false);
        put(&ctx, &c, &[("CONTENTS", b"z")], false);

        let mut txn = ctx.db.begin_write().unwrap();
        txn.delete_entry(&a).unwrap();
        txn.commit().unwrap();
        assert!(!ctx.db.has_entry(&a).unwrap());
        assert_eq!(read(&ctx, &a, "CONTENTS"), None);
        assert!(ctx.db.list_files(&a).unwrap().is_none());
        assert_eq!(ctx.db.aux_get(&a, "SLOT").unwrap(), None);
        assert_eq!(
            ctx.db.category_entries("dev-libs").unwrap(),
            vec!["b-1".to_string()]
        );
        // A sibling keeps its category.
        assert_eq!(
            sorted(ctx.db.categories().unwrap()),
            ["app-misc", "dev-libs"]
        );
        assert_eq!(read(&ctx, &b, "CONTENTS").as_deref(), Some(&b"y"[..]));

        // Portable part of "an emptied category goes away": no entries are
        // left in it. Whether the category itself is still listed is
        // backend-specific (`files` removes the directory, see
        // `files_only::delete_removes_an_emptied_category_directory`).
        let mut txn = ctx.db.begin_write().unwrap();
        txn.delete_entry(&b).unwrap();
        txn.commit().unwrap();
        assert!(ctx.db.category_entries("dev-libs").unwrap().is_empty());
        assert_eq!(ctx.db.entries().unwrap(), vec![c]);
        // Persisted.
        assert_eq!(ctx.reopen().entries().unwrap().len(), 1);
    }

    pub fn replace_file_rewrites_one_file_and_leaves_the_others(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "w4");
        let k = key("dev-libs", "a-1.0");
        let other = key("dev-libs", "b-1.0");
        put(
            &ctx,
            &k,
            &[
                ("CONTENTS", b"old old old\n"),
                ("NEEDED.ELF.2", b"n\n"),
                ("environment.bz2", BAD_UTF8),
                ("EMPTY", b""),
            ],
            false,
        );
        put(&ctx, &other, &[("CONTENTS", b"same\n")], false);
        let names_before: Vec<String> = ctx
            .db
            .list_files(&k)
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();

        let mut txn = ctx.db.begin_write().unwrap();
        txn.replace_file(&k, "CONTENTS", b"new\n").unwrap();
        txn.replace_file(&k, "environment.bz2", b"").unwrap();
        txn.commit().unwrap();

        assert_eq!(read(&ctx, &k, "CONTENTS").as_deref(), Some(&b"new\n"[..]));
        assert_eq!(read(&ctx, &k, "environment.bz2").as_deref(), Some(&b""[..]));
        assert_eq!(read(&ctx, &k, "NEEDED.ELF.2").as_deref(), Some(&b"n\n"[..]));
        assert_eq!(read(&ctx, &k, "EMPTY").as_deref(), Some(&b""[..]));
        assert_eq!(
            read(&ctx, &other, "CONTENTS").as_deref(),
            Some(&b"same\n"[..])
        );
        let names_after: Vec<String> = ctx
            .db
            .list_files(&k)
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names_before, names_after, "no file added or dropped");
        assert_eq!(ctx.db.file_meta(&k, "CONTENTS").unwrap().unwrap().len, 4);
        assert_eq!(ctx.db.entries().unwrap().len(), 2);
        // Persisted.
        assert_eq!(
            ctx.reopen().read_file(&k, "CONTENTS").unwrap().as_deref(),
            Some(&b"new\n"[..])
        );
    }

    pub fn counter_ticks_monotonically_and_persists(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "counter");
        assert_eq!(ctx.db.counter().unwrap(), None);
        let mut txn = ctx.db.begin_write().unwrap();
        let a = txn.next_counter().unwrap();
        let b = txn.next_counter().unwrap();
        txn.commit().unwrap();
        assert_eq!(b.0, a.0 + 1);
        assert!(a.0 >= 0, "the first merge gets a non-negative counter");
        // `counter` reads the store without ticking.
        assert_eq!(ctx.db.counter().unwrap(), Some(b));
        assert_eq!(ctx.db.counter().unwrap(), Some(b));
        // Survives a reopen; the next tick continues.
        let again = ctx.reopen();
        assert_eq!(again.counter().unwrap(), Some(b));
        let mut txn = again.begin_write().unwrap();
        let c = txn.next_counter().unwrap();
        txn.commit().unwrap();
        assert_eq!(c.0, b.0 + 1);
        assert!(Counter(c.0) > a);
    }

    pub fn set_counter_round_trips_or_names_its_step(f: Factory, caps: &Caps) {
        let ctx = Ctx::new(f, "setcounter");
        let mut txn = ctx.db.begin_write().unwrap();
        match caps.set_counter {
            // TODO(plan S2.6 / S2.7, `files` stub says "S2.6"): FilesDb::set_counter
            // is still Unsupported. When it is implemented, flip the
            // backend's `Caps::set_counter` to `None`; the branch below
            // then runs on files too.
            Some(step) => {
                assert_unsupported(txn.set_counter(Counter(41)), step, "set_counter");
            }
            None => {
                txn.set_counter(Counter(41)).unwrap();
                txn.commit().unwrap();
                assert_eq!(ctx.db.counter().unwrap(), Some(Counter(41)));
                assert_eq!(ctx.reopen().counter().unwrap(), Some(Counter(41)));
                let mut txn = ctx.db.begin_write().unwrap();
                assert_eq!(txn.next_counter().unwrap(), Counter(42));
                txn.set_counter(Counter(7)).unwrap();
                txn.commit().unwrap();
                // set_counter is a plain store: it may lower the counter.
                assert_eq!(ctx.db.counter().unwrap(), Some(Counter(7)));
            }
        }
    }

    pub fn world_and_world_sets_round_trip_independently(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "world");
        let atoms = World {
            atoms: vec![
                "dev-libs/a".into(),
                "dev-libs/b:2".into(),
                ">=sys-apps/c-1".into(),
            ],
        };
        // A transaction that holds only set_world is legal.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&atoms).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.world().unwrap(), atoms);
        assert_eq!(ctx.db.world_sets().unwrap(), WorldSets::default());
        assert!(ctx.db.entries().unwrap().is_empty(), "no entry appears");

        // And so is one that holds only set_world_sets.
        let sets = WorldSets {
            sets: vec!["selected".into(), "system".into()],
        };
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world_sets(&sets).unwrap();
        txn.commit().unwrap();
        assert_eq!(
            ctx.db.world_sets().unwrap(),
            sets,
            "names come back without @"
        );
        assert_eq!(
            ctx.db.world().unwrap(),
            atoms,
            "world untouched by world_sets"
        );

        // Both in one transaction (`--deselect` rewrites both).
        let atoms2 = World {
            atoms: vec!["dev-libs/z".into()],
        };
        let sets2 = WorldSets {
            sets: vec!["only".into()],
        };
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world_sets(&sets2).unwrap();
        txn.set_world(&atoms2).unwrap();
        txn.commit().unwrap();
        let again = ctx.reopen();
        assert_eq!(again.world().unwrap(), atoms2);
        assert_eq!(again.world_sets().unwrap(), sets2);
    }

    pub fn empty_world_lists_clear_the_stores(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "world-empty");
        // Writing empty lists to a database that never had them is legal.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&World::default()).unwrap();
        txn.set_world_sets(&WorldSets::default()).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.world().unwrap(), World::default());
        assert_eq!(ctx.db.world_sets().unwrap(), WorldSets::default());

        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&World {
            atoms: vec!["dev-libs/a".into()],
        })
        .unwrap();
        txn.set_world_sets(&WorldSets {
            sets: vec!["s".into()],
        })
        .unwrap();
        txn.commit().unwrap();
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&World::default()).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.world().unwrap(), World::default());
        assert_eq!(ctx.db.world_sets().unwrap().sets, vec!["s".to_string()]);
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world_sets(&WorldSets::default()).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.reopen().world_sets().unwrap(), WorldSets::default());
    }

    pub fn preserved_libs_round_trip_and_unchanged_write_is_skipped(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "plib");
        let mut libs = ctx.db.preserved_libs().unwrap();
        assert_eq!(libs, PreservedLibs::default());
        // entries == loaded: nothing is written (N6).
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_preserved_libs(&libs).unwrap();
        txn.commit().unwrap();
        assert!(ctx.db.preserved_libs().unwrap().entries.is_empty());

        libs.entries.insert(
            "dev-libs/a:0".to_string(),
            PreservedLibsEntry {
                cpv: "dev-libs/a-1.0".to_string(),
                counter: "3".to_string(),
                paths: vec![
                    "/usr/lib/liba.so.1".to_string(),
                    "/usr/lib/a\"b\\c.so".to_string(),
                    "/usr/lib/caf\u{e9}.so".to_string(),
                ],
            },
        );
        libs.entries.insert(
            "sys-libs/z:1/2".to_string(),
            PreservedLibsEntry {
                cpv: "sys-libs/z-2".to_string(),
                counter: "0".to_string(),
                paths: vec![],
            },
        );
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_preserved_libs(&libs).unwrap();
        txn.commit().unwrap();
        let back = ctx.db.preserved_libs().unwrap();
        assert_eq!(back.entries, libs.entries);
        assert_eq!(
            back.loaded, back.entries,
            "a fresh read has loaded == entries"
        );
        assert_eq!(ctx.reopen().preserved_libs().unwrap().entries, libs.entries);

        // Re-writing what was read changes nothing.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_preserved_libs(&back).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.preserved_libs().unwrap().entries, libs.entries);

        // Pruning everything (entries empty, loaded not) is written.
        let pruned = PreservedLibs {
            entries: BTreeMap::new(),
            loaded: back.loaded.clone(),
        };
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_preserved_libs(&pruned).unwrap();
        txn.commit().unwrap();
        assert!(ctx.db.preserved_libs().unwrap().entries.is_empty());
    }

    pub fn config_memory_round_trips_and_is_written_unconditionally(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "confmem");
        assert!(ctx.db.config_memory().unwrap().entries.is_empty());
        let mut mem = ConfigMemory::default();
        mem.entries
            .insert("/etc/a.conf".into(), "0123456789abcdef".into());
        mem.entries
            .insert("/etc/b/c.conf".into(), "fedcba9876543210".into());
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_config_memory(&mem).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.config_memory().unwrap(), mem);
        assert_eq!(ctx.reopen().config_memory().unwrap(), mem);
        // Unlike preserved libs there is no loaded/unchanged rule: a
        // replacement always replaces.
        let mut smaller = ConfigMemory::default();
        smaller
            .entries
            .insert("/etc/a.conf".into(), "11111111".into());
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_config_memory(&smaller).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.config_memory().unwrap(), smaller);
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_config_memory(&ConfigMemory::default()).unwrap();
        txn.commit().unwrap();
        assert!(ctx.db.config_memory().unwrap().entries.is_empty());
    }

    pub fn generation_changes_after_entry_commits_and_is_stable_across_reads(
        f: Factory,
        _c: &Caps,
    ) {
        let ctx = Ctx::new(f, "gen");
        let db = &ctx.db;
        let g0 = db.generation().unwrap();
        assert_eq!(g0, db.generation().unwrap());

        settle();
        let a = key("dev-libs", "a-1");
        put(&ctx, &a, &[("SLOT", b"0\n")], false);
        let g1 = db.generation().unwrap();
        assert_ne!(g1, g0, "insert changes the generation");
        // Reads do not move it.
        let _ = db.entries().unwrap();
        let _ = db.aux_get(&a, "SLOT").unwrap();
        let _ = db.read_file(&a, "SLOT").unwrap();
        let _ = db.list_files(&a).unwrap();
        let _ = db.read_file_all("SLOT").unwrap();
        let _ = db.owners(&[b"/x".as_slice()]).unwrap();
        assert_eq!(db.generation().unwrap(), g1);
        // A second handle on the same storage reports the same key.
        assert_eq!(ctx.reopen().generation().unwrap(), g1);

        settle();
        let b = key("dev-libs", "b-1");
        put(&ctx, &b, &[("SLOT", b"0\n")], false);
        let g2 = db.generation().unwrap();
        assert_ne!(g2, g1, "a second insert changes it again");

        settle();
        let mut txn = db.begin_write().unwrap();
        txn.delete_entry(&a).unwrap();
        txn.commit().unwrap();
        let g3 = db.generation().unwrap();
        assert_ne!(g3, g2, "delete changes it");
        assert_eq!(db.generation().unwrap(), g3);

        // Same-pf reinstall.
        settle();
        put(&ctx, &b, &[("SLOT", b"1\n")], false);
        assert_ne!(db.generation().unwrap(), g3, "reinstall changes it");

        // category_generation: stable across reads, moves with the
        // category's own entries.
        let c0 = db.category_generation("dev-libs").unwrap();
        assert_eq!(c0, db.category_generation("dev-libs").unwrap());
        settle();
        put(&ctx, &key("dev-libs", "c-1"), &[("SLOT", b"0\n")], false);
        assert_ne!(db.category_generation("dev-libs").unwrap(), c0);
    }

    pub fn owners_report_the_entries_that_claim_paths(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "owners");
        let a = key("dev-libs", "a-1");
        let b = key("dev-libs", "b-1");
        let none = key("app-misc", "none-1");
        put(
            &ctx,
            &a,
            &[(
                "CONTENTS",
                b"dir /usr\nobj /usr/bin/x abc 1\nsym /usr/bin/y -> x 1\nfoo /usr/z\n",
            )],
            false,
        );
        put(&ctx, &b, &[("CONTENTS", b"obj /usr/bin/x def 2\n")], false);
        put(&ctx, &none, &[("CONTENTS", b"")], false);
        // A pending entry owns nothing.
        let p = key("dev-libs", "pending-1");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&p).unwrap();
        txn.put_entry_file(&p, "CONTENTS", b"obj /usr/bin/x ghi 3\n")
            .unwrap();
        txn.commit().unwrap();

        let got = sorted(
            ctx.db
                .owners(&[
                    b"/usr/bin/x".as_slice(),
                    b"/usr/bin/y".as_slice(),
                    b"/usr".as_slice(),
                    b"/usr/z".as_slice(),
                    b"/nope".as_slice(),
                ])
                .unwrap(),
        );
        let want = sorted(vec![
            (b"/usr".to_vec(), a.clone()),
            (b"/usr/bin/x".to_vec(), a.clone()),
            (b"/usr/bin/y".to_vec(), a.clone()),
            (b"/usr/bin/x".to_vec(), b.clone()),
        ]);
        assert_eq!(
            got, want,
            "`foo` is not a recorded kind, so /usr/z and /nope are unowned"
        );
        assert!(ctx.db.owners(&[]).unwrap().is_empty());
    }

    pub fn read_file_all_lists_every_live_entry_with_none_for_a_missing_file(
        f: Factory,
        _c: &Caps,
    ) {
        let ctx = Ctx::new(f, "all");
        let a = key("dev-libs", "a-1");
        let b = key("dev-libs", "b-1");
        let c = key("app-misc", "c-1");
        put(&ctx, &a, &[("NEEDED.ELF.2", b"n\n")], false);
        put(&ctx, &b, &[("CONTENTS", b"x")], false);
        put(&ctx, &c, &[("NEEDED.ELF.2", b"")], false);
        let pend = key("dev-libs", "pending-1");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&pend).unwrap();
        txn.put_entry_file(&pend, "NEEDED.ELF.2", b"p\n").unwrap();
        txn.commit().unwrap();

        let all = sorted(ctx.db.read_file_all("NEEDED.ELF.2").unwrap());
        assert_eq!(
            all,
            sorted(vec![
                (a.clone(), Some(b"n\n".to_vec())),
                (b.clone(), None),
                (c.clone(), Some(Vec::new())),
            ]),
            "every live entry; None only where the file is missing; empty is Some"
        );
        assert_eq!(ctx.db.read_file_all("NOPE").unwrap().len(), 3);
        assert!(
            ctx.db
                .read_file_all("NOPE")
                .unwrap()
                .iter()
                .all(|(_, d)| d.is_none())
        );
    }

    pub fn categories_and_category_entries_list_live_entries(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "cats");
        for (c, pf) in [
            ("sys-apps", "z-1"),
            ("dev-libs", "b-2"),
            ("dev-libs", "a-1"),
            ("app-misc", "q-3"),
        ] {
            put(&ctx, &key(c, pf), &[("SLOT", b"0\n")], false);
        }
        assert_eq!(
            sorted(ctx.db.categories().unwrap()),
            ["app-misc", "dev-libs", "sys-apps"]
        );
        assert_eq!(
            sorted(ctx.db.category_entries("dev-libs").unwrap()),
            ["a-1", "b-2"]
        );
        assert_eq!(ctx.db.category_entries("sys-apps").unwrap(), ["z-1"]);
        assert!(ctx.db.category_entries("virtual").unwrap().is_empty());
        assert_eq!(
            sorted(ctx.db.entries().unwrap()),
            vec![
                key("app-misc", "q-3"),
                key("dev-libs", "a-1"),
                key("dev-libs", "b-2"),
                key("sys-apps", "z-1"),
            ]
        );
        // The listing order is stable between calls.
        assert_eq!(ctx.db.entries().unwrap(), ctx.db.entries().unwrap());
        assert_eq!(ctx.db.categories().unwrap(), ctx.db.categories().unwrap());
    }

    pub fn reverse_dependents_is_supported_or_names_its_step(f: Factory, caps: &Caps) {
        let ctx = Ctx::new(f, "revdeps");
        let k = key("dev-libs", "user-1");
        put(
            &ctx,
            &k,
            &[
                ("USE", b"ssl  x\n"),
                ("RDEPEND", b"dev-libs/lib\n"),
                ("DEPEND", b""),
            ],
            false,
        );
        let r = ctx
            .db
            .reverse_dependents("dev-libs/lib", &[DepClass::Rdepend, DepClass::Depend]);
        match caps.reverse_dependents {
            // Per module-doc item 19 FilesDb does not implement this (the
            // scan stays in portage-repo over aux_get). TODO(plan S8.1):
            // flip `Caps::reverse_dependents` to `None` for `files` if S8.1
            // implements it there; the branch below then runs.
            Some(step) => assert_unsupported(r, step, "reverse_dependents"),
            None => {
                let recs = r.unwrap();
                // A superset: the entry that names the cp must be there,
                // with USE and the classes in the order asked for.
                let rec = recs
                    .iter()
                    .find(|d| d.key == k)
                    .expect("the dependent entry is returned");
                assert_eq!(rec.use_flags, "ssl x");
                assert_eq!(
                    rec.deps,
                    vec![
                        (DepClass::Rdepend, "dev-libs/lib".to_string()),
                        (DepClass::Depend, String::new()),
                    ]
                );
            }
        }
    }

    /// S8.2: the edge cases of the path rule, on every backend (the
    /// database backends answer from the `owner` index): one leading `/`
    /// ignored on both sides, a path written without it, `//`, a repeated
    /// line, the first matching input path reported, and the order (entries
    /// by `(category, pf)`, then `CONTENTS` order).
    pub fn owners_follow_the_claim_paths_rule_on_edge_spellings(f: Factory, _c: &Caps) {
        let ctx = Ctx::new(f, "owners-edge");
        let a = key("dev-libs", "a-1");
        let b = key("app-misc", "b-1");
        put(
            &ctx,
            &a,
            &[(
                "CONTENTS",
                b"obj /usr/bin/x abc 1\ndir usr/share\nobj //weird d 2\n\
                  sym /usr/bin/x -> y 3\nobj /usr/bin/x abc 1\n\
                  dir /\nobj /tab\tbed 1\ndev /dev/null\nfif /f\nbin /b\n",
            )],
            false,
        );
        put(
            &ctx,
            &b,
            &[("CONTENTS", b"obj /usr/share d 1\r\nobj /zz z 1\n")],
            false,
        );
        // Not UTF-8: owns nothing, anywhere.
        let bad = key("dev-libs", "bad-1");
        put(
            &ctx,
            &bad,
            &[("CONTENTS", b"obj /usr/bin/x \xff 1\n")],
            false,
        );
        let q: &[&[u8]] = &[
            b"usr/bin/x",
            b"/usr/bin/x",
            b"/usr/share",
            b"/weird",
            b"//weird",
            b"/",
            b"",
            b"/dev/null",
            b"/f",
            b"/b",
            b"/zz",
            b"/nothing",
        ];
        let got = ctx.db.owners(q).unwrap();
        let x = b"usr/bin/x".to_vec();
        let want = vec![
            // app-misc/b-1 sorts first.
            (b"/usr/share".to_vec(), b.clone()),
            (b"/zz".to_vec(), b.clone()),
            // dev-libs/a-1, in CONTENTS order.
            (x.clone(), a.clone()),
            (b"/usr/share".to_vec(), a.clone()),
            (b"//weird".to_vec(), a.clone()),
            (x.clone(), a.clone()),
            (x.clone(), a.clone()),
            (b"/".to_vec(), a.clone()),
            (b"/dev/null".to_vec(), a.clone()),
            (b"/f".to_vec(), a.clone()),
            (b"/b".to_vec(), a.clone()),
        ];
        assert_eq!(got, want);
    }

    /// S8.1: `reverse_dependents` is a superset of the real dependents and
    /// an indexed backend still returns an entry whose deps it could not
    /// fully index (a `USE` conditional, an `||` group, a wildcard or other
    /// odd token), follows `replace_file` and `delete_entry`, and leaves out
    /// entries that cannot depend on the cp.
    pub fn reverse_dependents_index_is_a_superset_and_keeps_unsure_entries(f: Factory, c: &Caps) {
        if c.reverse_dependents.is_some() {
            return;
        }
        let ctx = Ctx::new(f, "revdeps-index");
        let mk = |n: &str, files: &[(&str, &[u8])]| {
            let k = key("app-misc", n);
            put(&ctx, &k, files, false);
            k
        };
        let cond = mk(
            "cond-1",
            &[
                ("USE", b"ssl\n"),
                ("RDEPEND", b"ssl? ( dev-libs/lib:= ) !ssl? ( dev-libs/o )\n"),
            ],
        );
        let alt = mk(
            "alt-1",
            &[(
                "RDEPEND",
                b"|| ( dev-libs/o >=dev-libs/lib-1.2[x,-y(+)] )\n",
            )],
        );
        let wild = mk("wild-1", &[("RDEPEND", b"dev-libs/* \n")]);
        let bare = mk("bare-1", &[("DEPEND", b"dev-libs/lib-1.2\n")]);
        let blk = mk("blk-1", &[("PDEPEND", b"!!<dev-libs/lib-3\n")]);
        let other = mk("other-1", &[("RDEPEND", b"dev-libs/o dev-libs/p:2\n")]);
        let none = mk("none-1", &[("USE", b"a\n"), ("SLOT", b"0\n")]);
        let all = [
            DepClass::Depend,
            DepClass::Rdepend,
            DepClass::Bdepend,
            DepClass::Pdepend,
            DepClass::Idepend,
        ];
        let keys = |cp: &str, cls: &[DepClass]| -> Vec<EntryKey> {
            ctx.db
                .reverse_dependents(cp, cls)
                .unwrap()
                .into_iter()
                .map(|r| r.key)
                .collect()
        };
        let got = keys("dev-libs/lib", &all);
        for (k, why) in [
            (&cond, "USE-conditional"),
            (&alt, "|| group with a versioned atom and use deps"),
            (&wild, "unindexable token"),
            (&bare, "bare versioned name (unsure)"),
            (&blk, "blocker"),
        ] {
            assert!(got.contains(k), "{why}: {k} missing from {got:?}");
        }
        assert!(
            !got.contains(&other),
            "an indexed entry that cannot match is skipped"
        );
        assert!(!got.contains(&none));
        // Entries come back in (category, pf) order, with the records the
        // caller reduces.
        assert_eq!(got, sorted(got.clone()));
        let rec = ctx
            .db
            .reverse_dependents("dev-libs/lib", &[DepClass::Rdepend, DepClass::Depend])
            .unwrap();
        let rc = rec.iter().find(|r| r.key == cond).unwrap();
        assert_eq!(rc.use_flags, "ssl");
        assert_eq!(
            rc.deps[0],
            (
                DepClass::Rdepend,
                "ssl? ( dev-libs/lib:= ) !ssl? ( dev-libs/o )".to_string()
            )
        );
        // The class filter: PDEPEND-only is not asked for.
        assert!(!keys("dev-libs/lib", &[DepClass::Rdepend]).contains(&blk));
        assert!(keys("dev-libs/lib", &[]).is_empty());
        // The index follows the files.
        assert!(!keys("dev-libs/p", &all).contains(&none));
        assert!(keys("dev-libs/p", &all).contains(&other));
        let mut txn = ctx.db.begin_write().unwrap();
        txn.replace_file(&none, "RDEPEND", b"dev-libs/p\n").unwrap();
        txn.replace_file(&other, "RDEPEND", b"dev-libs/q\n")
            .unwrap();
        txn.commit().unwrap();
        let after = keys("dev-libs/p", &all);
        assert!(after.contains(&none), "{after:?}");
        assert!(!after.contains(&other), "{after:?}");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.delete_entry(&none).unwrap();
        txn.commit().unwrap();
        assert!(!keys("dev-libs/p", &all).contains(&none));
        assert!(!keys("dev-libs/lib", &all).contains(&none));
    }

    /// S8.3: `rebuild_index` recomputes `owner`, `dep_atom` and `needed`
    /// from the stored files of the live entries only; the queries that
    /// read the index answer the same before and after, and a second
    /// rebuild writes the same rows. Repairing a damaged row is tested per
    /// backend (`sqlite.rs`, `redb_db.rs`), which can reach the rows.
    pub fn rebuild_index_recomputes_the_same_index_or_names_its_step(f: Factory, caps: &Caps) {
        let ctx = Ctx::new(f, "rebuild-index");
        if let Some(step) = caps.rebuild_index {
            let mut txn = ctx.db.begin_write().unwrap();
            assert_unsupported(txn.rebuild_index(), step, "rebuild_index");
            return;
        }
        let a = key("dev-libs", "a-1");
        let b = key("app-misc", "b-1");
        put(
            &ctx,
            &a,
            &[
                (
                    "CONTENTS",
                    b"dir /usr\nobj /usr/lib/liba.so.1 abc 1\nsym /usr/lib/liba.so -> liba.so.1 1\nfoo /x\n",
                ),
                (
                    "NEEDED.ELF.2",
                    b"X86_64;/usr/lib/liba.so.1;liba.so.1;  -  ;libc.so.6\nX86_64;/usr/bin/a;;  -  ;liba.so.1\nshort;line\n",
                ),
                ("RDEPEND", b"dev-libs/o dev-libs/* ssl? ( dev-libs/lib )\n"),
            ],
            false,
        );
        put(
            &ctx,
            &b,
            &[
                ("CONTENTS", b"obj /usr/bin/b def 2\n"),
                ("DEPEND", b"dev-libs/o\n"),
            ],
            false,
        );
        // A pending entry gets no rows from a rebuild either.
        let p = key("dev-libs", "pending-1");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&p).unwrap();
        txn.put_entry_file(&p, "CONTENTS", b"obj /usr/bin/b ghi 3\n")
            .unwrap();
        txn.put_entry_file(&p, "RDEPEND", b"dev-libs/o\n").unwrap();
        txn.commit().unwrap();

        let all = [
            DepClass::Depend,
            DepClass::Rdepend,
            DepClass::Bdepend,
            DepClass::Pdepend,
            DepClass::Idepend,
        ];
        let paths: [&[u8]; 4] = [b"/usr", b"/usr/lib/liba.so", b"/usr/bin/b", b"/x"];
        let answers = || {
            let owners = sorted(ctx.db.owners(&paths).unwrap());
            let deps: Vec<Vec<EntryKey>> = ["dev-libs/o", "dev-libs/lib", "dev-libs/zz"]
                .iter()
                .map(|cp| {
                    ctx.db
                        .reverse_dependents(cp, &all)
                        .unwrap()
                        .into_iter()
                        .map(|r| r.key)
                        .collect()
                })
                .collect();
            (owners, deps)
        };
        let before = answers();
        assert_eq!(before.1[0], vec![b.clone(), a.clone()]);
        assert_eq!(before.1[2], vec![a.clone()], "dev-libs/* is unsure");
        let g0 = ctx.db.generation().unwrap();

        let mut txn = ctx.db.begin_write().unwrap();
        let counts = txn.rebuild_index().unwrap();
        txn.commit().unwrap();
        let want = IndexCounts {
            entries: 2,
            // a: dir, obj, sym (`foo` is no recorded kind); b: obj.
            owner: 4,
            // a: dev-libs/o, the unsure dev-libs/*, dev-libs/lib; b: dev-libs/o.
            dep_atom: 4,
            // a: two lines of five fields; the short one is skipped.
            needed: 2,
        };
        assert_eq!(counts, want);
        assert_eq!(answers(), before);
        assert_ne!(ctx.db.generation().unwrap(), g0, "a rebuild is a write");

        let mut txn = ctx.db.begin_write().unwrap();
        assert_eq!(txn.rebuild_index().unwrap(), want, "idempotent");
        drop(txn);
        assert_eq!(answers(), before, "a dropped rebuild changes nothing");
    }

    pub fn read_file_at_is_supported_or_names_its_step(f: Factory, caps: &Caps) {
        let ctx = Ctx::new(f, "at");
        let k = key("dev-libs", "a-1");
        put(&ctx, &k, &[("DATA", b"0123456789"), ("EMPTY", b"")], false);
        match caps.read_file_at {
            // Every backend implements it now (files: S7.2); the arm stays
            // for a future backend that names a step.
            Some(step) => {
                assert_unsupported(ctx.db.read_file_at(&k, "DATA", 0, 4), step, "read_file_at")
            }
            None => {
                let at = |off, len| ctx.db.read_file_at(&k, "DATA", off, len).unwrap();
                assert_eq!(at(0, 4).as_deref(), Some(&b"0123"[..]));
                assert_eq!(at(2, 3).as_deref(), Some(&b"234"[..]));
                assert_eq!(
                    at(8, 10).as_deref(),
                    Some(&b"89"[..]),
                    "short at end of file"
                );
                assert_eq!(at(10, 4).as_deref(), Some(&b""[..]));
                assert_eq!(at(0, 0).as_deref(), Some(&b""[..]));
                assert_eq!(
                    ctx.db.read_file_at(&k, "EMPTY", 0, 8).unwrap().as_deref(),
                    Some(&b""[..])
                );
                assert_eq!(ctx.db.read_file_at(&k, "NOPE", 0, 4).unwrap(), None);
                assert_eq!(
                    ctx.db
                        .read_file_at(&key("dev-libs", "zz-1"), "DATA", 0, 4)
                        .unwrap(),
                    None
                );
            }
        }
    }

    pub fn snapshot_is_supported_or_names_its_step(f: Factory, caps: &Caps) {
        let ctx = Ctx::new(f, "snapshot");
        let a = key("dev-libs", "a-1");
        let b = key("dev-libs", "b-1");
        put(
            &ctx,
            &a,
            &[("SLOT", b"2\n"), ("RDEPEND", b" x  y\n")],
            false,
        );
        put(&ctx, &b, &[("SLOT", b"!bad\n")], true);
        match caps.snapshot {
            // TODO(plan S3.2): FilesDb::snapshot is Unsupported ("nothing on
            // files uses it").
            Some(step) => assert_unsupported(ctx.db.snapshot(), step, "snapshot"),
            None => {
                let s = ctx.db.snapshot().unwrap();
                assert_eq!(s.generation, ctx.db.generation().unwrap());
                assert_eq!(s.entries.len(), 2);
                let fa = s.get(&a).expect("a in snapshot");
                assert_eq!(fa.get("SLOT"), Some("2"));
                assert_eq!(fa.get("RDEPEND"), Some("x y"));
                assert_eq!(fa.get("USE"), Some(""));
                assert_eq!(s.get(&b).unwrap().get("SLOT"), Some("0"));
                assert!(s.get(&key("dev-libs", "zz-1")).is_none());
                // Same values as aux_get, for every field of every entry.
                for (k, fields) in &s.entries {
                    for (name, v) in fields.iter() {
                        assert_eq!(aux(&ctx, k, name).as_deref(), Some(v), "{k} {name}");
                    }
                }
            }
        }
    }

    pub fn entry_image_and_insert_entry_are_supported_or_name_their_step(f: Factory, caps: &Caps) {
        let ctx = Ctx::new(f, "image");
        let k = key("dev-libs", "a-1");
        put(
            &ctx,
            &k,
            &[
                ("CONTENTS", b"obj /x\n"),
                ("EMPTY", b""),
                ("environment.bz2", BAD_UTF8),
            ],
            false,
        );
        match caps.entry_image {
            // TODO(plan S2.6/S2.7; the `files` stubs say "S2.6"): FilesDb
            // entry_image / insert_entry are Unsupported. The converters
            // need them; then flip `Caps::entry_image` to `None`.
            Some(step) => {
                assert_unsupported(ctx.db.entry_image(&k), step, "entry_image");
                let image = EntryImage {
                    key: k.clone(),
                    files: vec![EntryFile {
                        meta: FileMeta {
                            name: "CONTENTS".into(),
                            len: 0,
                            mode: 0o644,
                            mtime_ns: 1,
                        },
                        data: vec![],
                    }],
                    dir_mode: 0o755,
                    dir_mtime_ns: 1,
                    metadata_stamp: MetadataStamp::Absent,
                };
                let mut txn = ctx.db.begin_write().unwrap();
                assert_unsupported(txn.insert_entry(&image), step, "insert_entry");
            }
            None => {
                let image = ctx.db.entry_image(&k).unwrap().expect("entry exists");
                assert_eq!(image.key, k);
                assert_eq!(image.metadata_stamp, MetadataStamp::Absent);
                let names: Vec<&str> = image.files.iter().map(|f| f.meta.name.as_str()).collect();
                assert_eq!(
                    names,
                    ["CONTENTS", "EMPTY", "environment.bz2"],
                    "name order"
                );
                assert!(
                    ctx.db
                        .entry_image(&key("dev-libs", "zz-1"))
                        .unwrap()
                        .is_none()
                );

                // Copy into a second database of the same backend.
                let dst = Ctx::new(f, "image-dst");
                let mut txn = dst.db.begin_write().unwrap();
                txn.insert_entry(&image).unwrap();
                txn.commit().unwrap();
                assert!(dst.db.has_entry(&k).unwrap());
                for file in &image.files {
                    assert_eq!(
                        read(&dst, &k, &file.meta.name).as_deref(),
                        Some(&file.data[..]),
                        "{}",
                        file.meta.name
                    );
                }
                assert_eq!(
                    read(&dst, &k, "metadata"),
                    None,
                    "no metadata file is added"
                );
                let back = dst.db.entry_image(&k).unwrap().unwrap();
                assert_eq!(back.metadata_stamp, MetadataStamp::Absent);
                assert_eq!(
                    back.files
                        .iter()
                        .map(|f| (&f.meta.name, &f.data))
                        .collect::<Vec<_>>(),
                    image
                        .files
                        .iter()
                        .map(|f| (&f.meta.name, &f.data))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    /// `entry_stat` is `entry_image` without the bytes, on every backend.
    pub fn entry_stat_is_the_entry_image_without_bytes(f: Factory, caps: &Caps) {
        if caps.entry_image.is_some() {
            return;
        }
        let ctx = Ctx::new(f, "stat");
        let k = key("dev-libs", "a-1");
        put(
            &ctx,
            &k,
            &[("CONTENTS", b"obj /x\n"), ("EMPTY", b""), ("SLOT", b"0\n")],
            false,
        );
        let image = ctx.db.entry_image(&k).unwrap().expect("entry exists");
        let stat = ctx.db.entry_stat(&k).unwrap().expect("entry exists");
        assert_eq!(stat, EntryStat::from(image));
        assert!(
            ctx.db
                .entry_stat(&key("dev-libs", "zz-1"))
                .unwrap()
                .is_none()
        );
    }

    /// #307: `dir_mode` is permission bits on every backend -- a merged
    /// entry's directory reports `0o7xx`, never `0o040xxx`, in both the
    /// image and the stat -- so `verify` of a files VDB against a natively
    /// merged sqlite/redb one has no "directory mode" finding.
    pub fn dir_mode_is_permission_bits_on_every_backend(f: Factory, caps: &Caps) {
        if caps.entry_image.is_some() {
            return;
        }
        let ctx = Ctx::new(f, "dirmode");
        let k = key("dev-libs", "a-1");
        put(&ctx, &k, &[("SLOT", b"0\n")], false);
        let image = ctx.db.entry_image(&k).unwrap().expect("entry exists");
        let stat = ctx.db.entry_stat(&k).unwrap().expect("entry exists");
        assert_eq!(image.dir_mode & !0o7777, 0, "{:o}", image.dir_mode);
        assert_eq!(stat.dir_mode, image.dir_mode);
        assert_ne!(image.dir_mode & 0o700, 0, "{:o}", image.dir_mode);
    }

    /// The registry is process-global: one test, serialised, resets
    /// around itself so no other test of this binary sees its state.
    pub fn registry_hands_out_the_registered_backend(f: Factory, _c: &Caps) {
        static REGISTRY_LOCK: Mutex<()> = Mutex::new(());
        let _g = REGISTRY_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        portage_vdb::reset();
        let ctx = Ctx::new(f, "registry");
        let k = key("dev-libs", "a-1");
        put(&ctx, &k, &[("SLOT", b"0\n")], false);
        let registered = Arc::clone(&ctx.db);
        let kind = registered.kind();
        portage_vdb::register(&ctx.root, Arc::clone(&registered));
        let got = portage_vdb::for_root(&ctx.root);
        assert!(
            Arc::ptr_eq(&got, &registered),
            "for_root returns the registered backend"
        );
        assert_eq!(got.kind(), kind);
        assert_eq!(got.entries().unwrap(), vec![k]);
        // After reset an unregistered root is the default `files` backend.
        portage_vdb::reset();
        let default = portage_vdb::for_root(&ctx.root);
        assert!(!Arc::ptr_eq(&default, &registered));
        assert_eq!(default.kind(), portage_vdb::BackendKind::Files);
        portage_vdb::reset();
    }
}

/// Instantiate every generic test for one backend:
/// `conformance_suite!(name, |root| <Arc<dyn InstalledDb>>, Caps { .. })`.
macro_rules! conformance_suite {
    ($backend:ident, $factory:expr, $caps:expr) => {
        mod $backend {
            use super::*;

            const FACTORY: Factory = $factory;
            const CAPS: Caps = $caps;

            conformance_suite!(@tests
                empty_database_has_no_entries_stores_or_counter
                insert_entry_reads_back_exact_bytes_including_empty_and_invalid_utf8
                list_files_and_file_meta_describe_stored_files
                missing_entry_or_file_reads_as_none
                aux_get_normalises_whitespace_and_invalid_slot
                aux_get_serves_valid_slots_unchanged
                aux_get_missing_field_is_empty_and_missing_entry_is_none
                aux_get_rejects_keys_outside_the_23_fields
                aux_get_is_the_same_with_and_without_seal
                seal_of_an_entry_without_metadata_fields_adds_no_metadata_file
                pending_entry_is_invisible_until_finish
                begin_entry_discards_a_stale_pending_entry
                pending_replacement_leaves_the_live_entry_until_finish
                discard_pending_removes_an_orphan_and_never_an_installed_entry
                replace_in_slot_leaves_only_the_new_entry
                delete_entry_removes_only_that_entry
                replace_file_rewrites_one_file_and_leaves_the_others
                counter_ticks_monotonically_and_persists
                set_counter_round_trips_or_names_its_step
                world_and_world_sets_round_trip_independently
                empty_world_lists_clear_the_stores
                preserved_libs_round_trip_and_unchanged_write_is_skipped
                config_memory_round_trips_and_is_written_unconditionally
                generation_changes_after_entry_commits_and_is_stable_across_reads
                owners_report_the_entries_that_claim_paths
                read_file_all_lists_every_live_entry_with_none_for_a_missing_file
                categories_and_category_entries_list_live_entries
                reverse_dependents_is_supported_or_names_its_step
                reverse_dependents_index_is_a_superset_and_keeps_unsure_entries
                owners_follow_the_claim_paths_rule_on_edge_spellings
                read_file_at_is_supported_or_names_its_step
                snapshot_is_supported_or_names_its_step
                entry_image_and_insert_entry_are_supported_or_name_their_step
                entry_stat_is_the_entry_image_without_bytes
                dir_mode_is_permission_bits_on_every_backend
                registry_hands_out_the_registered_backend
                rebuild_index_recomputes_the_same_index_or_names_its_step
            );
        }
    };
    (@tests $($name:ident)*) => {
        $(
            #[test]
            fn $name() {
                suite::$name(FACTORY, &CAPS);
            }
        )*
    };
}

// ------------------------------------------------------------- backends

conformance_suite!(
    files,
    |root| Arc::new(FilesDb::new(root)),
    Caps {
        // S2.6: `entry_image`, `insert_entry` and `set_counter` are
        // implemented on files too.
        set_counter: None,
        reverse_dependents: Some("reverse_dependents"),
        snapshot: Some("S3.2"),
        read_file_at: None,
        entry_image: None,
        rebuild_index: Some("S8.3"),
        seal_stores_metadata_file: true,
    }
);

// S2.5: every method is implemented on sqlite. `seal_entry` stores the
// consolidated `metadata` file as a row, like `files` stores the file.
#[cfg(feature = "vdb-sqlite")]
conformance_suite!(
    sqlite,
    |root| Arc::new(portage_vdb::SqliteDb::open(root.join("vdb.sqlite")).unwrap()),
    Caps {
        set_counter: None,
        reverse_dependents: None,
        snapshot: None,
        read_file_at: None,
        entry_image: None,
        rebuild_index: None,
        seal_stores_metadata_file: true,
    }
);

/// redb refuses a second read-write handle on one file, in this process
/// too (`Error::Busy`), but the generic tests call the factory again for
/// "a second handle on the same storage" while the first is alive. The
/// factory therefore hands out the one live handle of a path (a `Weak`
/// table, so it closes with its `Ctx`). Persistence across a real reopen
/// is covered by the unit tests in `redb_db.rs`.
#[cfg(feature = "vdb-redb")]
fn redb_handle(root: &Path) -> Arc<dyn InstalledDb> {
    use std::collections::HashMap;
    use std::sync::Weak;
    static OPEN: Mutex<Option<HashMap<PathBuf, Weak<portage_vdb::RedbDb>>>> = Mutex::new(None);
    let path = root.join("vdb.redb");
    let mut g = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
    let map = g.get_or_insert_with(HashMap::new);
    map.retain(|_, w| w.strong_count() > 0);
    if let Some(db) = map.get(&path).and_then(Weak::upgrade) {
        return db;
    }
    let db = Arc::new(portage_vdb::RedbDb::open(&path).unwrap());
    map.insert(path, Arc::downgrade(&db));
    db
}

// S5.3: every method is implemented on redb, in one redb write
// transaction per `begin_write`.
#[cfg(feature = "vdb-redb")]
conformance_suite!(
    redb,
    |root| redb_handle(root),
    Caps {
        set_counter: None,
        reverse_dependents: None,
        snapshot: None,
        read_file_at: None,
        entry_image: None,
        rebuild_index: None,
        seal_stores_metadata_file: true,
    }
);

// --------------------------------------------------- backend-independent

/// `parse_preserved_libs` / `format_preserved_libs` are the `files`
/// formats the converters read and write (real `json.dumps` layout).
#[test]
fn preserved_libs_format_and_parse_round_trip() {
    use portage_vdb::{format_preserved_libs, parse_preserved_libs};
    assert_eq!(format_preserved_libs(&BTreeMap::new()), "{}");
    assert_eq!(parse_preserved_libs("{}"), Some(BTreeMap::new()));
    let mut m = BTreeMap::new();
    m.insert(
        "dev-libs/a:0".to_string(),
        PreservedLibsEntry {
            cpv: "dev-libs/a-1".into(),
            counter: "5".into(),
            paths: vec!["/usr/lib/a\"\\\n\t.so".into(), "/usr/lib/\u{e9}.so".into()],
        },
    );
    m.insert(
        "dev-libs/b:1".to_string(),
        PreservedLibsEntry {
            cpv: "dev-libs/b-1".into(),
            counter: "6".into(),
            paths: vec![],
        },
    );
    let text = format_preserved_libs(&m);
    assert!(text.starts_with("{\n\t\"dev-libs/a:0\": [\n\t\t\"dev-libs/a-1\",\n\t\t\"5\",\n"));
    assert!(
        !text.ends_with('\n'),
        "no trailing newline, like json.dumps"
    );
    assert_eq!(parse_preserved_libs(&text), Some(m));
    assert_eq!(parse_preserved_libs("not json"), None);
    assert_eq!(parse_preserved_libs("{\"a\": 1}"), None);
}

// ------------------------------------------------------------ files only

/// Behaviour that belongs to the `files` layout and is not part of the
/// backend contract: directory shapes, on-disk bytes, exact generation
/// semantics.
mod files_only {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    fn files_ctx(tag: &str) -> Ctx {
        Ctx::new(|root| Arc::new(FilesDb::new(root)), tag)
    }

    fn vdb(ctx: &Ctx) -> PathBuf {
        ctx.root.join("var/db/pkg")
    }

    #[test]
    fn a_missing_vdb_has_generation_zero_and_a_created_one_does_not() {
        let ctx = files_ctx("fgen0");
        assert_eq!(ctx.db.generation().unwrap(), 0);
        assert_eq!(ctx.db.category_generation("dev-libs").unwrap(), 0);
        put(&ctx, &key("dev-libs", "a-1"), &[("SLOT", b"0\n")], false);
        assert_ne!(ctx.db.generation().unwrap(), 0);
        assert_ne!(ctx.db.category_generation("dev-libs").unwrap(), 0);
        assert_eq!(ctx.db.category_generation("app-misc").unwrap(), 0);
    }

    /// ODDITY (documented, not a bug of the step): on `files` the
    /// generation is a fingerprint of directory mtimes, so commits that
    /// touch no VDB directory (world, counter, preserved libs, config
    /// memory, W4 `replace_file`, which rewrites in place on purpose) do
    /// not change it. The database backends bump `meta.generation` on every
    /// commit, so the generic suite asserts a change only for entry-level
    /// commits.
    #[test]
    fn generation_ignores_commits_that_touch_no_vdb_directory() {
        let ctx = files_ctx("fgen-nochange");
        let k = key("dev-libs", "a-1");
        put(&ctx, &k, &[("CONTENTS", b"old\n")], false);
        let g = ctx.db.generation().unwrap();
        let cg = ctx.db.category_generation("dev-libs").unwrap();
        settle();
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&World {
            atoms: vec!["dev-libs/a".into()],
        })
        .unwrap();
        txn.next_counter().unwrap();
        txn.replace_file(&k, "CONTENTS", b"new\n").unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.generation().unwrap(), g);
        assert_eq!(ctx.db.category_generation("dev-libs").unwrap(), cg);
    }

    #[test]
    fn delete_removes_an_emptied_category_directory() {
        let ctx = files_ctx("fdelete");
        let a = key("dev-libs", "a-1");
        let b = key("dev-libs", "b-1");
        put(&ctx, &a, &[("CONTENTS", b"")], false);
        put(&ctx, &b, &[("CONTENTS", b"")], false);
        let mut txn = ctx.db.begin_write().unwrap();
        txn.delete_entry(&a).unwrap();
        assert!(
            vdb(&ctx).join("dev-libs").is_dir(),
            "a sibling keeps the category"
        );
        txn.delete_entry(&b).unwrap();
        txn.commit().unwrap();
        assert!(!vdb(&ctx).join("dev-libs").exists());
        assert!(ctx.db.categories().unwrap().is_empty());
        // A pending entry keeps the category directory alive (replace in
        // the only entry of a category).
        put(&ctx, &a, &[("CONTENTS", b"")], false);
        let n = key("dev-libs", "a-2");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&n).unwrap();
        txn.delete_entry(&a).unwrap();
        assert!(vdb(&ctx).join("dev-libs").is_dir());
        txn.finish_entry(&n).unwrap();
        txn.commit().unwrap();
        assert_eq!(ctx.db.entries().unwrap(), vec![n]);
    }

    #[test]
    fn delete_of_a_missing_entry_is_an_io_error_naming_its_path() {
        let ctx = files_ctx("fdelmiss");
        let k = key("dev-libs", "nope-1");
        let mut txn = ctx.db.begin_write().unwrap();
        let err = txn.delete_entry(&k).unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err}");
        assert!(
            err.to_string().starts_with(&format!(
                "{}: ",
                vdb(&ctx).join("dev-libs/nope-1").display()
            )),
            "{err}"
        );
    }

    #[test]
    fn paths_follow_the_historic_layout() {
        let ctx = files_ctx("flayout");
        let k = key("dev-libs", "a-1");
        assert_eq!(ctx.db.vdb_dir(), Some(vdb(&ctx)));
        assert_eq!(ctx.db.entry_path(&k), Some(vdb(&ctx).join("dev-libs/a-1")));
        assert_eq!(ctx.db.kind(), portage_vdb::BackendKind::Files);
        let mut txn = ctx.db.begin_write().unwrap();
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "CONTENTS", b"x").unwrap();
        txn.commit().unwrap();
        assert!(vdb(&ctx).join("dev-libs/-MERGING-a-1/CONTENTS").is_file());
        let mut txn = ctx.db.begin_write().unwrap();
        txn.finish_entry(&k).unwrap();
        txn.commit().unwrap();
        assert!(!vdb(&ctx).join("dev-libs/-MERGING-a-1").exists());
        assert_eq!(
            fs::read(vdb(&ctx).join("dev-libs/a-1/CONTENTS")).unwrap(),
            b"x"
        );
    }

    #[test]
    fn counter_starts_at_zero_and_is_a_bare_number_file() {
        let ctx = files_ctx("fcounter");
        let mut txn = ctx.db.begin_write().unwrap();
        assert_eq!(txn.next_counter().unwrap(), Counter(0));
        assert_eq!(txn.next_counter().unwrap(), Counter(1));
        txn.commit().unwrap();
        assert_eq!(
            fs::read(ctx.root.join("var/cache/edb/counter")).unwrap(),
            b"1"
        );
        // Unparsable store reads as missing, the next tick is 0 again
        // (portuale's rule, plan section 0.7: filed as a residue).
        fs::write(ctx.root.join("var/cache/edb/counter"), "garbage").unwrap();
        assert_eq!(ctx.db.counter().unwrap(), None);
        let mut txn = ctx.db.begin_write().unwrap();
        assert_eq!(txn.next_counter().unwrap(), Counter(0));
    }

    #[test]
    fn d4_stores_are_written_in_the_historic_formats() {
        let ctx = files_ctx("fstores");
        let mut libs = PreservedLibs::default();
        libs.entries.insert(
            "dev-libs/a:0".into(),
            PreservedLibsEntry {
                cpv: "dev-libs/a-1".into(),
                counter: "3".into(),
                paths: vec!["/usr/lib/liba.so.1".into()],
            },
        );
        let mut mem = ConfigMemory::default();
        mem.entries.insert("/etc/a".into(), "abc".into());
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&World {
            atoms: vec!["dev-libs/a".into(), "dev-libs/b".into()],
        })
        .unwrap();
        txn.set_world_sets(&WorldSets {
            sets: vec!["x".into()],
        })
        .unwrap();
        txn.set_preserved_libs(&libs).unwrap();
        txn.set_config_memory(&mem).unwrap();
        txn.commit().unwrap();
        let p = ctx.root.join("var/lib/portage");
        assert_eq!(
            fs::read_to_string(p.join("world")).unwrap(),
            "dev-libs/a\ndev-libs/b\n"
        );
        assert_eq!(fs::read_to_string(p.join("world_sets")).unwrap(), "@x\n");
        assert_eq!(
            fs::read_to_string(p.join("preserved_libs_registry")).unwrap(),
            portage_vdb::format_preserved_libs(&libs.entries)
        );
        assert_eq!(
            fs::read_to_string(p.join("config")).unwrap(),
            "/etc/a abc\n"
        );
        // Empty world lists are empty files, not removed files.
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_world(&World::default()).unwrap();
        txn.commit().unwrap();
        assert_eq!(fs::read(p.join("world")).unwrap(), b"");
        // A missing or unparsable registry is empty, not an error.
        fs::write(p.join("preserved_libs_registry"), "not json").unwrap();
        assert!(ctx.db.preserved_libs().unwrap().entries.is_empty());
    }

    /// World reading drops blanks, comments and `@` lines; world_sets
    /// keeps only `@` lines (hand-edited files).
    #[test]
    fn world_readers_filter_hand_edited_files() {
        let ctx = files_ctx("fworldedit");
        let p = ctx.root.join("var/lib/portage");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("world"), "# c\n\n @x \n dev-libs/a \n").unwrap();
        assert_eq!(
            ctx.db.world().unwrap().atoms,
            vec!["dev-libs/a".to_string()]
        );
        fs::write(p.join("world_sets"), "dev-libs/a\n# c\n@@one\n@two\n").unwrap();
        assert_eq!(
            ctx.db.world_sets().unwrap().sets,
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn seal_writes_a_valid_stamp_last_and_it_survives_publish() {
        let ctx = files_ctx("fseal");
        let k = key("dev-libs", "a-1");
        put(
            &ctx,
            &k,
            &[
                ("SLOT", b"0\n"),
                ("RDEPEND", b" a  b\n"),
                ("CONTENTS", b"x"),
            ],
            true,
        );
        let meta = String::from_utf8(read(&ctx, &k, "metadata").unwrap()).unwrap();
        assert!(
            meta.starts_with("#format=1\nRDEPEND=a b\nSLOT=0\n#dir_mtime="),
            "{meta}"
        );
        let st = fs::metadata(vdb(&ctx).join("dev-libs/a-1")).unwrap();
        let ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
        assert!(meta.ends_with(&format!("#dir_mtime={ns}\n")), "{meta}");
        // CONTENTS is not one of the 23 fields and is not in `metadata`.
        assert!(!meta.contains("CONTENTS"));
        assert!(
            ctx.db
                .list_files(&k)
                .unwrap()
                .unwrap()
                .iter()
                .any(|m| m.name == "metadata")
        );
    }

    /// W4 `replace_file` rewrites in place: the entry directory's mtime
    /// (and so the `metadata` stamp) is unchanged.
    #[test]
    fn replace_file_keeps_the_entry_directory_mtime_and_the_stamp() {
        let ctx = files_ctx("fw4");
        let k = key("dev-libs", "a-1");
        put(&ctx, &k, &[("SLOT", b"0\n"), ("CONTENTS", b"old\n")], true);
        let dir = vdb(&ctx).join("dev-libs/a-1");
        let before = fs::metadata(&dir).unwrap().modified().unwrap();
        let meta_before = read(&ctx, &k, "metadata");
        settle();
        let mut txn = ctx.db.begin_write().unwrap();
        txn.replace_file(&k, "CONTENTS", b"new content\n").unwrap();
        txn.commit().unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().modified().unwrap(), before);
        assert_eq!(read(&ctx, &k, "metadata"), meta_before);
        assert_eq!(aux(&ctx, &k, "SLOT").as_deref(), Some("0"));
    }

    /// ODDITY (FilesDb, found by S2.2; documents current behaviour, update when fixed): `seal_entry` skips a field file that is not UTF-8, and
    /// a valid snapshot is complete, so a sealed entry serves `""` where an
    /// unsealed one serves the lossy text.
    #[test]
    fn oddity_sealed_entry_drops_a_non_utf8_field_from_aux_get() {
        let ctx = files_ctx("fprobe");
        let a = key("dev-libs", "a-1");
        let b = key("dev-libs", "b-1");
        let files: &[(&str, &[u8])] = &[("SLOT", b"0\n"), ("DESCRIPTION", b"caf\xe9 x\n")];
        put(&ctx, &a, files, false);
        put(&ctx, &b, files, true);
        assert_eq!(
            aux(&ctx, &a, "DESCRIPTION").as_deref(),
            Some("caf\u{fffd} x")
        );
        assert_eq!(aux(&ctx, &b, "DESCRIPTION").as_deref(), Some(""));
    }

    /// A bare VDB directory has no D4 stores: reads are empty, writes
    /// are Unsupported.
    #[test]
    fn a_bare_vdb_dir_reads_empty_stores_and_refuses_store_writes() {
        let ctx = files_ctx("fbare");
        let db = FilesDb::open_vdb_dir(&ctx.root);
        assert_eq!(db.world().unwrap(), World::default());
        assert_eq!(db.counter().unwrap(), None);
        let mut txn = db.begin_write().unwrap();
        assert!(matches!(
            txn.set_world(&World::default()),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(txn.next_counter(), Err(Error::Unsupported(_))));
    }

    // ---------------------------------------------- S2.6: whole entries

    fn mtime_ns(p: &Path) -> i128 {
        let st = fs::metadata(p).unwrap();
        st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128
    }

    fn set_times(p: &Path, ns: i128) {
        let t = std::time::UNIX_EPOCH + Duration::from_nanos(ns as u64);
        fs::File::open(p)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(t).set_modified(t))
            .unwrap();
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().mode() & 0o7777
    }

    /// The facts the `insert_entry` write order relies on, measured.
    #[test]
    fn which_operations_change_a_directorys_mtime() {
        let ctx = files_ctx("fmtimefacts");
        let parent = ctx.root.join("p");
        let dir = parent.join("d");
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("f");
        fs::write(&f, b"x").unwrap();
        let t = 1_700_000_000_123_456_789i128;
        set_times(&dir, t);
        assert_eq!(mtime_ns(&dir), t, "futimens sets the directory exactly");
        let same = |what: &str| assert_eq!(mtime_ns(&dir), t, "{what} changed the dir mtime");
        // Unchanged:
        set_times(&f, 5_000_000_001);
        same("utimens of a file inside");
        fs::set_permissions(&f, fs::Permissions::from_mode(0o640)).unwrap();
        same("chmod of a file inside");
        fs::write(&f, b"longer content").unwrap();
        same("in-place truncate+write of an existing file");
        {
            use std::io::Write as _;
            let mut h = fs::OpenOptions::new().append(true).open(&f).unwrap();
            h.write_all(b"more").unwrap();
        }
        same("append to an existing file");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o750)).unwrap();
        same("chmod of the dir itself");
        let moved = parent.join("d2");
        fs::rename(&dir, &moved).unwrap();
        assert_eq!(mtime_ns(&moved), t, "rename within the parent");
        let dir = moved;
        // Changed:
        std::thread::sleep(Duration::from_millis(5));
        fs::write(dir.join("new"), b"").unwrap();
        assert_ne!(mtime_ns(&dir), t, "creating a file");
        set_times(&dir, t);
        fs::rename(dir.join("new"), dir.join("new2")).unwrap();
        assert_ne!(mtime_ns(&dir), t, "renaming a file inside");
        set_times(&dir, t);
        fs::remove_file(dir.join("new2")).unwrap();
        assert_ne!(mtime_ns(&dir), t, "removing a file");
    }

    const T_DIR: i128 = 1_650_000_000_111_222_333;
    const T_META: i128 = 1_650_000_001_000_000_007;
    const T_A: i128 = 1_650_000_002_000_000_011;
    const T_B: i128 = 1_650_000_003_999_999_999;

    /// A fixture-like entry in a fresh files database, with odd modes and
    /// mtimes, invalid UTF-8, an empty file, and a `metadata` file whose
    /// stamp is Valid (`stale == false`) or made stale by touching the
    /// directory afterwards. `SLOT` differs between the snapshot (`0`) and
    /// the field file (`9`, rewritten in place) so `aux_get` shows which
    /// one is served.
    fn fixture(tag: &str, k: &EntryKey, stale: bool, metadata: bool) -> (Ctx, EntryImage) {
        let ctx = files_ctx(tag);
        put(
            &ctx,
            k,
            &[
                ("SLOT", b"0\n"),
                ("RDEPEND", b" a  b\n"),
                ("CONTENTS", b"obj /x\n"),
                ("EMPTY", b""),
                ("environment.bz2", BAD_UTF8),
            ],
            metadata,
        );
        let dir = vdb(&ctx).join(&k.category).join(&k.pf);
        let mut txn = ctx.db.begin_write().unwrap();
        txn.replace_file(k, "SLOT", b"9\n").unwrap();
        txn.commit().unwrap();
        set_times(&dir.join("CONTENTS"), T_A);
        fs::set_permissions(dir.join("CONTENTS"), fs::Permissions::from_mode(0o600)).unwrap();
        set_times(&dir.join("EMPTY"), T_B);
        fs::set_permissions(dir.join("EMPTY"), fs::Permissions::from_mode(0o444)).unwrap();
        if metadata {
            set_times(&dir.join("metadata"), T_META);
            fs::set_permissions(dir.join("metadata"), fs::Permissions::from_mode(0o444)).unwrap();
        }
        if stale {
            std::thread::sleep(Duration::from_millis(5));
            fs::write(dir.join("touch"), b"").unwrap();
            fs::remove_file(dir.join("touch")).unwrap();
        }
        set_times(&dir, if stale { T_DIR + 7 } else { mtime_ns(&dir) });
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o750)).unwrap();
        let img = ctx.db.entry_image(k).unwrap().unwrap();
        (ctx, img)
    }

    fn assert_same_image(a: &EntryImage, b: &EntryImage) {
        assert_eq!(a.dir_mtime_ns, b.dir_mtime_ns, "dir mtime");
        assert_eq!(a.dir_mode, b.dir_mode, "dir mode");
        assert_eq!(a.metadata_stamp, b.metadata_stamp, "stamp state");
        assert_eq!(a.files.len(), b.files.len());
        for (x, y) in a.files.iter().zip(&b.files) {
            assert_eq!(x.meta.name, y.meta.name);
            assert_eq!(x.data, y.data, "{}", x.meta.name);
            assert_eq!(
                x.meta.mode & 0o7777,
                y.meta.mode & 0o7777,
                "{}",
                x.meta.name
            );
            assert_eq!(x.meta.mtime_ns, y.meta.mtime_ns, "{}", x.meta.name);
        }
    }

    #[test]
    fn entry_image_reports_the_three_stamp_states() {
        let k = key("dev-libs", "a-1");
        let (_c1, valid) = fixture("fimg-valid", &k, false, true);
        assert_eq!(valid.metadata_stamp, MetadataStamp::Valid);
        let (_c2, stale) = fixture("fimg-stale", &k, true, true);
        assert_eq!(stale.metadata_stamp, MetadataStamp::Stale);
        let (_c3, absent) = fixture("fimg-absent", &k, false, false);
        assert_eq!(absent.metadata_stamp, MetadataStamp::Absent);
        assert_eq!(valid.dir_mode, 0o750, "permission bits only, no S_IFDIR");
        let names: Vec<_> = valid.files.iter().map(|f| f.meta.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "CONTENTS",
                "EMPTY",
                "RDEPEND",
                "SLOT",
                "environment.bz2",
                "metadata"
            ]
        );
        let c = valid
            .files
            .iter()
            .find(|f| f.meta.name == "CONTENTS")
            .unwrap();
        assert_eq!((c.meta.mode & 0o7777, c.meta.mtime_ns), (0o600, T_A));
        assert!(
            valid
                .files
                .iter()
                .find(|f| f.meta.name == "environment.bz2")
                .unwrap()
                .data
                == BAD_UTF8
        );
    }

    #[test]
    fn insert_entry_valid_stays_valid_and_the_reader_serves_the_snapshot() {
        let k = key("dev-libs", "a-1");
        let (src, img) = fixture("fins-valid-src", &k, false, true);
        assert_eq!(aux(&src, &k, "SLOT").as_deref(), Some("0"), "snapshot");
        let dst = files_ctx("fins-valid-dst");
        let mut txn = dst.db.begin_write().unwrap();
        txn.insert_entry(&img).unwrap();
        txn.commit().unwrap();
        let dir = vdb(&dst).join("dev-libs/a-1");
        assert_eq!(mtime_ns(&dir), img.dir_mtime_ns, "dir mtime preserved");
        assert_eq!(mode(&dir), 0o750);
        // The stamp is the directory's mtime, by the raw bytes.
        let meta = String::from_utf8(fs::read(dir.join("metadata")).unwrap()).unwrap();
        assert!(
            meta.ends_with(&format!("#dir_mtime={}\n", img.dir_mtime_ns)),
            "{meta}"
        );
        assert_eq!(meta.matches("#dir_mtime=").count(), 1);
        // A fresh handle: reader rule accepts, snapshot is served.
        let fresh = FilesDb::new(&dst.root);
        assert_eq!(
            fresh.aux_get(&k, "SLOT").unwrap().as_deref(),
            Some("0"),
            "snapshot served (the SLOT field file says 9)"
        );
        assert_eq!(read(&dst, &k, "SLOT").as_deref(), Some(&b"9\n"[..]));
        let back = fresh.entry_image(&k).unwrap().unwrap();
        assert_eq!(back.metadata_stamp, MetadataStamp::Valid);
        assert_same_image(&img, &back);
        // No pending directory is left behind.
        assert!(!dst.db.has_entry(&key("dev-libs", "-MERGING-a-1")).unwrap());
        assert!(!vdb(&dst).join("dev-libs/-MERGING-a-1").exists());
    }

    /// A Valid image whose stored stamp is old (another backend kept the
    /// bytes of the source host) gets the new directory's value.
    #[test]
    fn insert_entry_valid_replaces_an_old_stamp() {
        let k = key("dev-libs", "a-1");
        let (_src, mut img) = fixture("fins-oldstamp-src", &k, false, true);
        img.dir_mtime_ns = T_DIR;
        let meta = img
            .files
            .iter_mut()
            .find(|f| f.meta.name == "metadata")
            .unwrap();
        meta.data = String::from_utf8(meta.data.clone())
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("#dir_mtime="))
            .map(|l| format!("{l}\n"))
            .collect::<String>()
            .replace("SLOT=0\n", "SLOT=0\n#dir_mtime=12345\n")
            .into_bytes();
        let dst = files_ctx("fins-oldstamp-dst");
        let mut txn = dst.db.begin_write().unwrap();
        txn.insert_entry(&img).unwrap();
        txn.commit().unwrap();
        let text = String::from_utf8(read(&dst, &k, "metadata").unwrap()).unwrap();
        assert_eq!(text.matches("#dir_mtime=").count(), 1, "{text}");
        assert!(text.ends_with(&format!("#dir_mtime={T_DIR}\n")), "{text}");
        assert_eq!(
            FilesDb::new(&dst.root)
                .aux_get(&k, "SLOT")
                .unwrap()
                .as_deref(),
            Some("0")
        );
    }

    #[test]
    fn insert_entry_stale_stays_stale_with_the_stored_bytes() {
        let k = key("dev-libs", "a-1");
        let (_src, img) = fixture("fins-stale-src", &k, true, true);
        assert_eq!(img.metadata_stamp, MetadataStamp::Stale);
        let stored = img
            .files
            .iter()
            .find(|f| f.meta.name == "metadata")
            .unwrap();
        let dst = files_ctx("fins-stale-dst");
        let mut txn = dst.db.begin_write().unwrap();
        txn.insert_entry(&img).unwrap();
        txn.commit().unwrap();
        assert_eq!(
            read(&dst, &k, "metadata").as_deref(),
            Some(&stored.data[..])
        );
        let fresh = FilesDb::new(&dst.root);
        assert_eq!(
            fresh.aux_get(&k, "SLOT").unwrap().as_deref(),
            Some("9"),
            "stale snapshot is ignored; the field file is read"
        );
        let back = fresh.entry_image(&k).unwrap().unwrap();
        assert_eq!(back.metadata_stamp, MetadataStamp::Stale);
        assert_same_image(&img, &back);
    }

    /// An image that says Stale but whose stamp equals the directory mtime
    /// (a foreign producer) is made stale by moving the stamp by one.
    #[test]
    fn insert_entry_stale_never_matches_even_when_the_stored_stamp_equals_the_dir_mtime() {
        let k = key("dev-libs", "a-1");
        let (_src, mut img) = fixture("fins-stale-eq-src", &k, true, true);
        img.dir_mtime_ns = T_DIR;
        let meta = img
            .files
            .iter_mut()
            .find(|f| f.meta.name == "metadata")
            .unwrap();
        let text = String::from_utf8(meta.data.clone()).unwrap();
        let body: String = text
            .lines()
            .filter(|l| !l.starts_with("#dir_mtime="))
            .map(|l| format!("{l}\n"))
            .collect();
        meta.data = format!("{body}#dir_mtime={T_DIR}\n").into_bytes();
        let dst = files_ctx("fins-stale-eq-dst");
        let mut txn = dst.db.begin_write().unwrap();
        txn.insert_entry(&img).unwrap();
        txn.commit().unwrap();
        let back = dst.db.entry_image(&k).unwrap().unwrap();
        assert_eq!(back.metadata_stamp, MetadataStamp::Stale);
        let text = String::from_utf8(read(&dst, &k, "metadata").unwrap()).unwrap();
        assert!(
            text.ends_with(&format!("#dir_mtime={}\n", T_DIR - 1)),
            "{text}"
        );
        assert_eq!(
            FilesDb::new(&dst.root)
                .aux_get(&k, "SLOT")
                .unwrap()
                .as_deref(),
            Some("9")
        );
    }

    #[test]
    fn insert_entry_absent_adds_no_metadata_file_and_keeps_modes_and_times() {
        let k = key("dev-libs", "a-1");
        let (_src, img) = fixture("fins-absent-src", &k, false, false);
        let dst = files_ctx("fins-absent-dst");
        let mut txn = dst.db.begin_write().unwrap();
        txn.insert_entry(&img).unwrap();
        txn.commit().unwrap();
        let dir = vdb(&dst).join("dev-libs/a-1");
        assert!(!dir.join("metadata").exists());
        assert_eq!(mode(&dir.join("EMPTY")), 0o444);
        assert_eq!(mtime_ns(&dir.join("EMPTY")), T_B);
        assert_eq!(mtime_ns(&dir.join("CONTENTS")), T_A);
        assert_eq!(mode(&dir.join("CONTENTS")), 0o600);
        assert_eq!(fs::read(dir.join("environment.bz2")).unwrap(), BAD_UTF8);
        assert_eq!(mtime_ns(&dir), img.dir_mtime_ns);
        assert_same_image(&img, &dst.db.entry_image(&k).unwrap().unwrap());
    }

    #[test]
    fn insert_entry_replaces_a_live_entry_and_leaves_no_pending_dir() {
        let k = key("dev-libs", "a-1");
        let (_src, img) = fixture("fins-repl-src", &k, false, true);
        let dst = files_ctx("fins-repl-dst");
        put(&dst, &k, &[("OLD", b"old"), ("SLOT", b"1\n")], false);
        let mut txn = dst.db.begin_write().unwrap();
        txn.insert_entry(&img).unwrap();
        txn.commit().unwrap();
        assert_eq!(read(&dst, &k, "OLD"), None);
        assert_same_image(&img, &dst.db.entry_image(&k).unwrap().unwrap());
        let names: Vec<_> = fs::read_dir(vdb(&dst).join("dev-libs"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a-1"]);
    }

    #[test]
    fn insert_entry_rejects_inconsistent_images_and_bad_names() {
        let k = key("dev-libs", "a-1");
        let (_c, img) = fixture("fins-bad", &k, false, true);
        let dst = files_ctx("fins-bad-dst");
        let mut bad = img.clone();
        bad.metadata_stamp = MetadataStamp::Absent;
        let mut txn = dst.db.begin_write().unwrap();
        assert!(matches!(txn.insert_entry(&bad), Err(Error::Invalid(_))));
        let mut bad = img.clone();
        bad.files.retain(|f| f.meta.name != "metadata");
        assert!(matches!(txn.insert_entry(&bad), Err(Error::Invalid(_))));
        let mut bad = img.clone();
        bad.files[0].meta.name = "../x".into();
        assert!(matches!(txn.insert_entry(&bad), Err(Error::Invalid(_))));
        assert!(!dst.db.has_entry(&k).unwrap());
        assert!(!vdb(&dst).join("dev-libs/-MERGING-a-1").exists());
    }

    #[test]
    fn set_counter_writes_the_bare_integer() {
        let ctx = files_ctx("fsetcounter");
        let mut txn = ctx.db.begin_write().unwrap();
        txn.set_counter(Counter(12345)).unwrap();
        txn.commit().unwrap();
        assert_eq!(
            fs::read(ctx.root.join("var/cache/edb/counter")).unwrap(),
            b"12345"
        );
        assert_eq!(ctx.db.counter().unwrap(), Some(Counter(12345)));
    }

    #[cfg(feature = "vdb-sqlite")]
    fn tree(dir: &Path) -> Vec<(String, Vec<u8>, u32, i128)> {
        let mut v = vec![(".".to_string(), vec![], mode(dir), mtime_ns(dir))];
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        names.sort();
        for p in names {
            v.push((
                p.file_name().unwrap().to_str().unwrap().to_string(),
                fs::read(&p).unwrap(),
                mode(&p),
                mtime_ns(&p),
            ));
        }
        v
    }

    /// files -> sqlite -> files gives byte-identical trees (bytes, modes,
    /// mtimes, directory mode and mtime) for each stamp state.
    #[cfg(feature = "vdb-sqlite")]
    #[test]
    fn round_trip_files_sqlite_files_is_byte_identical() {
        for (tag, stale, metadata) in [
            ("valid", false, true),
            ("stale", true, true),
            ("absent", false, false),
        ] {
            let k = key("dev-libs", "a-1");
            let (src, img) = fixture(&format!("frt-{tag}"), &k, stale, metadata);
            let sq_root = files_ctx(&format!("frt-sq-{tag}"));
            let sq = portage_vdb::SqliteDb::open(sq_root.root.join("vdb.sqlite")).unwrap();
            let mut txn = sq.begin_write().unwrap();
            txn.insert_entry(&img).unwrap();
            txn.commit().unwrap();
            let mid = sq.entry_image(&k).unwrap().unwrap();
            let dst = files_ctx(&format!("frt-dst-{tag}"));
            let mut txn = dst.db.begin_write().unwrap();
            txn.insert_entry(&mid).unwrap();
            txn.commit().unwrap();
            assert_eq!(
                tree(&vdb(&src).join("dev-libs/a-1")),
                tree(&vdb(&dst).join("dev-libs/a-1")),
                "{tag}"
            );
        }
    }
}
