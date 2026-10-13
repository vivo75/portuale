use super::*;
use portage_vdb::{EntryKey, InstalledDb};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

type Tree = BTreeMap<String, (u32, Vec<u8>)>;

fn binpkg(name: &str) -> PathBuf {
    fixtures_root().join("pkgdir/dev-libs").join(name)
}

/// Convert whatever `files` VDB `root` holds into a new sqlite
/// database at `db_path` (as `portuale vdb convert` does) and make
/// it `root`'s backend, as `mrg --vdb-backend=sqlite` does.
#[cfg(feature = "vdb-sqlite")]
fn use_sqlite(root: &Path, db_path: &Path) -> Arc<portage_vdb::SqliteDb> {
    let db = Arc::new(portage_vdb::SqliteDb::open(db_path).unwrap());
    portage_vdb::copy_all(&portage_vdb::FilesDb::new(root), db.as_ref(), false).unwrap();
    portage_vdb::register(root, db.clone());
    db
}

/// [`use_sqlite`] for any database backend: the same, with `kind`'s
/// file (`mrg --vdb-backend=<kind>`).
fn use_db(kind: portage_vdb::BackendKind, root: &Path, db_path: &Path) -> Arc<dyn InstalledDb> {
    let db: Arc<dyn InstalledDb> = match kind {
        #[cfg(feature = "vdb-sqlite")]
        portage_vdb::BackendKind::Sqlite => Arc::new(portage_vdb::SqliteDb::open(db_path).unwrap()),
        #[cfg(feature = "vdb-redb")]
        portage_vdb::BackendKind::Redb => Arc::new(portage_vdb::RedbDb::open(db_path).unwrap()),
        other => panic!("backend {other} is not built"),
    };
    portage_vdb::copy_all(&portage_vdb::FilesDb::new(root), db.as_ref(), false).unwrap();
    portage_vdb::register(root, db.clone());
    db
}

/// Every file of a live entry with its permission bits and bytes;
/// the volatile `#dir_mtime=` line of `metadata` is dropped (the
/// same thing `verify` ignores: directory mtimes). The group/other
/// write bits are masked: another test's in-process `brush` phase can
/// flip the process umask (`022`/`002`) between the two runs being
/// compared (#343).
fn entry_files(db: &dyn InstalledDb, key: &EntryKey) -> Tree {
    let image = db.entry_image(key).unwrap().expect("entry is installed");
    image
        .files
        .into_iter()
        .map(|f| {
            let data = if f.meta.name == "metadata" {
                String::from_utf8(f.data)
                    .unwrap()
                    .lines()
                    .filter(|l| !l.starts_with("#dir_mtime="))
                    .map(|l| format!("{l}\n"))
                    .collect::<String>()
                    .into_bytes()
            } else {
                f.data
            };
            (f.meta.name, (f.meta.mode & 0o7755, data))
        })
        .collect()
}

/// Everything under `root` except the stores a backend keeps
/// (`var/db/pkg`, `var/lib/portage`, `var/cache/edb`) and `etc`
/// (env-update's generated files, whose modes follow the process
/// umask; no fixture installs under `etc`): path ->
/// (mode, bytes or symlink target). Directories carry no bytes.
fn payload_tree(root: &Path) -> Tree {
    fn walk(root: &Path, dir: &Path, out: &mut Tree) {
        for entry in portage_util::read_dir_entries(dir).unwrap() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap().display().to_string();
            if ["var/db/pkg", "var/lib/portage", "var/cache/edb", "etc"].contains(&rel.as_str()) {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let data = if meta.file_type().is_symlink() {
                std::fs::read_link(&path)
                    .unwrap()
                    .display()
                    .to_string()
                    .into_bytes()
            } else if meta.is_file() {
                std::fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            out.insert(rel, (meta.mode(), data));
            if meta.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = Tree::new();
    walk(root, root, &mut out);
    // A directory left with nothing in the tree only held a store
    // (`var/db`, `var/cache` exist on `files` alone).
    // Deepest first, so a parent of a dropped directory goes too.
    let mut keys: Vec<String> = out.keys().cloned().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.matches('/').count()));
    for rel in keys {
        let is_dir = out[&rel].0 & 0o170000 == 0o040000;
        let prefix = format!("{rel}/");
        if is_dir && !out.keys().any(|k| k.starts_with(&prefix)) {
            out.remove(&rel);
        }
    }
    out
}

/// [`payload_tree`] without the permission bits: another test's
/// `umask` change (process-wide) can alter the modes of files a run
/// creates, which is not what the unmerge tests compare.
#[cfg(feature = "vdb-sqlite")]
fn payload_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    tree_bytes(payload_tree(root))
}

/// A [`payload_tree`] without the permission bits, for the merge tests
/// (ungated: [`payload_bytes`] is sqlite-gated but the binpkg/source
/// comparisons run on every backend).
fn tree_bytes(tree: Tree) -> BTreeMap<String, Vec<u8>> {
    tree.into_iter().map(|(k, (_, data))| (k, data)).collect()
}

fn install_hook(f: impl Fn(&Path) + 'static) {
    PUBLISH_HOOK.with(|hook| *hook.borrow_mut() = Some(Box::new(f)));
}

fn clear_hook() {
    PUBLISH_HOOK.with(|hook| *hook.borrow_mut() = None);
}

/// What one merge leaves: the entry, the payload, the config memory.
fn outcome(root: &Path, key: &EntryKey) -> (Tree, Tree, portage_vdb::ConfigMemory) {
    let db = portage_vdb::for_root(root);
    (
        entry_files(db.as_ref(), key),
        payload_tree(root),
        db.config_memory().unwrap(),
    )
}

/// (a) + (c): a binpkg merged on sqlite stores the same entry
/// files (bytes and modes; `metadata` without its directory-mtime
/// stamp) and lands the same payload as the same merge on `files`.
/// Both runs use the same ROOT and PORTAGE_TMPDIR paths, so the
/// `pkg_postinst` `PORTAGE_UPDATE_ENV` rewrite of `environment.bz2`
/// (which records paths) is byte-identical; on sqlite it went
/// through the scratch copy and `replace_file`, and the stored
/// bytes differ from the binpkg's (the pending entry's) own.
fn binpkg_merge_matches_files(kind: portage_vdb::BackendKind) {
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    let key = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
    let merge = || {
        std::fs::create_dir_all(&root).unwrap();
        let status = merge_binpkg(
            &binpkg("binpkgrmpkg-1.0.tbz2"),
            &root,
            &ptmp,
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);
    };

    merge();
    let on_files = outcome(&root, &key);
    std::fs::remove_dir_all(&root).unwrap();
    let _ = std::fs::remove_dir_all(&ptmp);

    std::fs::create_dir_all(&root).unwrap();
    let db = use_db(kind, &root, &tmp.join("vdb.db"));
    let pending_env: Rc<RefCell<Option<Vec<u8>>>> = Rc::default();
    {
        let db = db.clone();
        let pending_env = pending_env.clone();
        let key = key.clone();
        install_hook(move |_| {
            *pending_env.borrow_mut() = db.read_pending_file(&key, "environment.bz2").unwrap();
        });
    }
    merge();
    clear_hook();
    let on_sqlite = outcome(&root, &key);

    assert!(
        !root.join("var/db/pkg").exists(),
        "a {kind} merge writes no var/db/pkg"
    );
    assert_eq!(on_sqlite.0, on_files.0, "entry files differ");
    // Bytes only: the payload modes follow the process umask, which
    // another test's in-process `brush` phase can flip mid-run (#343).
    assert_eq!(
        tree_bytes(on_sqlite.1),
        tree_bytes(on_files.1),
        "payload differs"
    );
    assert_eq!(on_sqlite.2, on_files.2, "config memory differs");
    let pending_env = pending_env.borrow().clone().expect("pending env");
    assert_ne!(
        on_sqlite.0["environment.bz2"].1, pending_env,
        "pkg_postinst's PORTAGE_UPDATE_ENV rewrite reached the database"
    );
    assert!(db.pending_entries().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_binpkg_merged_on_sqlite_matches_the_files_merge() {
    binpkg_merge_matches_files(portage_vdb::BackendKind::Sqlite);
}

/// The same binpkg merge on redb.
#[cfg(feature = "vdb-redb")]
#[test]
fn a_binpkg_merged_on_redb_matches_the_files_merge() {
    binpkg_merge_matches_files(portage_vdb::BackendKind::Redb);
}

/// (b): a same-slot upgrade on sqlite, over a converted VDB. Right
/// before the publishing commit the old instance is still the
/// installed one and the new one is pending; after it, only the
/// new one is installed. The replaced version's `pkg_prerm` /
/// `pkg_postrm` ran from its stored environment (scratch copy).
#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_same_slot_upgrade_on_sqlite_replaces_in_the_publishing_commit() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    std::fs::create_dir_all(&root).unwrap();
    let old = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
    let new = EntryKey::new("dev-libs", "binpkgrmpkg-2.0");

    // 1.0 is installed on `files`, then the root is converted.
    merge_binpkg(
        &binpkg("binpkgrmpkg-1.0.tbz2"),
        &root,
        &ptmp,
        &MergeOptions::default(),
    )
    .expect("1.0 merges");
    let db = use_sqlite(&root, &tmp.join("vdb.sqlite"));
    assert!(db.has_entry(&old).unwrap());

    // (old live, new live, pending, generation) right before the commit.
    type Seen = (bool, bool, Vec<EntryKey>, u64);
    let seen: Rc<RefCell<Option<Seen>>> = Rc::default();
    {
        let db = db.clone();
        let seen = seen.clone();
        let (old, new) = (old.clone(), new.clone());
        install_hook(move |_| {
            *seen.borrow_mut() = Some((
                db.has_entry(&old).unwrap(),
                db.has_entry(&new).unwrap(),
                db.pending_entries().unwrap(),
                db.generation().unwrap(),
            ));
        });
    }
    let status = merge_binpkg(
        &binpkg("binpkgrmpkg-2.0.tbz2"),
        &root,
        &ptmp,
        &MergeOptions::default(),
    )
    .expect("2.0 merges");
    clear_hook();
    assert_eq!(status, 0);

    let (old_live, new_live, pending, generation) =
        seen.borrow().clone().expect("the publish hook ran");
    assert!(old_live, "the old instance is installed until the commit");
    assert!(!new_live, "the new instance is not live before the commit");
    assert_eq!(pending, vec![new.clone()]);

    assert!(!db.has_entry(&old).unwrap());
    assert!(db.has_entry(&new).unwrap());
    assert!(db.pending_entries().unwrap().is_empty());
    assert!(db.generation().unwrap() > generation);
    assert!(!root.join("var/db/pkg/dev-libs/binpkgrmpkg-2.0").exists());
    assert!(root.join("usr/share/binpkgrmpkg/payload-2.0.txt").is_file());
    assert!(!root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
        "setup-1.0\npreinst-1.0\npostinst-1.0\n\
         setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
    );
    // The counter ticked once on sqlite (1.0 took 0 on `files`).
    assert_eq!(
        db.read_file(&new, "COUNTER").unwrap().as_deref(),
        Some(&b"1"[..])
    );
    assert_eq!(db.counter().unwrap(), Some(portage_vdb::Counter(1)));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Clears the thread-local publish hook when dropped, so a hook that
/// panics (a simulated crash) cannot leak into later tests that reuse
/// this thread.
#[cfg(feature = "vdb-sqlite")]
struct HookGuard;
#[cfg(feature = "vdb-sqlite")]
impl Drop for HookGuard {
    fn drop(&mut self) {
        clear_hook();
    }
}

/// Run `f` (a merge or unmerge) with a hook that records the
/// generation and then panics, i.e. "the process dies right before
/// the final commit". Returns that generation; `f` must have panicked.
#[cfg(feature = "vdb-sqlite")]
fn crash_before_commit(db: &Arc<portage_vdb::SqliteDb>, f: impl FnOnce()) -> u64 {
    let at: Rc<RefCell<Option<u64>>> = Rc::default();
    let _guard = HookGuard;
    {
        let db = db.clone();
        let at = at.clone();
        install_hook(move |_| {
            *at.borrow_mut() = Some(db.generation().unwrap());
            panic!("simulated crash before the final commit");
        });
    }
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    assert!(res.is_err(), "the hook crashed the operation");
    let generation = *at.borrow();
    generation.expect("the hook ran")
}

#[cfg(feature = "vdb-sqlite")]
fn vdb_cli(args: &[&str]) -> (u8, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = crate::vdb_cmd::run_args(&args, &mut o, &mut e);
    (
        code,
        String::from_utf8_lossy(&o).into_owned() + &String::from_utf8_lossy(&e),
    )
}

/// A crash before the publishing commit leaves the old instance installed
/// plus a sweepable pending orphan, without advancing the generation.
#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_crash_before_the_publishing_commit_leaves_the_old_instance_and_a_sweepable_orphan() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    let dbp = tmp.join("vdb.sqlite");
    std::fs::create_dir_all(&root).unwrap();
    let old = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
    let new = EntryKey::new("dev-libs", "binpkgrmpkg-2.0");
    merge_binpkg(
        &binpkg("binpkgrmpkg-1.0.tbz2"),
        &root,
        &ptmp,
        &MergeOptions::default(),
    )
    .expect("1.0 merges");
    let db = use_sqlite(&root, &dbp);
    let old_files = entry_files(db.as_ref(), &old);

    let before_commit = crash_before_commit(&db, || {
        let _ = merge_binpkg(
            &binpkg("binpkgrmpkg-2.0.tbz2"),
            &root,
            &ptmp,
            &MergeOptions::default(),
        );
    });
    drop(db);

    // "Restart": a fresh handle on the same file.
    let fresh = Arc::new(portage_vdb::SqliteDb::open(&dbp).unwrap());
    portage_vdb::register(&root, fresh.clone());
    assert_eq!(fresh.entries().unwrap(), vec![old.clone()]);
    assert_eq!(entry_files(fresh.as_ref(), &old), old_files);
    assert!(!fresh.has_entry(&new).unwrap());
    assert_eq!(fresh.pending_entries().unwrap(), vec![new.clone()]);
    assert_eq!(
        fresh.generation().unwrap(),
        before_commit,
        "the aborted commit did not advance the generation"
    );
    let counter_after_crash = fresh.counter().unwrap().expect("a counter was taken");
    let warnings = crate::mrg::pending_entries_warnings(
        std::slice::from_ref(&new),
        &dbp,
        portage_vdb::BackendKind::Sqlite,
    );
    assert!(warnings[0].contains("binpkgrmpkg-2.0"), "{warnings:?}");
    assert!(
        crate::mrg::pending_entries_warnings(
            &fresh.pending_entries().unwrap(),
            &dbp,
            portage_vdb::BackendKind::Sqlite,
        )
        .iter()
        .any(|w| w.contains("dev-libs/binpkgrmpkg-2.0"))
    );

    let spec = format!("sqlite:{}", dbp.display());
    let (code, out) = vdb_cli(&["status", &spec]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("dev-libs/binpkgrmpkg-2.0"), "{out}");
    let (code, out) = vdb_cli(&["sweep", "--remove", "dev-libs/binpkgrmpkg-2.0", &spec]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = vdb_cli(&["status", &spec]);
    assert_eq!(code, 0, "{out}");
    assert!(fresh.pending_entries().unwrap().is_empty());
    assert!(fresh.has_entry(&old).unwrap());
    assert_eq!(entry_files(fresh.as_ref(), &old), old_files);

    // The upgrade can now be redone.
    let status = merge_binpkg(
        &binpkg("binpkgrmpkg-2.0.tbz2"),
        &root,
        &ptmp,
        &MergeOptions::default(),
    )
    .expect("2.0 merges after the sweep");
    assert_eq!(status, 0);
    assert!(fresh.has_entry(&new).unwrap());
    assert!(!fresh.has_entry(&old).unwrap());
    let counter_file = fresh.read_file(&new, "COUNTER").unwrap().unwrap();
    let counter: i64 = String::from_utf8(counter_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        counter > counter_after_crash.0,
        "{counter} reuses a counter taken before the crash ({counter_after_crash:?})"
    );
    assert!(fresh.generation().unwrap() > before_commit);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A crash before the retiring commit of `-C` leaves the row installed with
/// the generation unchanged; a later `-C` succeeds.
#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_crash_before_the_retire_commit_leaves_the_entry_installed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    let dbp = tmp.join("vdb.sqlite");
    std::fs::create_dir_all(&root).unwrap();
    let key = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
    merge_binpkg(
        &binpkg("binpkgrmpkg-1.0.tbz2"),
        &root,
        &ptmp,
        &MergeOptions::default(),
    )
    .expect("1.0 merges");
    let db = use_sqlite(&root, &dbp);
    let files = entry_files(db.as_ref(), &key);

    let before_commit = crash_before_commit(&db, || {
        unmerge_standalone(&root, &ptmp, "binpkgrmpkg-1.0", "binpkgrmpkg");
    });
    drop(db);

    let fresh = Arc::new(portage_vdb::SqliteDb::open(&dbp).unwrap());
    portage_vdb::register(&root, fresh.clone());
    assert!(fresh.has_entry(&key).unwrap());
    assert_eq!(entry_files(fresh.as_ref(), &key), files);
    assert!(fresh.pending_entries().unwrap().is_empty());
    assert_eq!(fresh.generation().unwrap(), before_commit);

    unmerge_standalone(&root, &ptmp, "binpkgrmpkg-1.0", "binpkgrmpkg");
    assert!(!fresh.has_entry(&key).unwrap());
    assert!(fresh.generation().unwrap() > before_commit);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A source merge (`ebuild <file> merge`) on sqlite: same entry
/// files and payload as on `files`, the `pkg_postinst` environment
/// rewrite included.
#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_source_merge_on_sqlite_matches_the_files_merge() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    let ebuild = fixtures_root().join("repo/dev-libs/mergepkg/mergepkg-1.0.ebuild");
    let key = EntryKey::new("dev-libs", "mergepkg-1.0");
    let merge = || {
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&ptmp).unwrap();
        let status = run_merge(&ebuild, &root, &ptmp, &MergeOptions::default(), None)
            .expect("run_merge succeeds");
        assert_eq!(status, 0);
    };

    merge();
    let on_files = outcome(&root, &key);
    std::fs::remove_dir_all(&root).unwrap();
    let _ = std::fs::remove_dir_all(&ptmp);

    std::fs::create_dir_all(&root).unwrap();
    use_sqlite(&root, &tmp.join("vdb.sqlite"));
    merge();
    let on_sqlite = outcome(&root, &key);

    assert!(!root.join("var/db/pkg").exists());
    assert_eq!(
        on_sqlite.0.keys().collect::<Vec<_>>(),
        on_files.0.keys().collect::<Vec<_>>()
    );
    for (name, (mode, data)) in &on_files.0 {
        // A source build records its own build time.
        if name == "BUILD_TIME" || name == "metadata" {
            continue;
        }
        // Masked: the process umask can differ between the two runs (#343).
        assert_eq!(on_sqlite.0[name].0 & 0o7755, mode & 0o7755, "{name} mode");
        // `CONTENTS` records each file's mtime, which a fresh
        // build moves: compare the lines without their mtime.
        let strip = |b: &[u8]| -> Vec<String> {
            String::from_utf8_lossy(b)
                .lines()
                .map(|l| match l.split_whitespace().next() {
                    Some("obj" | "sym") => l.rsplit_once(' ').map_or(l, |(a, _)| a),
                    _ => l,
                })
                .map(String::from)
                .collect()
        };
        if name == "CONTENTS" {
            assert_eq!(strip(&on_sqlite.0[name].1), strip(data), "CONTENTS lines");
            continue;
        }
        assert_eq!(&on_sqlite.0[name].1, data, "{name} bytes");
    }
    // Bytes only, as above: payload modes follow the process umask (#343).
    assert_eq!(
        tree_bytes(on_sqlite.1),
        tree_bytes(on_files.1),
        "payload differs"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
fn unmerge_standalone(root: &Path, ptmp: &Path, pf: &str, pn: &str) {
    unmerge_one_installed(UnmergeOneInstalledArgs {
        root,
        category: "dev-libs",
        package: pn,
        pf,
        also_keep: &[],
        scratch_dir: &ptmp.join("scratch"),
        portage_tmpdir: ptmp,
        options: &MergeOptions::default(),
        backup: None,
        is_replacement: false,
        replaced_by_version: None,
        replacement_preserved: &BTreeMap::new(),
        replacement_needed: &[],
        deferred_delete: None,
    })
    .expect("unmerge succeeds");
}

/// `mrg -C` on sqlite retires the row in one commit, matching the `files` run.
#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_standalone_unmerge_on_sqlite_retires_the_row_in_one_commit() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    let key = EntryKey::new("dev-libs", "binpkgrmpkg-1.0");
    let run = |sqlite: bool| {
        std::fs::create_dir_all(&root).unwrap();
        let db = sqlite.then(|| use_sqlite(&root, &tmp.join("vdb.sqlite")));
        merge_binpkg(
            &binpkg("binpkgrmpkg-1.0.tbz2"),
            &root,
            &ptmp,
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert!(portage_vdb::for_root(&root).has_entry(&key).unwrap());
        let seen: Rc<RefCell<Option<(bool, bool, u64)>>> = Rc::default();
        let before = db.as_ref().map(|db| db.generation().unwrap());
        if let Some(db) = &db {
            let db = db.clone();
            let seen = seen.clone();
            let key = key.clone();
            let root = root.clone();
            install_hook(move |_| {
                *seen.borrow_mut() = Some((
                    db.has_entry(&key).unwrap(),
                    root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists(),
                    db.generation().unwrap(),
                ));
            });
        }
        unmerge_standalone(&root, &ptmp, "binpkgrmpkg-1.0", "binpkgrmpkg");
        clear_hook();
        assert!(!portage_vdb::for_root(&root).has_entry(&key).unwrap());
        let log = std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap();
        let tree = payload_bytes(&root);
        if let Some(db) = &db {
            let (live, payload, generation) = seen.borrow().expect("retire hook ran");
            assert!(live, "the row is installed until the retire commit");
            assert!(!payload, "the payload was removed before it");
            assert_eq!(generation, before.unwrap());
            assert_eq!(db.generation().unwrap(), before.unwrap() + 1);
            assert!(!root.join("var/db/pkg").exists());
        }
        (log, tree)
    };
    let on_files = run(false);
    std::fs::remove_dir_all(&root).unwrap();
    let _ = std::fs::remove_dir_all(&ptmp);
    let on_sqlite = run(true);
    assert!(
        on_files.0.ends_with("prerm-1.0\npostrm-1.0\n"),
        "{}",
        on_files.0
    );
    assert_eq!(on_sqlite.0, on_files.0, "phase log differs");
    assert_eq!(on_sqlite.1, on_files.1, "payload differs");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Unmerging the last consumer prunes the preserved library in the same
/// retire commit; the end state equals the `files` run.
#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_standalone_unmerge_prunes_preserved_libs_in_its_retire_commit() {
    let tmp = tempdir();
    let ptmp = tmp.join("ptmp");
    let run = |name: &str, sqlite: bool| {
        let root = tmp.join(name);
        std::fs::create_dir_all(&root).unwrap();
        // `/usr/lib64` must be in the linker path for the consumer to be found.
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::write(
            root.join("etc/env.d/99sonamebump"),
            "LDPATH=\"/usr/lib64\"\n",
        )
        .unwrap();
        let db = sqlite.then(|| use_sqlite(&root, &tmp.join(format!("{name}.sqlite"))));
        for (pn, version) in [
            ("sonamebumplib", "1.0"),
            ("consumesonamebump", "1.0"),
            ("sonamebumplib", "2.0"),
        ] {
            let status = run_merge(
                &versioned_fixture(pn, version),
                &root,
                &ptmp,
                &MergeOptions::default(),
                None,
            )
            .expect("merge succeeds");
            assert_eq!(status, 0);
        }
        let lib = EntryKey::new("dev-libs", "sonamebumplib-2.0");
        let consumer = EntryKey::new("dev-libs", "consumesonamebump-1.0");
        let live = portage_vdb::for_root(&root);
        let registry_before = read_plib_registry(&root).preserved_libs();
        assert!(
            registry_before.contains_key("dev-libs/sonamebumplib-2.0"),
            "sanity: the old soname is preserved: {registry_before:?}"
        );
        let contents_before = live.read_file(&lib, "CONTENTS").unwrap().unwrap();

        type Seen = (Vec<u8>, BTreeMap<String, Vec<String>>);
        let seen: Rc<RefCell<Option<Seen>>> = Rc::default();
        let before = db.as_ref().map(|db| db.generation().unwrap());
        if let Some(db) = &db {
            let db = db.clone();
            let seen = seen.clone();
            let lib = lib.clone();
            install_hook(move |_| {
                *seen.borrow_mut() = Some((
                    db.read_file(&lib, "CONTENTS").unwrap().unwrap(),
                    // The stored registry (`read_plib_registry` would
                    // prune the paths the unmerge already deleted).
                    db.preserved_libs()
                        .unwrap()
                        .entries
                        .into_values()
                        .map(|e| (e.cpv, e.paths))
                        .collect(),
                ));
            });
        }
        unmerge_standalone(&root, &ptmp, "consumesonamebump-1.0", "consumesonamebump");
        clear_hook();

        assert!(!live.has_entry(&consumer).unwrap());
        assert!(read_plib_registry(&root).entries.is_empty());
        let contents_after = live.read_file(&lib, "CONTENTS").unwrap().unwrap();
        assert_ne!(contents_after, contents_before, "W4 rewrote CONTENTS");
        if let Some(db) = &db {
            let (contents_seen, registry_seen) = seen.borrow().clone().expect("hook ran");
            assert_eq!(contents_seen, contents_before, "W4 not visible before");
            assert_eq!(
                registry_seen, registry_before,
                "registry not visible before"
            );
            assert_eq!(db.generation().unwrap(), before.unwrap() + 1);
        }
        // Each run builds its own files: compare `CONTENTS` without mtimes.
        let contents_after: Vec<String> = String::from_utf8(contents_after)
            .unwrap()
            .lines()
            .map(|l| match l.split_whitespace().next() {
                Some("obj" | "sym") => l.rsplit_once(' ').map_or(l, |(a, _)| a).to_string(),
                _ => l.to_string(),
            })
            .collect();
        (
            contents_after,
            live.read_file(&lib, "NEEDED.ELF.2").unwrap(),
            payload_bytes(&root),
        )
    };
    let on_files = run("files-root", false);
    let on_sqlite = run("sqlite-root", true);
    assert_eq!(on_sqlite.0, on_files.0, "CONTENTS differs");
    assert_eq!(on_sqlite.1, on_files.1, "NEEDED.ELF.2 differs");
    let differing: Vec<&String> = on_files
        .2
        .keys()
        .chain(on_sqlite.2.keys())
        .filter(|k| on_files.2.get(*k) != on_sqlite.2.get(*k))
        .collect();
    assert!(differing.is_empty(), "payload differs at {differing:?}");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// What a VDB holds after a sequence, with the run-dependent values
/// taken out (see [`a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files`]).
#[derive(Debug, PartialEq)]
struct Snap {
    /// key -> (files: name -> (mode, bytes), stamp state).
    entries: BTreeMap<EntryKey, (Tree, portage_vdb::MetadataStamp)>,
    world: portage_vdb::World,
    world_sets: portage_vdb::WorldSets,
    preserved_libs: BTreeMap<String, portage_vdb::PreservedLibsEntry>,
    config_memory: portage_vdb::ConfigMemory,
    counter: Option<portage_vdb::Counter>,
}

/// `CONTENTS` / `metadata` / `BUILD_TIME` reduced to their stable
/// parts; every other file is returned as stored.
fn stable_bytes(name: &str, data: Vec<u8>) -> Vec<u8> {
    let lines = |data: &[u8], keep: &dyn Fn(&str) -> Option<String>| -> Vec<u8> {
        String::from_utf8_lossy(data)
            .lines()
            .filter_map(keep)
            .map(|l| format!("{l}\n"))
            .collect::<String>()
            .into_bytes()
    };
    match name {
        // The mtime is the last field of `obj` / `sym` lines.
        "CONTENTS" => lines(&data, &|l| {
            Some(match l.split_whitespace().next() {
                Some("obj" | "sym") => l.rsplit_once(' ').map_or(l, |(a, _)| a).to_string(),
                _ => l.to_string(),
            })
        }),
        // The stamp line and the BUILD_TIME field; the other
        // `metadata` lines are the stable fields.
        "metadata" => lines(&data, &|l| {
            (!l.starts_with("#dir_mtime=") && !l.starts_with("BUILD_TIME=")).then(|| l.to_string())
        }),
        _ => data,
    }
}

fn snapshot(db: &dyn InstalledDb) -> Snap {
    let mut entries = BTreeMap::new();
    for key in db.entries().unwrap() {
        let image = db.entry_image(&key).unwrap().expect("live entry");
        let files: Tree = image
            .files
            .into_iter()
            // A source build records its own build time.
            .filter(|f| f.meta.name != "BUILD_TIME")
            .map(|f| {
                let data = stable_bytes(&f.meta.name, f.data);
                // Group/other write bits depend on the process umask,
                // which another test may have changed.
                (f.meta.name, (f.meta.mode & 0o7755, data))
            })
            .collect();
        entries.insert(key, (files, image.metadata_stamp));
    }
    Snap {
        entries,
        world: db.world().unwrap(),
        world_sets: db.world_sets().unwrap(),
        preserved_libs: db.preserved_libs().unwrap().entries,
        config_memory: db.config_memory().unwrap(),
        counter: db.counter().unwrap(),
    }
}

/// The same merge/upgrade/unmerge sequence on a `files` ROOT and on a
/// database ROOT ends in the same installed state, modulo run-dependent
/// values (mtimes, `BUILD_TIME`, umask bits).
fn sequence_equivalence(kind: portage_vdb::BackendKind) {
    // `FEATURES`/`PORTAGE_CONFIGROOT`/`INSTALL_MASK` are process-global:
    // the merges below record the merge-time `FEATURES` into the vdb
    // `environment`, so no other test may mutate them mid-sequence. Shares
    // the lock the mutating tests hold (`PLACED_CONFIG_ENV_LOCK`); immune
    // to a poisoned mutex so one failed holder cannot cascade (#343).
    let _env = crate::remote::tests::PLACED_CONFIG_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = tempdir();
    let root = tmp.join("root");
    let ptmp = tmp.join("ptmp");
    let ebuild = fixtures_root().join("repo/dev-libs/mergepkg/mergepkg-1.0.ebuild");

    let sequence = || {
        for dir in [&root, &ptmp] {
            let _ = std::fs::remove_dir_all(dir);
            std::fs::create_dir_all(dir).unwrap();
        }
        // `/usr/lib64` must be in the linker path for the consumer to be found.
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::write(
            root.join("etc/env.d/99sonamebump"),
            "LDPATH=\"/usr/lib64\"\n",
        )
        .unwrap();
    };
    // Runs the steps on whatever backend `root` has.
    let steps = || {
        let opts = MergeOptions::default();
        let binpkg_1 = binpkg("binpkgrmpkg-1.0.tbz2");
        assert_eq!(merge_binpkg(&binpkg_1, &root, &ptmp, &opts).unwrap(), 0);
        assert_eq!(run_merge(&ebuild, &root, &ptmp, &opts, None).unwrap(), 0);
        assert_eq!(
            merge_binpkg(&binpkg("binpkgrmpkg-2.0.tbz2"), &root, &ptmp, &opts).unwrap(),
            0
        );
        for (pn, version) in [
            ("sonamebumplib", "1.0"),
            ("consumesonamebump", "1.0"),
            ("sonamebumplib", "2.0"),
        ] {
            let fixture = versioned_fixture(pn, version);
            assert_eq!(run_merge(&fixture, &root, &ptmp, &opts, None).unwrap(), 0);
        }
        let preserved = portage_vdb::for_root(&root).preserved_libs().unwrap();
        assert!(
            preserved.entries.contains_key("dev-libs/sonamebumplib:0"),
            "sanity: the old soname is preserved: {:?}",
            preserved.entries
        );
        unmerge_standalone(&root, &ptmp, "consumesonamebump-1.0", "consumesonamebump");
        assert!(
            portage_vdb::for_root(&root)
                .preserved_libs()
                .unwrap()
                .entries
                .is_empty()
        );
        unmerge_standalone(&root, &ptmp, "mergepkg-1.0", "mergepkg");
    };

    // The files run; then its ROOT moves aside (rename keeps mtimes).
    sequence();
    steps();
    let files_db = portage_vdb::FilesDb::new(&root);
    let files_snap = snapshot(&files_db);
    let files_payload = payload_tree(&root);
    let files_final = tmp.join("files-final");
    std::fs::rename(&root, &files_final).unwrap();

    // The sqlite run, in the same paths.
    sequence();
    let db = use_db(kind, &root, &tmp.join("vdb.db"));
    steps();
    let sqlite_snap = snapshot(db.as_ref());
    let sqlite_payload = payload_tree(&root);
    assert!(!root.join("var/db/pkg").exists());
    assert!(db.pending_entries().unwrap().is_empty());

    // Convert the sqlite result to a temporary files root.
    let converted = tmp.join("converted");
    std::fs::create_dir_all(&converted).unwrap();
    let converted_db = portage_vdb::FilesDb::new(&converted);
    portage_vdb::copy_all(db.as_ref(), &converted_db, false).unwrap();
    let converted_snap = snapshot(&converted_db);

    let live: Vec<String> = files_snap.entries.keys().map(|k| k.to_string()).collect();
    assert_eq!(
        live,
        ["dev-libs/binpkgrmpkg-2.0", "dev-libs/sonamebumplib-2.0"],
        "unexpected end state"
    );
    assert!(files_snap.counter.is_some());

    // Field by field, so a failure names what differs.
    let same = |got: &Snap, what: &str| {
        assert_eq!(
            got.entries.keys().collect::<Vec<_>>(),
            files_snap.entries.keys().collect::<Vec<_>>(),
            "{what}: live entries"
        );
        for (key, (files, stamp)) in &files_snap.entries {
            let (g_files, g_stamp) = &got.entries[key];
            assert_eq!(g_stamp, stamp, "{what}: {key} metadata stamp state");
            assert_eq!(
                g_files.keys().collect::<Vec<_>>(),
                files.keys().collect::<Vec<_>>(),
                "{what}: {key} file set"
            );
            for (name, (mode, data)) in files {
                let (g_mode, g_data) = &g_files[name];
                assert_eq!(g_mode, mode, "{what}: {key}/{name} mode");
                assert_eq!(
                    String::from_utf8_lossy(g_data),
                    String::from_utf8_lossy(data),
                    "{what}: {key}/{name} bytes"
                );
            }
        }
        assert_eq!(got.world, files_snap.world, "{what}: world");
        assert_eq!(got.world_sets, files_snap.world_sets, "{what}: world_sets");
        assert_eq!(
            got.preserved_libs, files_snap.preserved_libs,
            "{what}: plibs"
        );
        assert_eq!(
            got.config_memory, files_snap.config_memory,
            "{what}: config"
        );
        assert_eq!(got.counter, files_snap.counter, "{what}: counter");
    };
    same(&sqlite_snap, &format!("{kind} vs files"));
    same(&converted_snap, &format!("{kind} converted vs files"));
    let bytes = |tree: Tree| -> BTreeMap<String, Vec<u8>> {
        tree.into_iter().map(|(k, (_, data))| (k, data)).collect()
    };
    assert_eq!(bytes(sqlite_payload), bytes(files_payload), "payload tree");

    // Every difference `verify` reports must be a run-dependent one.
    let rep = portage_vdb::verify(&portage_vdb::FilesDb::new(&files_final), &converted_db).unwrap();
    assert_eq!(rep.entries_compared, 2);
    for line in &rep.differences {
        let (name, what) = line.split_once(": ").unwrap_or((line, ""));
        let file = name.rsplit_once('/').map_or("", |(_, f)| f);
        // Of a mode difference, only the umask bits may differ.
        let mode_ok = what
            .strip_prefix("mode ")
            .or_else(|| what.strip_prefix("directory mode "))
            .is_some_and(|m| {
                let v: Vec<u32> = m
                    .split(" != ")
                    .filter_map(|x| u32::from_str_radix(x, 8).ok())
                    .collect();
                v.len() == 2 && (v[0] ^ v[1]) & !0o022 == 0
            });
        let ok = what.starts_with("mtime")
            || what.starts_with("directory mtime")
            || mode_ok
            || (what.starts_with("bytes differ")
                && ["CONTENTS", "metadata", "BUILD_TIME"].contains(&file));
        assert!(ok, "verify reports a non-volatile difference: {line}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[cfg(feature = "vdb-sqlite")]
#[test]
fn a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files() {
    sequence_equivalence(portage_vdb::BackendKind::Sqlite);
}

/// The same sequence on redb (see [`sequence_equivalence`]).
#[cfg(feature = "vdb-redb")]
#[test]
fn a_merge_upgrade_unmerge_sequence_is_equivalent_on_redb_and_files() {
    sequence_equivalence(portage_vdb::BackendKind::Redb);
}

/// The database backends compiled in, as `BackendKind`s.
fn db_kinds() -> Vec<portage_vdb::BackendKind> {
    vec![
        #[cfg(feature = "vdb-sqlite")]
        portage_vdb::BackendKind::Sqlite,
        #[cfg(feature = "vdb-redb")]
        portage_vdb::BackendKind::Redb,
    ]
}

/// With two replaced same-slot instances, the later one's replace-loop prune
/// must not see the earlier one's `NEEDED.ELF.2`.
#[test]
fn a_later_replaced_instance_does_not_see_the_earlier_ones_needed_on_a_database() {
    let run = |kind: Option<portage_vdb::BackendKind>| {
        let tmp = tempdir();
        let (root, _) = seed_old_owned_consumer_root(&tmp);
        let twin = root.join("var/db/pkg/dev-libs/oldapp-0.9");
        std::fs::create_dir_all(&twin).unwrap();
        std::fs::write(twin.join("CONTENTS"), "obj /usr/bin/cprog2 abc 1\n").unwrap();
        // The replacing package ships `cprog2` too, so only the visibility
        // of the instances' `NEEDED.ELF.2` decides.
        std::fs::write(
            twin.join("NEEDED.ELF.2"),
            "X86_64;/usr/bin/cprog2;;;libkeep.so.1\n",
        )
        .unwrap();
        std::fs::write(root.join("usr/bin/cprog2"), b"fake elf").unwrap();
        for pf in ["oldapp-0.9", "oldapp-1.0"] {
            std::fs::write(
                root.join("var/db/pkg/dev-libs").join(pf).join("SLOT"),
                "0\n",
            )
            .unwrap();
        }
        let _db = kind.map(|k| use_db(k, &root, &tmp.join("vdb.db")));
        let new = EntryKey::new("dev-libs", "newapp-1.0");
        let handle = portage_vdb::for_root(&root);
        let mut txn = handle.begin_write().unwrap();
        txn.begin_entry(&new).unwrap();
        txn.put_entry_file(&new, "CONTENTS", b"obj /usr/bin/cprog2 abc 1\n")
            .unwrap();
        txn.commit().unwrap();
        let scratch = tmp.join("scratch");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let replaced = unmerge_replaced_same_slot(UnmergeReplacedSameSlotArgs {
            root: &root,
            category: "dev-libs",
            package: "oldapp",
            new_pf: "newapp-1.0",
            main_slot: "0",
            scratch_dir: &scratch,
            portage_tmpdir: &portage_tmpdir,
            options: &MergeOptions::default(),
            replacement_preserved: &BTreeMap::new(),
        })
        .expect("the replace loop succeeds");
        assert_eq!(replaced.len(), 2, "both instances are replaced");
        (
            root.join("usr/lib/keepA.so").exists(),
            read_plib_registry(&root)
                .entries
                .contains_key("dev-libs/provider:0"),
        )
    };
    let on_files = run(None);
    assert_eq!(
        on_files,
        (false, false),
        "files: the orphaned lib is pruned"
    );
    for kind in db_kinds() {
        assert_eq!(run(Some(kind)), on_files, "{kind}");
    }
}

/// The collision scenario on a database backend, with `find_owners`
/// answered from the `owner` index.
#[test]
fn collision_protect_names_the_owner_from_the_index_on_every_backend() {
    for kind in db_kinds() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let db = use_db(kind, &root, &tmp.join("vdb.db"));
        run_merge(
            &collision_fixture("collisionpkg-a"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("collisionpkg-a merges cleanly");
        assert_eq!(db.kind(), kind);
        let collisions = vec![
            "/usr/share/collisiontest/shared.txt".to_string(),
            "/usr/share/collisiontest/stray.txt".to_string(),
        ];
        assert_eq!(
            find_owners(&root, &collisions),
            BTreeMap::from([(
                "dev-libs/collisionpkg-a-1.0".to_string(),
                vec!["/usr/share/collisiontest/shared.txt".to_string()],
            )]),
            "{kind}"
        );
        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let err = run_merge(
            &collision_fixture("collisionpkg-c"),
            &root,
            &portage_tmpdir,
            &options,
            None,
        )
        .expect_err("collision-protect should abort the merge");
        assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{kind}: {err}");
        assert!(
            err.contains("/usr/share/collisiontest/shared.txt"),
            "{kind}: {err}"
        );
        assert!(
            err.contains("NOT merged due to file collisions"),
            "{kind}: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
            "hello from collisionpkg-a\n",
            "{kind}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// One hand-written entry: `(category, pf, files)`.
type HandEntry<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// A `files` VDB written by hand.
fn write_vdb(root: &Path, entries: &[HandEntry]) {
    for (cat, pf, files) in entries {
        let dir = root.join("var/db/pkg").join(cat).join(pf);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in *files {
            std::fs::write(dir.join(name), data).unwrap();
        }
    }
}

/// On a hand-built VDB carrying every dependency-token kind,
/// `installed_reverse_dependents` and `find_owners` agree on `files` and on
/// every database backend, with the right answer.
#[test]
fn reverse_dependents_and_owners_from_the_index_equal_the_scan() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let e =
        |pf: &'static str, files: &'static [(&'static str, &'static str)]| ("dev-libs", pf, files);
    write_vdb(
        &root,
        &[
            e(
                "consumer-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("CONTENTS", "dir /usr/lib\nobj /usr/lib/libc.so abc 1\n"),
                ],
            ),
            e(
                "cond-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("USE", "ssl\n"),
                    (
                        "RDEPEND",
                        "ssl? ( dev-libs/consumer ) !ssl? ( dev-libs/o )\n",
                    ),
                ],
            ),
            e(
                "condoff-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("USE", "x\n"),
                    ("RDEPEND", "ssl? ( dev-libs/consumer )\n"),
                ],
            ),
            e(
                "alt-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("RDEPEND", "|| ( dev-libs/zzz >=dev-libs/consumer-0.5 )\n"),
                ],
            ),
            e(
                "slotted-1.0",
                &[("SLOT", "0\n"), ("DEPEND", "dev-libs/consumer:0=[x(+)]\n")],
            ),
            e(
                "pdep-1.0",
                &[("SLOT", "0\n"), ("PDEPEND", "=dev-libs/consumer-1*\n")],
            ),
            e(
                "blocker-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("RDEPEND", "!dev-libs/consumer !!<dev-libs/consumer-3\n"),
                ],
            ),
            e(
                "toonew-1.0",
                &[("SLOT", "0\n"), ("RDEPEND", ">=dev-libs/consumer-2\n")],
            ),
            e("wild-1.0", &[("SLOT", "0\n"), ("RDEPEND", "dev-libs/*\n")]),
            e(
                "bare-1.0",
                &[("SLOT", "0\n"), ("RDEPEND", "dev-libs/consumer-1.0\n")],
            ),
            e(
                "junk-1.0",
                &[("SLOT", "0\n"), ("RDEPEND", "ssl? ( ( dev-libs/consumer\n")],
            ),
            e(
                "other-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("RDEPEND", "dev-libs/other dev-libs/consumer2\n"),
                ],
            ),
            e("nodeps-1.0", &[("SLOT", "0\n")]),
            e(
                "bidx-1.0",
                &[
                    ("SLOT", "0\n"),
                    ("BDEPEND", "dev-libs/consumer\n"),
                    ("CONTENTS", "obj usr/lib/libc.so d 1\nobj //x d 2\n"),
                ],
            ),
        ],
    );
    let consumers = [
        ("dev-libs", "consumer", "1.0"),
        ("dev-libs", "other", "1.0"),
    ];
    let want_rdeps = |c: (&str, &str, &str)| installed_reverse_dependents_for(&root, c.0, c.1, c.2);
    let scan: Vec<Vec<String>> = consumers.iter().map(|&c| want_rdeps(c)).collect();
    let wanted: Vec<String> = ["alt-1.0", "bidx-1.0", "cond-1.0", "pdep-1.0", "slotted-1.0"]
        .iter()
        .map(|pf| format!("dev-libs/{pf}"))
        .collect();
    for d in &wanted {
        assert!(scan[0].contains(d), "the scan misses {d}: {:?}", scan[0]);
    }
    for no in [
        "condoff-1.0",
        "blocker-1.0",
        "toonew-1.0",
        "other-1.0",
        "nodeps-1.0",
    ] {
        assert!(!scan[0].contains(&format!("dev-libs/{no}")), "{no}");
    }
    let paths: Vec<String> = ["/usr/lib", "/usr/lib/libc.so", "/x", "/nope"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let scan_owners = find_owners(&root, &paths);
    assert_eq!(scan_owners.len(), 2, "{scan_owners:?}");
    for (n, kind) in db_kinds().into_iter().enumerate() {
        // Another spelling of the same root per backend, so the
        // registry keeps `root` on `files`.
        let mut alias = root.clone();
        for _ in 0..=n {
            alias = alias.join("../root");
        }
        let db = use_db(kind, &alias, &tmp.join(format!("vdb{n}.db")));
        assert_eq!(db.kind(), kind);
        for (&c, want) in consumers.iter().zip(&scan) {
            assert_eq!(
                &installed_reverse_dependents_for(&alias, c.0, c.1, c.2),
                want,
                "{kind}: {c:?}"
            );
        }
        assert_eq!(find_owners(&alias, &paths), scan_owners, "{kind}");
        // The index is really in use: the entry without any dep token is skipped.
        let recs = db
            .reverse_dependents("dev-libs/consumer", &[portage_vdb::DepClass::Rdepend])
            .unwrap();
        assert!(!recs.iter().any(|r| r.key.pf == "nodeps-1.0"), "{kind}");
        assert!(recs.iter().any(|r| r.key.pf == "wild-1.0"), "{kind}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

fn installed_reverse_dependents_for(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<String> {
    portage_repo::installed_reverse_dependents(root, category, package, version)
}

/// Every installed fixture package gets the same reverse dependents from
/// the scan and from each database backend.
#[test]
fn fixture_reverse_dependents_are_identical_on_every_backend() {
    let fixtures = fixtures_root();
    let tmp = tempdir();
    let pkgs = portage_repo::all_installed_packages(&fixtures);
    assert!(pkgs.len() > 5);
    let want: Vec<Vec<String>> = pkgs
        .iter()
        .map(|p| {
            portage_repo::installed_reverse_dependents(
                &fixtures,
                &p.category,
                &p.package,
                &p.version,
            )
        })
        .collect();
    assert!(want.iter().any(|w| !w.is_empty()), "the fixture has edges");
    for (n, kind) in db_kinds().into_iter().enumerate() {
        let mut alias = fixtures.clone();
        for _ in 0..=n {
            alias = alias.join("../fixtures");
        }
        use_db(kind, &alias, &tmp.join(format!("vdb{n}.db")));
        for (p, w) in pkgs.iter().zip(&want) {
            assert_eq!(
                &portage_repo::installed_reverse_dependents(
                    &alias,
                    &p.category,
                    &p.package,
                    &p.version
                ),
                w,
                "{kind}: {}",
                p.cpv()
            );
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
