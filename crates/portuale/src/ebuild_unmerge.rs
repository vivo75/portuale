//! Package removal: `ebuild <file> unmerge`.
//!
//! Runs `pkg_prerm`, deletes every `CONTENTS`-listed entry deepest-first,
//! runs `pkg_postrm`, then removes the vdb entry itself on success.
//! Portage: dbapi/vartree.py, dblink.unmerge.
//!
//! Gaps: no `bsd_chflags` handling (dead code on Linux); other I/O errors
//! fail hard where Portage counts per-file failures; the builddir is
//! recomputed fresh instead of reusing a prior merge's.

use crate::ebuild_merge;
use crate::ebuild_phases;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Removes the vdb entry directory itself, then best-effort `rmdir`s its
/// now-maybe-empty parent category directory (ignored on failure). Split
/// out so the binpkg replace path can drop an old entry phaselessly.
/// Portage: dbapi/vartree.py, dblink.delete.
pub(crate) fn delete_vdb_dir(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    // Deletes the entry, then best-effort category `rmdir`.
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.delete_entry(&portage_vdb::EntryKey::new(category, pf))
        .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}
