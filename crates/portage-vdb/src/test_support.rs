//! Test helpers shared by the backend test modules.

use crate::{EntryKey, Error, FilesDb, InstalledDb, METADATA_FILE_FIELDS};

/// Read `name` as `files` shows the live and the pending file.
pub(crate) const PROBE_FILES: &[&str] = &[
    "CONTENTS",
    "SLOT",
    "EMPTY",
    "BIN",
    "NUL",
    "metadata",
    "NEEDED.ELF.2",
    "nope",
    "extra",
    "DESCRIPTION",
    "RDEPEND",
];
