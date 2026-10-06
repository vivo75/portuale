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

/// Portage `recompose_mem`'s hardlink break: a content-only copy into a
/// new file in the same directory, renamed over.
fn break_hardlink(file: &Path) -> std::io::Result<()> {
    let dir = file.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let base = file
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let mut n = 0u32;
    let (tmp, mut dst): (PathBuf, std::fs::File) = loop {
        let mut name = base.clone().into_vec();
        name.extend_from_slice(format!(".{}{n}", std::process::id()).as_bytes());
        let candidate = dir.join(OsString::from_vec(name));
        // Default creation mode 0o666, filtered by the umask.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        // Content only: `std::fs::copy` would also copy the permissions.
        std::io::copy(&mut std::fs::File::open(file)?, &mut dst)?;
        drop(dst);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
