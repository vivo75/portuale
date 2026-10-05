// The `vdb` applet: convert and verify installed-package databases
// between backends.
//
//   portuale vdb convert --from KIND:PATH --to KIND:PATH [--force]
//   portuale vdb verify  KIND:PATH KIND:PATH
//   portuale vdb verify  --against KIND:PATH KIND:PATH
//
// The work is `portage_vdb::copy_all` / `portage_vdb::verify`; this file
// is argument parsing, backend opening and reporting only.
//
// `files:PATH` names a ROOT (its VDB directory plus the D4 stores
// `var/lib/portage/{world,world_sets,preserved_libs_registry}` and
// `var/cache/edb/counter` beneath it): a files destination needs those
// stores, and they live outside the VDB directory. Convenience for
// reading and writing alike: a PATH that is the VDB directory means the
// root three levels up (the host VDB is `files:/`). A source PATH
// that holds no VDB and is not one is read as a bare VDB
// directory (entries only, no D4 stores); a bare directory is refused as
// a destination.
//
//   portuale vdb mount [--foreground] [--allow-other] KIND:PATH MOUNTPOINT
//
// `mount` serves a read-only FUSE view of the database (the view is
// `vdb_view.rs`, the `fuser` adapter `vdb_fuse.rs`; feature `vdb-fuse`).
//
//   portuale vdb status  KIND:PATH
//   portuale vdb sweep   (--remove CAT/PF)... | --all  KIND:PATH
//
//   portuale vdb rebuild-index KIND:PATH
//
// `rebuild-index` recomputes a database's derived index (`owner`,
// `dep_atom`, `needed`) from the stored entry files in one transaction.
//
// `status` lists the entries left mid-merge: `merging`
// rows on a database, `-MERGING-<pf>` directories on files. `sweep` deletes
// the named ones in one write transaction; it refuses a key that is not
// pending, so an installed entry is never removed by it.
//
// Exit codes: 0 success / verify equal / status with nothing pending,
// 1 verify found differences / status found pending entries,
// 2 usage or I/O error.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portage_vdb::{BackendKind, EntryKey, FilesDb, InstalledDb};

const USAGE: &str = "\
Usage:
   portuale vdb convert --from KIND:PATH --to KIND:PATH [--force]
   portuale vdb verify [--against] KIND:PATH KIND:PATH
   portuale vdb status KIND:PATH
   portuale vdb sweep (--remove CAT/PF)... KIND:PATH
   portuale vdb sweep --all KIND:PATH
   portuale vdb rebuild-index KIND:PATH
   portuale vdb import-stores files:ROOT KIND:PATH
   portuale vdb mount [--foreground] [--allow-other] [--rw [--root ROOT]] KIND:PATH MOUNTPOINT
   portuale vdb --help

Copy the installed-package database (VDB) between backends, or compare two
backends. Counters are preserved, never renumbered.

KIND:PATH
   files:ROOT     the historic tree: the VDB directory under ROOT plus the world, world_sets, preserved-libs,
                  config-memory and counter stores under ROOT. A PATH that is
                  the VDB directory itself means the root three levels up
                  (files:/ for the host). As a source only, a directory
                  without a VDB under it is read as a bare VDB directory
                  (entries only, no stores).
   sqlite:FILE    one SQLite file (created by convert when missing)
   redb:FILE      one redb file (created by convert when missing). redb allows one
                  writer process at a time: a write (convert, sweep,
                  rebuild-index, a merging mrg) on a file held open by another process (a running mrg,
                  another vdb command, a FUSE mount) fails with the Busy
                  message (database is already open) (exit 2) instead of
                  waiting. Readers (verify, status, mrg --pretend, a FUSE
                  mount) share the file with each other, but also need it to
                  be free of a read-write holder.

convert options:
   --from KIND:PATH   the source (read only)
   --to KIND:PATH     the destination; must hold no entries unless --force
   --force            make the destination an exact copy: replace same-name
                      entries and delete the ones the source lacks
   Entries mid-merge (-MERGING-) are not copied; they are listed.

verify compares live entries (files, bytes, modes, mtimes, directory mode and
mtime, metadata-stamp state), world, world_sets, preserved libs, config memory
and the counter. Differences are listed, at most 100 lines.

status prints the backend, path, number of installed entries, generation,
counter, the import mark (a database converted from files) and the entries
left mid-merge by an interrupted merge. Exit 0 when none are pending, 1 when
some are.

sweep deletes pending entries (never installed ones) in one transaction:
   --remove CAT/PF    a pending entry to delete; repeatable
   --all              every pending entry
A key that is not pending refuses the whole sweep (nothing is deleted).

import-stores copies the world, world_sets, preserved-libs, config-memory and
counter files of files:ROOT into a sqlite or redb database in one transaction
(the counter becomes the larger of the two). A --rw mount does this itself
when it is unmounted; run it by hand if the mount process died.

rebuild-index recomputes the index a sqlite or redb database derives from the
stored entry files (file owners, dependency atoms, NEEDED.ELF.2 lines) for
every installed entry, in one transaction, and prints the row counts. It
repairs an index written wrong or incompletely by an older build. The files
and the counter are not touched; files:ROOT has no index (exit 2).

mount serves the database at MOUNTPOINT (an existing directory) as the
historic tree CAT/PF/files, through FUSE (fusermount3; no libfuse needed):
   --foreground, -f   stay in the foreground; SIGINT/SIGTERM unmount and exit
   --allow-other      let other users (root, running emerge) read the mount;
                      needs user_allow_other in /etc/fuse.conf unless root
   --rw               read-write, sqlite and redb only: real Portage can merge,
                      replace and unmerge through the mount; see below
   --root ROOT        with --rw: the ROOT whose world, world_sets,
                      preserved-libs, config-memory and counter files real
                      emerge writes (default /); imported at unmount
Without --foreground it detaches once the mount is ready. Unmount with
`fusermount3 -u MOUNTPOINT` (or umount as root). Without --rw the database is
opened read-only and every write is EROFS. Directory mtimes follow real
Portage (an entry shows its stored mtime, a category the latest of its
entries, the root the latest category) and the metadata file's stamp
matches the entry directory mtime shown. Each
directory listing and each open file serves the generation it was opened on.
Over redb the mount holds the file read-only: readers (mrg --pretend, verify,
status) run beside it, writers (a merging mrg, convert, sweep, rebuild-index)
get the Busy message until it is unmounted. sqlite can be mounted while mrg
merges. files:ROOT is a pass-through, for testing.

With --rw (mount it over real Portage's VDB directory) the writes real
Portage makes are stored: a new <cat>/-MERGING-<pf> directory is staged on
scratch disk ($TMPDIR/portuale-vdb-rw-*) and stored in one transaction when
it is renamed to <cat>/<pf> (its metadata stamp kept valid, the counter
raised to its COUNTER); a rewrite of a file in a live entry is stored at
close; an entry is deleted in one transaction when real removes its last
file and the directory. Lock files live in memory only. Any other write is
EPERM. Over redb the --rw mount holds the file read-write, so mrg and other
vdb commands get the Busy message until it is unmounted.

Exit status: 0 done / equal / nothing pending, 1 verify found differences or
status found pending entries, 2 usage or I/O error.";

struct Spec {
    kind: BackendKind,
    path: PathBuf,
}

fn parse_spec(s: &str) -> Result<Spec, String> {
    let (kind, path) = s
        .split_once(':')
        .ok_or_else(|| format!("{s:?}: expected KIND:PATH (files:ROOT, sqlite:FILE, redb:FILE)"))?;
    if path.is_empty() {
        return Err(format!("{s:?}: empty path"));
    }
    let kind = kind.parse::<BackendKind>().map_err(|e| e.to_string())?;
    Ok(Spec {
        kind,
        path: PathBuf::from(path),
    })
}

fn open(spec: &Spec, write: bool) -> Result<Box<dyn InstalledDb>, String> {
    match spec.kind {
        BackendKind::Files => match FilesDb::from_cli_path(&spec.path) {
            Some(db) if !write || db.root().is_some() => Ok(Box::new(db)),
            // A destination that does not exist, or is an empty directory,
            // becomes a new ROOT; a non-empty bare VDB directory is refused.
            _ if write => {
                let busy = std::fs::read_dir(&spec.path)
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false);
                if busy {
                    return Err(format!(
                        "{}: a files destination is a ROOT (it gets a VDB and the stores \
                         beneath it); this directory is not empty and holds no VDB",
                        spec.path.display()
                    ));
                }
                Ok(Box::new(FilesDb::new(&spec.path)))
            }
            _ => Err(format!("{}: not a directory", spec.path.display())),
        },
        BackendKind::Sqlite => open_sqlite(&spec.path, write),
        BackendKind::Redb => open_redb(&spec.path, write),
    }
}

#[cfg(feature = "vdb-redb")]
fn open_redb(path: &Path, write: bool) -> Result<Box<dyn InstalledDb>, String> {
    let db = if write {
        portage_vdb::RedbDb::open(path)
    } else {
        portage_vdb::RedbDb::open_readonly(path)
    };
    // `Error::Busy` displays the file and the one-process rule.
    db.map(|d| Box::new(d) as Box<dyn InstalledDb>)
        .map_err(|e| e.to_string())
}

#[cfg(not(feature = "vdb-redb"))]
fn open_redb(_path: &Path, _write: bool) -> Result<Box<dyn InstalledDb>, String> {
    Err("redb: this portuale was built without the vdb-redb feature".into())
}

#[cfg(feature = "vdb-sqlite")]
fn open_sqlite(path: &Path, write: bool) -> Result<Box<dyn InstalledDb>, String> {
    let db = if write {
        portage_vdb::SqliteDb::open(path)
    } else {
        portage_vdb::SqliteDb::open_readonly(path)
    };
    db.map(|d| Box::new(d) as Box<dyn InstalledDb>)
        .map_err(|e| e.to_string())
}

#[cfg(not(feature = "vdb-sqlite"))]
fn open_sqlite(_path: &Path, _write: bool) -> Result<Box<dyn InstalledDb>, String> {
    Err("sqlite: this portuale was built without the vdb-sqlite feature".into())
}

fn value_of(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    if let Some((_, v)) = args[*i].split_once('=') {
        return Ok(v.to_string());
    }
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a KIND:PATH argument"))
}

fn convert(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let (mut from, mut to, mut force) = (None, None, false);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-h" || a == "--help" {
            let _ = writeln!(out, "{USAGE}");
            return Ok(0);
        } else if a == "--from" || a.starts_with("--from=") {
            from = Some(parse_spec(&value_of(args, &mut i, "--from")?)?);
        } else if a == "--to" || a.starts_with("--to=") {
            to = Some(parse_spec(&value_of(args, &mut i, "--to")?)?);
        } else if a == "--force" {
            force = true;
        } else {
            return Err(format!("convert: unexpected argument {a:?}"));
        }
        i += 1;
    }
    let from = from.ok_or("convert: --from KIND:PATH is required")?;
    let to = to.ok_or("convert: --to KIND:PATH is required")?;
    let src = open(&from, false)?;
    let dst = open(&to, true)?;
    let rep = portage_vdb::copy_all(&*src, &*dst, force).map_err(|e| e.to_string())?;
    let _ = writeln!(
        out,
        "converted {} entries ({} -> {}); world {} atoms, {} sets; preserved libs {}; \
         config memory {}; counter {}",
        rep.copied.len(),
        from.kind,
        to.kind,
        rep.world_atoms,
        rep.world_sets,
        rep.preserved_libs,
        rep.config_memory,
        rep.counter.map_or("none".to_string(), |c| c.to_string()),
    );
    for k in &rep.removed {
        let _ = writeln!(out, "removed from destination (not in source): {k}");
    }
    for k in &rep.pending_skipped {
        let _ = writeln!(
            err,
            "portuale vdb: pending entry not copied: {k} (-MERGING-)"
        );
    }
    Ok(0)
}

fn verify(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    let mut specs = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                let _ = writeln!(out, "{USAGE}");
                return Ok(0);
            }
            // The design's spelling: `verify --against A B`.
            "--against" => {}
            s if s.starts_with("--against=") => specs.push(parse_spec(&s["--against=".len()..])?),
            s if s.starts_with('-') && !s.contains(':') => {
                return Err(format!("verify: unknown option {s:?}"));
            }
            s => specs.push(parse_spec(s)?),
        }
    }
    let [a, b] = specs.as_slice() else {
        return Err("verify: expected exactly two KIND:PATH arguments".into());
    };
    let (da, db) = (open(a, false)?, open(b, false)?);
    let rep = portage_vdb::verify(&*da, &*db).map_err(|e| e.to_string())?;
    if rep.is_equal() {
        let _ = writeln!(out, "equal: {} entries compared", rep.entries_compared);
        return Ok(0);
    }
    let _ = writeln!(out, "DIFFERENT: {} difference(s)", rep.total_differences);
    for d in &rep.differences {
        let _ = writeln!(out, "  {d}");
    }
    if rep.total_differences > rep.differences.len() {
        let _ = writeln!(
            out,
            "  ... {} more not shown",
            rep.total_differences - rep.differences.len()
        );
    }
    Ok(1)
}

/// Open an existing database for `status` / `sweep`; never creates one.
fn open_existing(spec: &Spec, write: bool) -> Result<Box<dyn InstalledDb>, String> {
    if spec.kind != BackendKind::Files && !spec.path.exists() {
        return Err(format!("{}: no such database", spec.path.display()));
    }
    open(spec, write)
}

/// The scratch directory of a `--rw` mount of `db`: named after the
/// database file, so the next mount of the same database clears what a
/// daemon that died left behind (`RwView::new` empties it).
#[cfg(feature = "vdb-fuse")]
fn rw_scratch(db: &Path) -> PathBuf {
    use std::hash::{Hash as _, Hasher as _};
    let abs = std::fs::canonicalize(db).unwrap_or_else(|_| db.to_path_buf());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    abs.hash(&mut h);
    std::env::temp_dir().join(format!("portuale-vdb-rw-{:016x}", h.finish()))
}

#[cfg(not(feature = "vdb-fuse"))]
fn mount_spec(
    _spec: Spec,
    _mountpoint: PathBuf,
    _foreground: bool,
    _allow_other: bool,
    _rw: Option<PathBuf>,
) -> Result<u8, String> {
    Err("mount: this portuale was built without the vdb-fuse feature".into())
}

fn one_spec(cmd: &str, specs: Vec<Spec>) -> Result<Spec, String> {
    let mut it = specs.into_iter();
    match (it.next(), it.next()) {
        (Some(s), None) => Ok(s),
        _ => Err(format!("{cmd}: expected exactly one KIND:PATH argument")),
    }
}

fn status(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    let mut specs = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                let _ = writeln!(out, "{USAGE}");
                return Ok(0);
            }
            s if s.starts_with('-') && !s.contains(':') => {
                return Err(format!("status: unknown option {s:?}"));
            }
            s => specs.push(parse_spec(s)?),
        }
    }
    let spec = one_spec("status", specs)?;
    let db = open_existing(&spec, false)?;
    let e = |e: portage_vdb::Error| e.to_string();
    let installed = db.entries().map_err(e)?.len();
    let _ = writeln!(out, "backend:    {}", spec.kind);
    let _ = writeln!(out, "path:       {}", spec.path.display());
    let _ = writeln!(out, "installed:  {installed}");
    let _ = writeln!(out, "generation: {}", db.generation().map_err(e)?);
    let _ = writeln!(
        out,
        "counter:    {}",
        db.counter()
            .map_err(e)?
            .map_or("none".to_string(), |c| c.to_string())
    );
    if let Some((generation, source)) = db.import_mark().map_err(e)? {
        let _ = writeln!(
            out,
            "imported:   from files:{source} at generation {generation}"
        );
    }
    let pending = db.pending_entries().map_err(e)?;
    if pending.is_empty() {
        let _ = writeln!(out, "pending:    none");
        return Ok(0);
    }
    let _ = writeln!(out, "pending:    {} (interrupted merges)", pending.len());
    for k in &pending {
        let _ = writeln!(out, "  {k}");
    }
    let _ = writeln!(
        out,
        "remove with: portuale vdb sweep --remove CAT/PF {}:{}",
        spec.kind,
        spec.path.display()
    );
    Ok(1)
}

fn sweep(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    let (mut remove, mut all, mut specs) = (Vec::<EntryKey>::new(), false, Vec::new());
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-h" || a == "--help" {
            let _ = writeln!(out, "{USAGE}");
            return Ok(0);
        } else if a == "--all" {
            all = true;
        } else if a == "--remove" || a.starts_with("--remove=") {
            let v = match a.split_once('=') {
                Some((_, v)) => v.to_string(),
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or("--remove needs a CAT/PF argument")?
                }
            };
            let (cat, pf) = v
                .split_once('/')
                .filter(|(c, p)| !c.is_empty() && !p.is_empty() && !p.contains('/'))
                .ok_or_else(|| format!("{v:?}: expected CAT/PF"))?;
            remove.push(EntryKey::new(cat, pf));
        } else if a.starts_with('-') && !a.contains(':') {
            return Err(format!("sweep: unknown option {a:?}"));
        } else {
            specs.push(parse_spec(a)?);
        }
        i += 1;
    }
    if all == !remove.is_empty() {
        return Err("sweep: give --all or at least one --remove CAT/PF (not both)".into());
    }
    let spec = one_spec("sweep", specs)?;
    let db = open_existing(&spec, true)?;
    let pending = db.pending_entries().map_err(|e| e.to_string())?;
    if all {
        remove = pending.clone();
    }
    // Refuse the whole sweep before deleting anything.
    for k in &remove {
        if !pending.contains(k) {
            let why = if db.has_entry(k).map_err(|e| e.to_string())? {
                "an installed entry, not an interrupted merge"
            } else {
                "not pending"
            };
            return Err(format!("sweep: {k} is {why}; nothing was removed"));
        }
    }
    remove.sort();
    remove.dedup();
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    for k in &remove {
        txn.discard_pending(k).map_err(|e| e.to_string())?;
    }
    txn.commit().map_err(|e| e.to_string())?;
    for k in &remove {
        let _ = writeln!(out, "removed pending entry {k}");
    }
    if remove.is_empty() {
        let _ = writeln!(out, "nothing pending");
    }
    Ok(0)
}

/// What [`import_stores`] copied.
pub(crate) struct StoresReport {
    pub world: usize,
    pub world_sets: usize,
    pub preserved_libs: usize,
    pub config_memory: usize,
    pub counter: Option<i64>,
}

impl std::fmt::Display for StoresReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "world {} atoms, {} sets; preserved libs {}; config memory {}; counter {}",
            self.world,
            self.world_sets,
            self.preserved_libs,
            self.config_memory,
            self.counter.map_or("none".to_string(), |c| c.to_string())
        )
    }
}

/// Copy the stores Portage's emerge keeps outside the VDB tree (world,
/// world_sets, the preserved-libs registry, config memory, the counter)
/// from the files tree under `root` into `db`, in one transaction. The
/// counter becomes the larger of the two, so it never goes back.
pub(crate) fn import_stores(
    root: &Path,
    db: &dyn InstalledDb,
) -> portage_vdb::Result<StoresReport> {
    let src = FilesDb::new(root);
    let world = src.world()?;
    let sets = src.world_sets()?;
    let libs = src.preserved_libs()?;
    let memory = src.config_memory()?;
    let counter = match (src.counter()?, db.counter()?) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let mut txn = db.begin_write()?;
    txn.set_world(&world)?;
    txn.set_world_sets(&sets)?;
    txn.set_preserved_libs(&libs)?;
    txn.set_config_memory(&memory)?;
    if let Some(c) = counter {
        txn.set_counter(c)?;
    }
    txn.commit()?;
    Ok(StoresReport {
        world: world.atoms.len(),
        world_sets: sets.sets.len(),
        preserved_libs: libs.entries.len(),
        config_memory: memory.entries.len(),
        counter: counter.map(|c| c.0),
    })
}

/// `portuale vdb import-stores files:ROOT KIND:PATH`.
fn import_stores_cmd(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    let mut specs = Vec::new();
    for a in args {
        if a == "-h" || a == "--help" {
            let _ = writeln!(out, "{USAGE}");
            return Ok(0);
        } else if a.starts_with('-') && !a.contains(':') {
            return Err(format!("import-stores: unknown option {a:?}"));
        }
        specs.push(parse_spec(a)?);
    }
    let [from, to] = <[Spec; 2]>::try_from(specs)
        .map_err(|_| "import-stores: expected files:ROOT and KIND:PATH".to_string())?;
    if from.kind != BackendKind::Files {
        return Err("import-stores: the source must be files:ROOT".into());
    }
    if to.kind == BackendKind::Files {
        return Err("import-stores: the destination must be a sqlite or redb database".into());
    }
    let db = open_existing(&to, true)?;
    let rep = import_stores(&from.path, &*db).map_err(|e| e.to_string())?;
    let _ = writeln!(out, "imported {rep}");
    Ok(0)
}

/// `portuale vdb rebuild-index KIND:PATH`.
fn rebuild_index(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    let mut specs = Vec::new();
    for a in args {
        if a == "-h" || a == "--help" {
            let _ = writeln!(out, "{USAGE}");
            return Ok(0);
        } else if a.starts_with('-') && !a.contains(':') {
            return Err(format!("rebuild-index: unknown option {a:?}"));
        }
        specs.push(parse_spec(a)?);
    }
    let spec = one_spec("rebuild-index", specs)?;
    if spec.kind == BackendKind::Files {
        return Err(
            "rebuild-index: files:ROOT has no derived index (it is the files themselves)".into(),
        );
    }
    let db = open_existing(&spec, true)?;
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    let n = txn.rebuild_index().map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())?;
    let _ = writeln!(
        out,
        "rebuilt the index of {} installed entries: owner {} rows, dep_atom {}, needed {}",
        n.entries, n.owner, n.dep_atom, n.needed
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/var")
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("portuale-vdbcmd-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// `cp -a` (modes and mtimes kept, symlinks kept).
    fn copy_tree(from: &Path, to: &Path) {
        let st = Command2::run(&["cp", "-a", &from.to_string_lossy(), &to.to_string_lossy()]);
        assert!(st, "cp -a failed");
    }

    struct Command2;
    impl Command2 {
        fn run(argv: &[&str]) -> bool {
            std::process::Command::new(argv[0])
                .args(&argv[1..])
                .status()
                .unwrap()
                .success()
        }
    }

    /// A temp ROOT holding a copy of the fixture var tree.
    fn fixture_root(tag: &str) -> PathBuf {
        let root = scratch(tag);
        copy_tree(&fixtures(), &root.join("var"));
        root
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn files_path_forms() {
        let root = fixture_root("paths");
        let root_of =
            |p: &Path| FilesDb::from_cli_path(p).and_then(|d| d.root().map(Path::to_path_buf));
        assert_eq!(
            root_of(&root.join("var/db/pkg")).as_deref(),
            Some(root.as_path())
        );
        assert_eq!(root_of(&root).as_deref(), Some(root.as_path()));
        // A plain directory is a bare VDB directory (no root); a missing path is none.
        assert!(FilesDb::from_cli_path(&root.join("var")).is_some_and(|d| d.root().is_none()));
        assert!(FilesDb::from_cli_path(&root.join("nope")).is_none());
        assert_eq!(
            root_of(Path::new("var/db/pkg")).as_deref(),
            Some(Path::new("."))
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Leave `cat/pf` pending (begin_entry, a file, no finish) through the
    /// backend's own write path, as a merge killed between its commits does.
    fn leave_pending(spec: &str, cat: &str, pf: &str) {
        let db = open(&parse_spec(spec).unwrap(), true).unwrap();
        let k = EntryKey::new(cat, pf);
        let mut txn = db.begin_write().unwrap();
        txn.begin_entry(&k).unwrap();
        txn.put_entry_file(&k, "CONTENTS", b"half\n").unwrap();
        txn.commit().unwrap();
    }
}
