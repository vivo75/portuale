//! Scratch copies of an entry for code that needs a path (S4.1, N9).
//!
//! Bash and the copiers take files by path: `PORTAGE_UPDATE_ENV` (the
//! `pkg_postinst` rewrite of `environment.bz2`), the saved-environment
//! phases of an installed instance (`pkg_prerm`, `pkg_postrm`,
//! `pkg_config`) and `quickpkg`. On `files` the caller uses
//! [`InstalledDb::entry_path`] and none of this runs. A database backend
//! has no entry directory, so the caller writes the files it needs into a
//! scratch directory ([`materialize_files`], [`materialize_entry`]), hands
//! that path on, and afterwards stores back a file the phase rewrote
//! ([`absorb_file`], one [`WriteTxn::replace_file`] in its own commit).
//!
//! The helpers work on any backend; they only read through
//! [`InstalledDb`] and write through [`WriteTxn`].

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use crate::{EntryKey, Error, InstalledDb, Result};

/// Write `data` to `path` with the permission bits of `mode` (the type
/// bits are ignored). An existing file is replaced.
fn write_scratch(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(path, e)),
    }
    std::fs::write(path, data).map_err(|e| Error::io(path, e))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o7777))
        .map_err(|e| Error::io(path, e))
}

/// Copy the files `names` of the live entry `key` into `dir` (created
/// when missing), each as `dir/<name>` with its stored bytes and
/// permission bits. A name the entry does not have is skipped (and a
/// stale scratch copy of it removed). Returns how many were written;
/// `0` also when the entry does not exist.
pub fn materialize_files(
    db: &dyn InstalledDb,
    key: &EntryKey,
    names: &[&str],
    dir: &Path,
) -> Result<usize> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let mut written = 0;
    for &name in names {
        let path = dir.join(name);
        match (db.file_meta(key, name)?, db.read_file(key, name)?) {
            (Some(meta), Some(data)) => {
                write_scratch(&path, &data, meta.mode)?;
                written += 1;
            }
            _ => match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(&path, e)),
            },
        }
    }
    Ok(written)
}

/// Copy every file of the live entry `key` into a fresh `dir` (an
/// existing `dir` is removed first), so a copier that takes the whole
/// directory (`quickpkg`) sees exactly the entry. Returns `false` (and
/// leaves `dir` alone) when the entry does not exist.
pub fn materialize_entry(db: &dyn InstalledDb, key: &EntryKey, dir: &Path) -> Result<bool> {
    let Some(image) = db.entry_image(key)? else {
        return Ok(false);
    };
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    for f in &image.files {
        write_scratch(&dir.join(&f.meta.name), &f.data, f.meta.mode)?;
    }
    Ok(true)
}

/// Store `dir/<name>` back into the live entry `key` when it differs from
/// the stored file (or the entry has none): one transaction with one
/// [`crate::WriteTxn::replace_file`], committed. Returns whether it wrote.
/// A missing scratch file writes nothing (the phase did not create it).
pub fn absorb_file(db: &dyn InstalledDb, key: &EntryKey, dir: &Path, name: &str) -> Result<bool> {
    let path = dir.join(name);
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::io(&path, e)),
    };
    if db.read_file(key, name)?.as_deref() == Some(data.as_slice()) {
        return Ok(false);
    }
    let mut txn = db.begin_write()?;
    txn.replace_file(key, name, &data)?;
    txn.commit()?;
    Ok(true)
}

#[cfg(all(test, feature = "vdb-sqlite"))]
mod tests {
    use super::*;
    use crate::SqliteDb;
    use std::path::PathBuf;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        let d =
            std::env::temp_dir().join(format!("portage-vdb-scratch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }

    fn install(db: &SqliteDb, key: &EntryKey, files: &[(&str, &[u8])]) {
        let mut txn = db.begin_write().unwrap();
        txn.begin_entry(key).unwrap();
        for (name, data) in files {
            txn.put_entry_file(key, name, data).unwrap();
        }
        txn.finish_entry(key).unwrap();
        txn.commit().unwrap();
    }

    #[test]
    fn materialize_then_absorb_round_trips_a_rewritten_file() {
        let t = tmp("absorb");
        let db = SqliteDb::open(t.0.join("vdb.sqlite")).unwrap();
        let key = EntryKey::new("dev-libs", "a-1");
        install(&db, &key, &[("environment.bz2", b"old"), ("SLOT", b"0\n")]);
        let dir = t.0.join("scratch");
        let n = materialize_files(&db, &key, &["environment.bz2", "missing"], &dir).unwrap();
        assert_eq!(n, 1);
        assert_eq!(std::fs::read(dir.join("environment.bz2")).unwrap(), b"old");
        assert!(!dir.join("missing").exists());
        // Unchanged: nothing is written, the generation stays.
        let g = db.generation().unwrap();
        assert!(!absorb_file(&db, &key, &dir, "environment.bz2").unwrap());
        assert_eq!(db.generation().unwrap(), g);
        std::fs::write(dir.join("environment.bz2"), b"new").unwrap();
        assert!(absorb_file(&db, &key, &dir, "environment.bz2").unwrap());
        assert_eq!(
            db.read_file(&key, "environment.bz2").unwrap().as_deref(),
            Some(&b"new"[..])
        );
        assert_eq!(db.generation().unwrap(), g + 1);

        let whole = t.0.join("whole");
        assert!(materialize_entry(&db, &key, &whole).unwrap());
        let mut names: Vec<String> = std::fs::read_dir(&whole)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["SLOT", "environment.bz2"]);
        assert!(!materialize_entry(&db, &EntryKey::new("x", "y-1"), &whole).unwrap());
    }

    /// `finish_entry_replacing` on sqlite: the old entry is installed and
    /// the new one pending until the commit; one commit (generation + 1)
    /// swaps them.
    #[test]
    fn finish_entry_replacing_swaps_in_one_commit() {
        let t = tmp("replace");
        let db = SqliteDb::open(t.0.join("vdb.sqlite")).unwrap();
        let old = EntryKey::new("dev-libs", "a-1");
        let new = EntryKey::new("dev-libs", "a-2");
        install(&db, &old, &[("SLOT", b"0\n")]);
        {
            let mut txn = db.begin_write().unwrap();
            txn.begin_entry(&new).unwrap();
            txn.put_entry_file(&new, "SLOT", b"0\n").unwrap();
            txn.commit().unwrap();
        }
        let g = db.generation().unwrap();
        let mut txn = db.begin_write().unwrap();
        txn.finish_entry_replacing(&new, std::slice::from_ref(&old))
            .unwrap();
        // Not committed yet: readers see the old state.
        assert!(db.has_entry(&old).unwrap());
        assert!(!db.has_entry(&new).unwrap());
        assert_eq!(db.pending_entries().unwrap(), vec![new.clone()]);
        txn.commit().unwrap();
        assert!(!db.has_entry(&old).unwrap());
        assert!(db.has_entry(&new).unwrap());
        assert!(db.pending_entries().unwrap().is_empty());
        assert_eq!(db.generation().unwrap(), g + 1);
    }
}
