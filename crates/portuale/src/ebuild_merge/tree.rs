//! Merging the image tree onto ROOT.

use crate::ebuild_merge::*;

/// Finds the repo name by walking up from `pkg_dir` (`<category>/<package>`)
/// for the first `profiles/repo_name` file (the canonical source besides
/// `layout.conf`'s `repo-name` key). `None` when no ancestor has one.
pub(crate) fn repository_name_for(pkg_dir: &Path) -> Option<String> {
    for ancestor in pkg_dir.ancestors() {
        let candidate = ancestor.join("profiles").join("repo_name");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            let name = text.lines().next().unwrap_or("").trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

pub(crate) fn md5_hex_bytes(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex MD5 of a file's bytes. Shared with the remote-merge server so a later
/// index rebuild sees a still-current instance either way.
pub(crate) fn md5_hex(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(md5_hex_bytes(&data))
}

/// Creates a fresh FIFO or device node at `dest`, matching `src`'s type,
/// permission bits, and (for a device) major/minor. Recreated rather than
/// moved so `${D}` stays intact. An explicit `chmod` afterward closes the
/// umask gap (`mkfifo`/`mknod` apply it, unlike `std::fs::copy`).
///
/// # Errors
///
/// Returns `Err` (not a panic) when `mknod` hits the root/`CAP_MKNOD`
/// permission wall for a nonzero major:minor device.
pub(crate) fn create_special_node(
    src: &Path,
    dest: &Path,
    file_type: &std::fs::FileType,
) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;

    let src_meta = std::fs::symlink_metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mode = src_meta.mode() & 0o7777;
    let dest_c = std::ffi::CString::new(dest.as_os_str().as_bytes())
        .map_err(|e| format!("{}: {e}", dest.display()))?;

    let ret = if file_type.is_fifo() {
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus a plain mode int.
        unsafe { libc::mkfifo(dest_c.as_ptr(), mode) }
    } else {
        let type_bit = if file_type.is_char_device() {
            libc::S_IFCHR
        } else {
            libc::S_IFBLK
        };
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain mode/dev ints.
        unsafe {
            libc::mknod(
                dest_c.as_ptr(),
                type_bit | mode,
                src_meta.rdev() as libc::dev_t,
            )
        }
    };
    if ret != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `dest_c` is a CString that outlives the call; takes a
    // NUL-terminated path pointer plus a plain mode int.
    if unsafe { libc::chmod(dest_c.as_ptr(), mode) } != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Sets `dest`'s owner/group to the source's recorded uid/gid from `${D}`
/// (`lchown` for a symlink, `chown` otherwise). Unconditional, like Portage:
/// unprivileged callers only fail on a genuinely privileged change, and that
/// surfaces as an ordinary merge failure, never silently swallowed.
// Portage: movefile.py, movefile._apply_stat
pub(crate) fn lchown_or_chown(
    dest: &Path,
    uid: u32,
    gid: u32,
    is_symlink: bool,
) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let dest_c = std::ffi::CString::new(dest.as_os_str().as_bytes())
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    let ret = if is_symlink {
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain uid/gid ints.
        unsafe { libc::lchown(dest_c.as_ptr(), uid, gid) }
    } else {
        // SAFETY: `dest_c` is a CString that outlives the call; takes a
        // NUL-terminated path pointer plus plain uid/gid ints.
        unsafe { libc::chown(dest_c.as_ptr(), uid, gid) }
    };
    if ret != 0 {
        return Err(format!(
            "{}: {}",
            dest.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

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
