//! Test harness for `vdb_rw`: replay one operation script through
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
//!                            (Portage's `_stamp_metadata_file`)
//! ```

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use portage_vdb::{EntryKey, FilesDb, InstalledDb, MetadataStamp};

use super::{RwView, SetAttr};
use crate::vdb_view::{ROOT_INO, ViewError, rewrite_dir_mtime};

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
/// live entry, `dev-libs/seed-1` (`SLOT`, `CONTENTS`, `COUNTER 7`,
/// `environment.bz2`).
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
            ("environment.bz2", b"BZh91AY&SY seed env"),
        ] {
            std::fs::write(e.join(n), b).unwrap();
            // Exactly 0644, whatever the process umask.
            std::fs::set_permissions(e.join(n), std::fs::Permissions::from_mode(0o644)).unwrap();
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
