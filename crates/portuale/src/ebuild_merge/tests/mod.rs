use super::*;

fn fixtures_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The merge on a database backend.
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
mod db_merge;
