//! Merging the image tree onto ROOT.

use crate::ebuild_merge::*;

/// `true` unless the destination already exists as a regular file with the
/// same full mode and byte-identical content. When `false`, the destination's
/// inode, ownership and xattrs survive the rebuild and only its mtime is
/// refreshed. Under `FEATURES=xattr` xattrs are compared too
/// (`PORTAGE_XATTR_EXCLUDE`-aware); content is compared in fixed chunks.
// Portage: vartree.py, dblink._needs_move
pub(crate) fn needs_move(src: &Path, dest: &Path, xattrs: &XattrPolicy) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(src_meta) = std::fs::symlink_metadata(src) else {
        return true;
    };
    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        return true;
    };
    if !dest_meta.is_file() || src_meta.permissions().mode() != dest_meta.permissions().mode() {
        return true;
    }
    if xattrs.enabled && !xattrs_equal(src, dest, &xattrs.exclude) {
        return true;
    }
    !files_equal(src, dest).unwrap_or(false)
}

/// Byte-for-byte comparison for [`needs_move`], short-circuiting on a size
/// mismatch first. Chunked so a large object never materializes in memory twice.
pub(crate) fn files_equal(a: &Path, b: &Path) -> std::io::Result<bool> {
    let mut fa = std::fs::File::open(a)?;
    let mut fb = std::fs::File::open(b)?;
    if fa.metadata()?.len() != fb.metadata()?.len() {
        return Ok(false);
    }
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
        let na = read_filling(&mut fa, &mut ba)?;
        let nb = read_filling(&mut fb, &mut bb)?;
        if na != nb || ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// `read` until `buf` is full or EOF (a plain `Read::read` may return a short
/// count, which would otherwise misalign the chunk comparison).
pub(crate) fn read_filling(reader: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::Read as _;
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}
