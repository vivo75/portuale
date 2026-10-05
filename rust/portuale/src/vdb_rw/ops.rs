//! Test harness for `vdb_rw` (#317): replay one operation script through
//! an `RwView` and on a plain `files` VDB directory, then compare the two
//! databases entry by entry.
//!
//! Script format, one operation per line (`#` comments, blank lines
//! ignored), paths relative to `var/db/pkg`:
//!
//! ```text
//! mkdir   PATH MODE          create  PATH MODE     (O_CREAT|O_EXCL)
//! write   PATH BYTES         append  PATH BYTES    (BYTES: rest of line, \n \t \\ escapes)
//! unlink  PATH               unlink? PATH          (? = ENOENT is fine)
//! rmdir   PATH               rmdir?  PATH          (? = ENOTEMPTY/ENOENT is fine)
//! rename  FROM TO            link    FROM TO
//! chmod   PATH MODE          chown   PATH UID GID  (-1 = unchanged)
//! utime   PATH NS            sameino A B           (assert one inode)
//! stamp   FILE DIR           append `#dir_mtime=<DIR's mtime ns>\n` to FILE
//!                            (real `_stamp_metadata_file`)
//! ```

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use portage_vdb::{EntryKey, FilesDb, InstalledDb, MetadataStamp};

use super::{RwView, SetAttr};
use crate::vdb_view::{ROOT_INO, ViewError, rewrite_dir_mtime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Mkdir(String, u32),
    Create(String, u32),
    Write(String, Vec<u8>),
    Append(String, Vec<u8>),
    Unlink(String, bool),
    Rmdir(String, bool),
    Rename(String, String),
    Link(String, String),
    Chmod(String, u32),
    Chown(String, Option<u32>, Option<u32>),
    Utime(String, i128),
    SameIno(String, String),
    Stamp(String, String),
}

fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        if b == b'\\' {
            match it.next() {
                Some(b'n') => out.push(b'\n'),
                Some(b't') => out.push(b'\t'),
                Some(b'\\') => out.push(b'\\'),
                Some(o) => {
                    out.push(b'\\');
                    out.push(o);
                }
                None => out.push(b'\\'),
            }
        } else {
            out.push(b);
        }
    }
    out
}

/// Parse a script (see the module doc).
pub fn parse(script: &str) -> Vec<Op> {
    let mut ops = Vec::new();
    for (n, raw) in script.lines().enumerate() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let args: Vec<&str> = rest.split_whitespace().collect();
        let a = |i: usize| -> String {
            args.get(i)
                .unwrap_or_else(|| panic!("line {}: {verb}: missing argument {i}", n + 1))
                .to_string()
        };
        let mode = |i: usize| u32::from_str_radix(&a(i), 8).expect("octal mode");
        let id = |i: usize| match a(i).as_str() {
            "-1" => None,
            v => Some(v.parse::<u32>().expect("uid/gid")),
        };
        let bytes = || {
            let (_, b) = rest.split_once(' ').unwrap_or((rest, ""));
            unescape(b)
        };
        ops.push(match verb {
            "mkdir" => Op::Mkdir(a(0), mode(1)),
            "create" => Op::Create(a(0), mode(1)),
            "write" => Op::Write(a(0), bytes()),
            "append" => Op::Append(a(0), bytes()),
            "unlink" => Op::Unlink(a(0), false),
            "unlink?" => Op::Unlink(a(0), true),
            "rmdir" => Op::Rmdir(a(0), false),
            "rmdir?" => Op::Rmdir(a(0), true),
            "rename" => Op::Rename(a(0), a(1)),
            "link" => Op::Link(a(0), a(1)),
            "chmod" => Op::Chmod(a(0), mode(1)),
            "chown" => Op::Chown(a(0), id(1), id(2)),
            "utime" => Op::Utime(a(0), a(1).parse().expect("ns")),
            "sameino" => Op::SameIno(a(0), a(1)),
            "stamp" => Op::Stamp(a(0), a(1)),
            other => panic!("line {}: unknown verb {other:?}", n + 1),
        });
    }
    ops
}

// --------------------------------------------------------------- RwView

/// The inode of `path` (relative to the VDB root).
pub fn resolve(v: &RwView, path: &str) -> Result<u64, ViewError> {
    let mut ino = ROOT_INO;
    for c in path.split('/').filter(|c| !c.is_empty()) {
        ino = v.lookup(ino, c)?.ino;
    }
    Ok(ino)
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn rw_write(v: &RwView, path: &str, data: &[u8], append: bool) -> Result<(), ViewError> {
    let (dir, name) = split(path);
    let parent = resolve(v, dir)?;
    let (fh, off) = match v.lookup(parent, name) {
        Ok(a) => {
            let flags = libc::O_WRONLY
                | if append {
                    libc::O_APPEND
                } else {
                    libc::O_TRUNC
                };
            let fh = v.open(a.ino, flags)?;
            (fh, if append { a.size } else { 0 })
        }
        Err(ViewError::NoEnt) => {
            let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
            (v.create(parent, name, 0o644, flags)?.1, 0)
        }
        Err(e) => return Err(e),
    };
    let r = v.write(fh, off, data).map(|_| ());
    let rel = v.release(fh);
    r.and(rel)
}

fn rw_one(v: &RwView, op: &Op) -> Result<(), ViewError> {
    match op {
        Op::Mkdir(p, m) => {
            let (dir, name) = split(p);
            v.mkdir(resolve(v, dir)?, name, *m).map(|_| ())
        }
        Op::Create(p, m) => {
            let (dir, name) = split(p);
            let flags = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL;
            let (_, fh) = v.create(resolve(v, dir)?, name, *m, flags)?;
            v.release(fh)
        }
        Op::Write(p, b) => rw_write(v, p, b, false),
        Op::Append(p, b) => rw_write(v, p, b, true),
        Op::Unlink(p, _) => {
            let (dir, name) = split(p);
            v.unlink(resolve(v, dir)?, name)
        }
        Op::Rmdir(p, _) => {
            let (dir, name) = split(p);
            v.rmdir(resolve(v, dir)?, name)
        }
        Op::Rename(a, b) => {
            let ((da, na), (db, nb)) = (split(a), split(b));
            v.rename(resolve(v, da)?, na, resolve(v, db)?, nb)
        }
        Op::Link(a, b) => {
            let (db, nb) = split(b);
            v.link(resolve(v, a)?, resolve(v, db)?, nb).map(|_| ())
        }
        Op::Chmod(p, m) => {
            let s = SetAttr {
                mode: Some(*m),
                ..SetAttr::default()
            };
            v.setattr(resolve(v, p)?, &s).map(|_| ())
        }
        Op::Chown(p, u, g) => {
            let s = SetAttr {
                uid: *u,
                gid: *g,
                ..SetAttr::default()
            };
            v.setattr(resolve(v, p)?, &s).map(|_| ())
        }
        Op::Utime(p, ns) => {
            let s = SetAttr {
                mtime_ns: Some(*ns),
                ..SetAttr::default()
            };
            v.setattr(resolve(v, p)?, &s).map(|_| ())
        }
        Op::SameIno(a, b) => {
            assert_eq!(resolve(v, a)?, resolve(v, b)?, "sameino {a} {b}");
            Ok(())
        }
        Op::Stamp(f, d) => {
            let ns = v.getattr(resolve(v, d)?)?.mtime_ns;
            rw_write(v, f, format!("#dir_mtime={ns}\n").as_bytes(), true)
        }
    }
}

fn tolerated(op: &Op, e: &ViewError) -> bool {
    match op {
        Op::Unlink(_, true) => *e == ViewError::NoEnt,
        Op::Rmdir(_, true) => matches!(e, ViewError::NoEnt | ViewError::NotEmpty),
        _ => false,
    }
}

/// Replay `ops` through `v`; panics on the first unexpected error.
pub fn apply_rw(v: &RwView, ops: &[Op]) {
    for op in ops {
        if let Err(e) = rw_one(v, op)
            && !tolerated(op, &e)
        {
            panic!("{op:?}: {e:?}");
        }
    }
}

// ---------------------------------------------------------------- files

fn fs_one(vdb: &Path, op: &Op) -> std::io::Result<()> {
    let p = |s: &str| vdb.join(s);
    match op {
        Op::Mkdir(d, m) => {
            std::fs::create_dir(p(d))?;
            std::fs::set_permissions(p(d), std::fs::Permissions::from_mode(*m))
        }
        Op::Create(f, m) => {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(p(f))?;
            // Exactly MODE, whatever the process umask (#308).
            std::fs::set_permissions(p(f), std::fs::Permissions::from_mode(*m))
        }
        Op::Write(f, b) => std::fs::write(p(f), b),
        Op::Append(f, b) => {
            use std::io::Write as _;
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(p(f))?
                .write_all(b)
        }
        Op::Unlink(f, _) => std::fs::remove_file(p(f)),
        Op::Rmdir(d, _) => std::fs::remove_dir(p(d)),
        Op::Rename(a, b) => std::fs::rename(p(a), p(b)),
        Op::Link(a, b) => std::fs::hard_link(p(a), p(b)),
        Op::Chmod(f, m) => std::fs::set_permissions(p(f), std::fs::Permissions::from_mode(*m)),
        // Owners are not compared (the database stores none).
        Op::Chown(..) => Ok(()),
        Op::Utime(f, ns) => {
            let t = crate::vdb_view::system_time(*ns);
            std::fs::File::options()
                .write(false)
                .read(true)
                .open(p(f))?
                .set_modified(t)
        }
        Op::SameIno(a, b) => {
            assert_eq!(
                std::fs::metadata(p(a))?.ino(),
                std::fs::metadata(p(b))?.ino()
            );
            Ok(())
        }
        Op::Stamp(f, d) => {
            let md = std::fs::metadata(p(d))?;
            let ns = md.mtime() as i128 * 1_000_000_000 + md.mtime_nsec() as i128;
            fs_one(
                vdb,
                &Op::Append(f.clone(), format!("#dir_mtime={ns}\n").into_bytes()),
            )
        }
    }
}

/// Replay `ops` on the plain VDB directory `vdb`.
pub fn apply_fs(vdb: &Path, ops: &[Op]) {
    for op in ops {
        if let Err(e) = fs_one(vdb, op) {
            let ok = match op {
                Op::Unlink(_, true) => e.kind() == std::io::ErrorKind::NotFound,
                Op::Rmdir(_, true) => matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ),
                _ => false,
            };
            assert!(ok, "{op:?}: {e}");
        }
    }
}

// ------------------------------------------------------------ comparing

/// The two databases hold the same live entries with the same files,
/// bytes, modes and `metadata` stamp state. Mtimes are not compared: the
/// two replays stamp "now" independently (the stamp rule is checked
/// through the stamp state instead).
pub fn assert_same(files_root: &Path, db: &dyn InstalledDb) {
    let files = FilesDb::new(files_root);
    let mut ka = files.entries().unwrap();
    let mut kb = db.entries().unwrap();
    ka.sort();
    kb.sort();
    assert_eq!(ka, kb, "live entries");
    for k in &ka {
        let a = files.entry_image(k).unwrap().expect("files image");
        let b = db.entry_image(k).unwrap().expect("db image");
        assert_eq!(a.metadata_stamp, b.metadata_stamp, "{k}: stamp state");
        assert_eq!(a.dir_mode & 0o7777, b.dir_mode & 0o7777, "{k}: dir mode");
        let names = |i: &portage_vdb::EntryImage| -> Vec<String> {
            i.files.iter().map(|f| f.meta.name.clone()).collect()
        };
        assert_eq!(names(&a), names(&b), "{k}: file names");
        for (fa, fb) in a.files.iter().zip(&b.files) {
            let n = &fa.meta.name;
            assert_eq!(
                fa.meta.mode & 0o7777,
                fb.meta.mode & 0o7777,
                "{k}/{n}: mode"
            );
            if n == "metadata" && a.metadata_stamp != MetadataStamp::Absent {
                assert_eq!(
                    rewrite_dir_mtime(&fa.data, 0),
                    rewrite_dir_mtime(&fb.data, 0),
                    "{k}/{n}: bytes (stamp value aside)"
                );
            } else {
                assert_eq!(fa.data, fb.data, "{k}/{n}: bytes");
            }
        }
    }
}

// ------------------------------------------------------------------ envs

/// One database backend under test, with its own `files` twin.
pub struct Env {
    pub label: &'static str,
    /// The `files` ROOT scripts are replayed on (the oracle).
    pub files_root: PathBuf,
    pub files_vdb: PathBuf,
    /// The database the `RwView` writes to: a conversion of the twin's
    /// starting state.
    pub db: Arc<dyn InstalledDb>,
    /// The database file (read by the sqlite-only failure tests).
    #[cfg_attr(not(feature = "vdb-sqlite"), allow(dead_code))]
    pub db_path: PathBuf,
    /// The `RwView`'s scratch directory.
    pub scratch: PathBuf,
    /// The one entry both start with.
    pub seeded: EntryKey,
    dir: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One `Env` per database backend compiled in. Both sides start with one
/// live entry, `dev-libs/seed-1` (`SLOT`, `CONTENTS`, `COUNTER 7`).
pub fn envs(tag: &str) -> Vec<Env> {
    let mut out = Vec::new();
    for label in ["sqlite", "redb"] {
        if (label == "sqlite" && !cfg!(feature = "vdb-sqlite"))
            || (label == "redb" && !cfg!(feature = "vdb-redb"))
        {
            continue;
        }
        let dir = std::env::temp_dir().join(format!(
            "portuale-vdbrw-{}-{tag}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let files_root = dir.join("root");
        let files_vdb = FilesDb::new(&files_root)
            .vdb_dir()
            .expect("a files VDB dir");
        let seeded = EntryKey::new("dev-libs", "seed-1");
        let e = files_vdb.join("dev-libs/seed-1");
        std::fs::create_dir_all(&e).unwrap();
        for (n, b) in [
            ("SLOT", &b"0\n"[..]),
            ("CONTENTS", b"obj /usr/bin/seed 0123 1\n"),
            ("COUNTER", b"7"),
        ] {
            std::fs::write(e.join(n), b).unwrap();
        }
        let spec = format!("{label}:{}", dir.join(format!("vdb.{label}")).display());
        let args: Vec<String> = [
            "convert",
            "--from",
            &format!("files:{}", files_root.display()),
            "--to",
            &spec,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (mut o, mut er) = (Vec::new(), Vec::new());
        let code = crate::vdb_cmd::run_args(&args, &mut o, &mut er);
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&er));
        let path = dir.join(format!("vdb.{label}"));
        let db: Arc<dyn InstalledDb> = match label {
            #[cfg(feature = "vdb-sqlite")]
            "sqlite" => Arc::new(portage_vdb::SqliteDb::open(&path).unwrap()),
            #[cfg(feature = "vdb-redb")]
            "redb" => Arc::new(portage_vdb::RedbDb::open(&path).unwrap()),
            _ => unreachable!("{path:?}"),
        };
        out.push(Env {
            label,
            files_root,
            files_vdb,
            db,
            db_path: path,
            scratch: dir.join("scratch"),
            seeded,
            dir,
        });
    }
    out
}
