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
