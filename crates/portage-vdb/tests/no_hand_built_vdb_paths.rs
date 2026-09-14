//! Test that production code doesn't build installed-database paths by hand.
//!
//! The only correct way to get a VDB path is through the public API
//! (portage_vdb::for_root, FilesDb::open_vdb_dir, etc.), never by hand.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
}
