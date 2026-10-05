// The `vdb` applet (feat#157 / backlog #305, plan S2.7): convert and verify
// installed-package databases between backends.
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
// `mount` serves a read-only FUSE view of the database (S7; the view is
// `vdb_view.rs`, the `fuser` adapter `vdb_fuse.rs`; feature `vdb-fuse`).
//
//   portuale vdb status  KIND:PATH
//   portuale vdb sweep   (--remove CAT/PF)... | --all  KIND:PATH
//
// `status` lists the entries left mid-merge (S4.3, design §9.1): `merging`
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
   portuale vdb mount [--foreground] [--allow-other] KIND:PATH MOUNTPOINT
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
                  process at a time: a file held open by another process (a
                  running mrg, another vdb command, a FUSE mount) fails with the
                  Busy message (database is already open) (exit 2) instead of
                  waiting. Read-only commands (verify, status) also need the
                  file to be free of a read-write holder.

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

mount serves the database read-only at MOUNTPOINT (an existing directory) as
the historic tree CAT/PF/files, through FUSE (fusermount3; no libfuse needed):
   --foreground, -f   stay in the foreground; SIGINT/SIGTERM unmount and exit
   --allow-other      let other users (root, running emerge) read the mount;
                      needs user_allow_other in /etc/fuse.conf unless root
Without --foreground it detaches once the mount is ready. Unmount with
`fusermount3 -u MOUNTPOINT`. The database is opened read-only. Every write is
EROFS. Directory mtimes follow real Portage (an entry shows its stored
mtime, a category the latest of its entries, the root the latest category)
and the metadata file's stamp matches the entry directory mtime shown. Each
directory listing and each open file serves the generation it was opened on.
A redb file is held by the mount (one process at a time): over redb the mount
is an offline view and mrg cannot run until it is unmounted. sqlite can be
mounted while mrg runs. files:ROOT is a pass-through, for testing.

Exit status: 0 done / equal / nothing pending, 1 verify found differences or
status found pending entries, 2 usage or I/O error.";

/// Entry point: `args` are the arguments after `vdb`.
pub fn run(args: &[String]) -> ExitCode {
    let code = run_args(args, &mut std::io::stdout(), &mut std::io::stderr());
    ExitCode::from(code)
}

/// `run` with injectable output, returning the exit status.
pub(crate) fn run_args(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match dispatch(args, out, err) {
        Ok(code) => code,
        Err(msg) => {
            let _ = writeln!(err, "portuale vdb: {msg}");
            2
        }
    }
}

fn dispatch(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, String> {
    let Some(sub) = args.first().map(String::as_str) else {
        let _ = writeln!(err, "{USAGE}");
        return Ok(2);
    };
    match sub {
        "-h" | "--help" | "help" => {
            let _ = writeln!(out, "{USAGE}");
            Ok(0)
        }
        "convert" => convert(&args[1..], out, err),
        "verify" => verify(&args[1..], out),
        "status" => status(&args[1..], out),
        "sweep" => sweep(&args[1..], out),
        "mount" => mount(&args[1..], out),
        other => Err(format!(
            "unknown subcommand {other:?} (expected convert, verify, status, sweep or mount); see `portuale vdb --help`"
        )),
    }
}

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

/// `portuale vdb mount [--foreground] [--allow-other] KIND:PATH MOUNTPOINT`.
fn mount(args: &[String], out: &mut dyn Write) -> Result<u8, String> {
    let mut foreground = false;
    let mut allow_other = false;
    let mut pos: Vec<&str> = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                let _ = writeln!(out, "{USAGE}");
                return Ok(0);
            }
            "-f" | "--foreground" => foreground = true,
            "--allow-other" => allow_other = true,
            s if s.starts_with('-') => return Err(format!("mount: unknown option {s:?}")),
            s => pos.push(s),
        }
    }
    let [spec, mountpoint] = pos[..] else {
        return Err("mount: expected KIND:PATH and MOUNTPOINT".into());
    };
    let spec = parse_spec(spec)?;
    let mountpoint = PathBuf::from(mountpoint);
    if !mountpoint.is_dir() {
        return Err(format!(
            "{}: the mountpoint must be an existing directory",
            mountpoint.display()
        ));
    }
    mount_spec(spec, mountpoint, foreground, allow_other)
}

#[cfg(feature = "vdb-fuse")]
fn mount_spec(
    spec: Spec,
    mountpoint: PathBuf,
    foreground: bool,
    allow_other: bool,
) -> Result<u8, String> {
    use std::sync::Arc;
    let fsname = format!("{}:{}", spec.kind, spec.path.display());
    let open_ro =
        || -> Result<Arc<dyn InstalledDb>, String> { open_existing(&spec, false).map(Arc::from) };
    // Open once here so a bad PATH, a busy redb or a missing feature is
    // reported before anything forks; the handle is dropped again, the
    // daemon opens its own.
    drop(open_ro()?);
    if foreground {
        crate::vdb_fuse::serve(open_ro()?, &mountpoint, &fsname, allow_other, || {})
            .map_err(|e| format!("{}: {e}", mountpoint.display()))?;
    } else {
        crate::vdb_fuse::serve_background(open_ro, &mountpoint, &fsname, allow_other)
            .map_err(|e| format!("{}: {e}", mountpoint.display()))?;
    }
    Ok(0)
}

#[cfg(not(feature = "vdb-fuse"))]
fn mount_spec(
    _spec: Spec,
    _mountpoint: PathBuf,
    _foreground: bool,
    _allow_other: bool,
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

    fn cli(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = run_args(&args, &mut o, &mut e);
        (
            code,
            String::from_utf8_lossy(&o).into_owned(),
            String::from_utf8_lossy(&e).into_owned(),
        )
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[cfg(feature = "vdb-sqlite")]
    #[test]
    fn round_trip_files_sqlite_files_verifies_equal_and_an_alteration_is_named() {
        let root = fixture_root("rt");
        let db = root.join("vdb.sqlite");
        let back = root.join("back");
        let (c, o, e) = cli(&[
            "convert",
            "--from",
            &format!("files:{}", s(&root)),
            "--to",
            &format!("sqlite:{}", s(&db)),
        ]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(o.contains("converted "), "{o}");
        let (c, o, e) = cli(&[
            "convert",
            &format!("--from=sqlite:{}", s(&db)),
            &format!("--to=files:{}", s(&back)),
        ]);
        assert_eq!(c, 0, "{o}{e}");
        // files:<vdb dir> normalises to its root, so the stores are compared too.
        let (c, o, e) = cli(&[
            "verify",
            "--against",
            &format!("files:{}", s(&root.join("var/db/pkg"))),
            &format!("sqlite:{}", s(&db)),
        ]);
        assert_eq!(c, 0, "{o}{e}");
        let (c, o, e) = cli(&[
            "verify",
            &format!("files:{}", s(&root)),
            &format!("files:{}", s(&back)),
        ]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(o.starts_with("equal: "), "{o}");

        // Alter one byte of one entry file in the round-tripped copy.
        let victim = fs::read_dir(back.join("var/db/pkg/dev-libs"))
            .unwrap()
            .map(|d| d.unwrap().path())
            .find(|p| p.join("SLOT").is_file())
            .unwrap();
        let slot = victim.join("SLOT");
        let mtime = fs::metadata(&slot).unwrap().modified().unwrap();
        fs::write(&slot, "9\n").unwrap();
        fs::File::options()
            .write(true)
            .open(&slot)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        let (c, o, _) = cli(&[
            "verify",
            &format!("files:{}", s(&root)),
            &format!("files:{}", s(&back)),
        ]);
        assert_eq!(c, 1, "{o}");
        let name = victim.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            o.contains(&format!("dev-libs/{name}/SLOT: bytes differ")),
            "{o}"
        );
        // The altered files copy also differs from the sqlite file.
        let (c, _, _) = cli(&[
            "verify",
            &format!("files:{}", s(&back)),
            &format!("sqlite:{}", s(&db)),
        ]);
        assert_eq!(c, 1);
        // A changed world is named too.
        fs::write(back.join("var/lib/portage/world"), "x/y\n").unwrap();
        let (c, o, _) = cli(&[
            "verify",
            &format!("files:{}", s(&root)),
            &format!("files:{}", s(&back)),
        ]);
        assert_eq!(c, 1);
        assert!(o.contains("world: "), "{o}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_non_empty_destination_is_refused_unless_forced() {
        let root = fixture_root("ne");
        let other = fixture_root("ne2");
        let before = fs::metadata(other.join("var/lib/portage/world"))
            .unwrap()
            .mtime_nsec();
        let (c, _, e) = cli(&[
            "convert",
            "--from",
            &format!("files:{}", s(&root)),
            "--to",
            &format!("files:{}", s(&other)),
        ]);
        assert_eq!(c, 2);
        assert!(e.contains("not empty") && e.contains("--force"), "{e}");
        assert_eq!(
            fs::metadata(other.join("var/lib/portage/world"))
                .unwrap()
                .mtime_nsec(),
            before,
            "a refused copy touches nothing"
        );
        // --force over an altered copy makes it equal again.
        fs::remove_dir_all(other.join("var/db/pkg/dev-libs/igw0a-1")).unwrap();
        fs::create_dir_all(other.join("var/db/pkg/x-extra/extra-1")).unwrap();
        let (c, o, e) = cli(&[
            "convert",
            "--force",
            "--from",
            &format!("files:{}", s(&root)),
            "--to",
            &format!("files:{}", s(&other)),
        ]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(o.contains("removed from destination"), "{o}");
        let (c, o, _) = cli(&[
            "verify",
            &format!("files:{}", s(&root)),
            &format!("files:{}", s(&other)),
        ]);
        assert_eq!(c, 0, "{o}");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&other);
    }

    #[test]
    fn a_pending_entry_is_reported_and_not_copied() {
        let root = fixture_root("pend");
        let dst = scratch("pend-dst");
        let cat = root.join("var/db/pkg/dev-libs");
        copy_tree(&cat.join("igw0a-1"), &cat.join("-MERGING-halfdone-1"));
        let (c, o, e) = cli(&[
            "convert",
            "--from",
            &format!("files:{}", s(&root)),
            "--to",
            &format!("files:{}", s(&dst)),
        ]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(
            e.contains("pending entry not copied: dev-libs/halfdone-1"),
            "{e}"
        );
        let vdb = dst.join("var/db/pkg/dev-libs");
        assert!(vdb.join("igw0a-1").is_dir());
        assert!(!vdb.join("halfdone-1").exists());
        assert!(!vdb.join("-MERGING-halfdone-1").exists());
        let (c, o, _) = cli(&[
            "verify",
            &format!("files:{}", s(&root)),
            &format!("files:{}", s(&dst)),
        ]);
        assert_eq!(c, 0, "pending entries are not compared: {o}");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn usage_errors_exit_2_and_help_exits_0() {
        assert_eq!(cli(&["--help"]).0, 0);
        assert_eq!(cli(&[]).0, 2);
        assert_eq!(cli(&["frob"]).0, 2);
        assert_eq!(cli(&["convert", "--from", "files:/x"]).0, 2);
        assert_eq!(cli(&["verify", "files:/x"]).0, 2);
        // A missing redb file is an I/O error (exit 2), never created by verify.
        let (c, _, e) = cli(&["verify", "redb:/nonexistent/a", "redb:/nonexistent/b"]);
        assert_eq!(c, 2);
        assert!(e.contains("portuale vdb:"), "{e}");
        let (c, _, e) = cli(&["verify", "lmdb:/a", "files:/b"]);
        assert_eq!(c, 2);
        assert!(e.contains("unknown VDB backend"), "{e}");
    }

    #[test]
    fn mount_usage_errors_exit_2_before_anything_is_mounted() {
        let (c, o, _) = cli(&["mount", "--help"]);
        assert_eq!(c, 0);
        assert!(o.contains("vdb mount"), "{o}");
        // Arguments: exactly KIND:PATH and MOUNTPOINT.
        for args in [
            &["mount"][..],
            &["mount", "files:/x"],
            &["mount", "files:/x", "/a", "/b"],
            &["mount", "--bogus", "files:/x", "/a"],
        ] {
            let (c, _, e) = cli(args);
            assert_eq!(c, 2, "{args:?}: {e}");
            assert!(e.contains("mount:"), "{e}");
        }
        let (c, _, e) = cli(&["mount", "lmdb:/x", "/tmp"]);
        assert_eq!(c, 2);
        assert!(e.contains("unknown VDB backend"), "{e}");
        // The mountpoint must be an existing directory.
        let (c, _, e) = cli(&["mount", "files:/x", "/nonexistent/mountpoint"]);
        assert_eq!(c, 2);
        assert!(e.contains("existing directory"), "{e}");
        // A database that does not exist is refused before any mount; the
        // backend is opened read-only and never created.
        let mp = scratch("mp");
        let (c, _, e) = cli(&["mount", "sqlite:/nonexistent/vdb.sqlite", &s(&mp)]);
        assert_eq!(c, 2, "{e}");
        assert!(!Path::new("/nonexistent/vdb.sqlite").exists());
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

    fn status_sweep_cycle(spec: &str, keep: &str) {
        let (c, o, e) = cli(&["status", spec]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(o.contains("pending:    none"), "{o}");
        leave_pending(spec, "dev-libs", "halfdone-1");
        let (c, o, e) = cli(&["status", spec]);
        assert_eq!(c, 1, "{o}{e}");
        assert!(o.contains("dev-libs/halfdone-1"), "{o}");
        // An installed key is refused and nothing is removed.
        let (c, _, e) = cli(&["sweep", "--remove", keep, spec]);
        assert_eq!(c, 2);
        assert!(e.contains("installed entry"), "{e}");
        let (c, _, e) = cli(&[
            "sweep",
            "--remove",
            "dev-libs/halfdone-1",
            "--remove",
            keep,
            spec,
        ]);
        assert_eq!(c, 2, "{e}");
        assert_eq!(
            cli(&["status", spec]).0,
            1,
            "the orphan survives a refused sweep"
        );
        // A key that is neither is refused too.
        assert_eq!(cli(&["sweep", "--remove", "x/y-1", spec]).0, 2);
        let (c, o, e) = cli(&["sweep", "--remove", "dev-libs/halfdone-1", spec]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(
            o.contains("removed pending entry dev-libs/halfdone-1"),
            "{o}"
        );
        let (c, o, _) = cli(&["status", spec]);
        assert_eq!(c, 0, "{o}");
        // The installed entry is still there.
        assert!(o.contains("installed:  "), "{o}");
        leave_pending(spec, "dev-libs", "one-1");
        leave_pending(spec, "dev-libs", "two-1");
        let (c, o, e) = cli(&["sweep", "--all", spec]);
        assert_eq!(c, 0, "{o}{e}");
        assert!(o.contains("one-1") && o.contains("two-1"), "{o}");
        assert_eq!(cli(&["status", spec]).0, 0);
    }

    #[test]
    fn status_and_sweep_on_files() {
        let root = fixture_root("sweepfiles");
        let installed = fs::read_dir(root.join("var/db/pkg/dev-libs"))
            .unwrap()
            .map(|d| d.unwrap().file_name().to_string_lossy().into_owned())
            .find(|n| !n.starts_with('-'))
            .unwrap();
        let spec = format!("files:{}", s(&root));
        status_sweep_cycle(&spec, &format!("dev-libs/{installed}"));
        assert!(root.join("var/db/pkg/dev-libs").join(&installed).is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(feature = "vdb-sqlite")]
    #[test]
    fn status_and_sweep_on_sqlite() {
        let root = fixture_root("sweepsql");
        let db = root.join("vdb.sqlite");
        let spec = format!("sqlite:{}", s(&db));
        let (c, o, e) = cli(&[
            "convert",
            "--from",
            &format!("files:{}", s(&root)),
            "--to",
            &spec,
        ]);
        assert_eq!(c, 0, "{o}{e}");
        let d = open(&parse_spec(&spec).unwrap(), false).unwrap();
        let keep = d.entries().unwrap()[0].to_string();
        drop(d);
        let (_, o, _) = cli(&["status", &spec]);
        assert!(o.contains("imported:"), "{o}");
        status_sweep_cycle(&spec, &keep);
        let d = open(&parse_spec(&spec).unwrap(), false).unwrap();
        assert!(d.has_entry(&d.entries().unwrap()[0]).unwrap());
        // A missing database is an error and is not created.
        let missing = format!("sqlite:{}", s(&root.join("none.sqlite")));
        assert_eq!(cli(&["status", &missing]).0, 2);
        assert!(!root.join("none.sqlite").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(feature = "vdb-redb")]
    #[test]
    fn status_and_sweep_on_redb_and_busy_when_held() {
        let root = fixture_root("sweepredb");
        let db = root.join("vdb.redb");
        let spec = format!("redb:{}", s(&db));
        let files = format!("files:{}", s(&root));
        let (c, o, e) = cli(&["convert", "--from", &files, "--to", &spec]);
        assert_eq!(c, 0, "{o}{e}");
        // files -> redb -> files round trip verifies equal.
        let back = root.join("back");
        let back_spec = format!("files:{}", s(&back));
        let (c, o, e) = cli(&["convert", "--from", &spec, "--to", &back_spec]);
        assert_eq!(c, 0, "{o}{e}");
        let (c, o, _) = cli(&["verify", &spec, &back_spec]);
        assert_eq!(c, 0, "{o}");
        let (c, o, _) = cli(&["verify", &files, &spec]);
        assert_eq!(c, 0, "{o}");
        let d = open(&parse_spec(&spec).unwrap(), false).unwrap();
        let keep = d.entries().unwrap()[0].to_string();
        drop(d);
        let (_, o, _) = cli(&["status", &spec]);
        assert!(
            o.contains("imported:") && o.contains("backend:    redb"),
            "{o}"
        );
        status_sweep_cycle(&spec, &keep);
        // A missing database is an error and is not created.
        let missing = format!("redb:{}", s(&root.join("none.redb")));
        assert_eq!(cli(&["status", &missing]).0, 2);
        assert_eq!(cli(&["sweep", "--all", &missing]).0, 2);
        assert!(!root.join("none.redb").exists());
        // Held by a read-write handle: every command reports Busy, exit 2.
        let held = portage_vdb::RedbDb::open(&db).unwrap();
        for args in [
            vec!["status", spec.as_str()],
            vec!["sweep", "--all", spec.as_str()],
            vec!["verify", spec.as_str(), files.as_str()],
            vec![
                "convert",
                "--from",
                files.as_str(),
                "--to",
                spec.as_str(),
                "--force",
            ],
        ] {
            let (c, _, e) = cli(&args);
            assert_eq!(c, 2, "{args:?}: {e}");
            assert!(e.contains("database is already open"), "{args:?}: {e}");
        }
        drop(held);
        assert_eq!(cli(&["status", &spec]).0, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn status_and_sweep_usage_errors_exit_2() {
        assert_eq!(cli(&["status"]).0, 2);
        assert_eq!(cli(&["sweep", "files:/x"]).0, 2);
        assert_eq!(
            cli(&["sweep", "--all", "--remove", "a/b-1", "files:/x"]).0,
            2
        );
        assert_eq!(cli(&["sweep", "--remove", "nocat", "files:/x"]).0, 2);
    }
}
