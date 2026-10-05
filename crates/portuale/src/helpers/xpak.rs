//! Native `xpak-helper.py recompose`: packs a metadata dir into a tbz2.
//!
//! The inverse of the `binpkg.rs` reader. `xpak(dir)` walks the tree
//! (`sub/` for real subdirectories, symlinked dirs skipped, `CONTENTS`
//! skipped, non-UTF-8 names packed surrogateescape-style), and
//! `recompose_mem` swaps the trailing segment, breaking hardlinks first.
// Portage: bin/xpak-helper.py, command_recompose

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Portage `tbz2.scan()`'s `xpaksize`: `infosize + 8` when the last 16
/// bytes are `XPAKSTOP be32(infosize) STOP`, else 0.
fn scan_xpaksize(file: &Path) -> std::io::Result<u64> {
    let mut f = std::fs::File::open(file)?;
    let len = f.seek(SeekFrom::End(0))?;
    if len < 16 {
        return Ok(0);
    }
    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))?;
    f.read_exact(&mut trailer)?;
    if &trailer[12..16] != b"STOP" || &trailer[0..8] != b"XPAKSTOP" {
        return Ok(0);
    }
    let infosize = u32::from_be_bytes([trailer[8], trailer[9], trailer[10], trailer[11]]) as u64;
    Ok(infosize + 8)
}

#[cfg(test)]
mod tests {
    use portage_util::TempDir;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/helpers/xpak")
    }
}
