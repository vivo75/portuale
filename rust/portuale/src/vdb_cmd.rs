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
// Exit codes: 0 success / verify equal, 1 verify found differences,
// 2 usage or I/O error.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portage_vdb::{BackendKind, FilesDb, InstalledDb};

const USAGE: &str = "\
Usage:
   portuale vdb convert --from KIND:PATH --to KIND:PATH [--force]
   portuale vdb verify [--against] KIND:PATH KIND:PATH
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
   redb:FILE      not implemented yet (S5)

convert options:
   --from KIND:PATH   the source (read only)
   --to KIND:PATH     the destination; must hold no entries unless --force
   --force            make the destination an exact copy: replace same-name
                      entries and delete the ones the source lacks
   Entries mid-merge (-MERGING-) are not copied; they are listed.

verify compares live entries (files, bytes, modes, mtimes, directory mode and
mtime, metadata-stamp state), world, world_sets, preserved libs, config memory
and the counter. Differences are listed, at most 100 lines.

Exit status: 0 done / equal, 1 verify found differences, 2 usage or I/O error.";

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
        other => Err(format!(
            "unknown subcommand {other:?} (expected convert or verify); see `portuale vdb --help`"
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
        .ok_or_else(|| format!("{s:?}: expected KIND:PATH (files:ROOT, sqlite:FILE)"))?;
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
        BackendKind::Redb => Err("redb: not built / not implemented yet (plan S5)".into()),
    }
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
        let (c, _, e) = cli(&["verify", "redb:/a", "redb:/b"]);
        assert_eq!(c, 2);
        assert!(e.contains("redb") && e.contains("S5"), "{e}");
        let (c, _, e) = cli(&["verify", "lmdb:/a", "files:/b"]);
        assert_eq!(c, 2);
        assert!(e.contains("unknown VDB backend"), "{e}");
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
}
