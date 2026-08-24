//! Scratch copies of an entry for code that needs a path.
//!
//! Bash and the copiers take files by path. On `files` the caller uses
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
