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

use super::SetAttr;
use crate::vdb_view::ViewError;

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
