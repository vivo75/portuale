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

/// One `CONTENTS` line: `<type> <path>[ <md5>| -> <target>][ <mtime>]\n`.
// Portage: dblink._format_contents_line
pub(crate) fn format_contents_line(
    node_type: &str,
    abs_path: &str,
    md5_digest: Option<&str>,
    symlink_target: Option<&str>,
    mtime_secs: Option<i64>,
) -> String {
    let mut fields = vec![node_type.to_string(), abs_path.to_string()];
    if let Some(md5) = md5_digest {
        fields.push(md5.to_string());
    } else if let Some(target) = symlink_target {
        fields.push(format!("-> {target}"));
    }
    if let Some(mtime) = mtime_secs {
        fields.push(mtime.to_string());
    }
    format!("{}\n", fields.join(" "))
}

/// Converts a file's mtime to seconds since the epoch. `ebuild_unmerge`'s
/// mtime-staleness check reuses this to compare against `CONTENTS`-recorded times.
pub(crate) fn mtime_secs(metadata: &std::fs::Metadata) -> Result<i64, String> {
    use std::time::UNIX_EPOCH;
    let mtime = metadata
        .modified()
        .map_err(|e| format!("reading mtime: {e}"))?;
    Ok(mtime
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("mtime before epoch: {e}"))?
        .as_secs() as i64)
}

pub(crate) struct MergeTreeArgs<'a> {
    pub(crate) d: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) installed_instance_pf: Option<&'a str>,
    pub(crate) protect_if_modified: bool,
    pub(crate) config_protect: &'a str,
    pub(crate) config_protect_mask: &'a str,
    pub(crate) noconfmem: bool,
    pub(crate) cfgfiledict: &'a mut BTreeMap<String, String>,
    pub(crate) xattrs: &'a XattrPolicy,
}

/// Walks `d` (`${D}`) and merges every entry into `root` (`${ROOT}`),
/// returning the accumulated `CONTENTS` text. `cfgfiledict` is read once by
/// the caller before this runs and written back once after, a single
/// whole-merge read/update/write, not a per-file one.
// Portage: vartree.py, mergeme; vardbapi._conf_mem_file
pub(crate) fn merge_tree(args: MergeTreeArgs<'_>) -> Result<String, String> {
    let MergeTreeArgs {
        d,
        root,
        category,
        installed_instance_pf,
        protect_if_modified,
        config_protect,
        config_protect_mask,
        noconfmem,
        cfgfiledict,
        xattrs,
    } = args;
    let mut contents = String::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = d.join(&relative_dir);
        let children: Vec<PathBuf> = portage_util::read_dir_entries(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .into_iter()
            .map(|e| relative_dir.join(e.file_name()))
            .collect();

        for relative_path in children {
            let src = d.join(&relative_path);
            let dest = root.join(&relative_path);
            let abs_path = format!("/{}", relative_path.display());
            let file_type = std::fs::symlink_metadata(&src)
                .map_err(|e| format!("{}: {e}", src.display()))?
                .file_type();

            if file_type.is_symlink() {
                let target =
                    std::fs::read_link(&src).map_err(|e| format!("{}: {e}", src.display()))?;
                let target_str = target.to_string_lossy().to_string();
                // A CONFIG_PROTECT'd `sym` entry whose live destination differs
                // is diverted to a fresh ._cfgNNNN_ sibling (the target
                // string's own MD5 is what's hashed, not file content).
                // Portage: dblink._protect
                let mut write_dest = dest.to_path_buf();
                let mut moveme = true;
                let protected_path = is_protected(root, config_protect, config_protect_mask, &dest);
                if protected_path {
                    let src_md5 = md5_hex_bytes(target_str.as_bytes());
                    (write_dest, moveme) = protect_decision(ProtectDecisionArgs {
                        root,
                        category,
                        installed_instance_pf,
                        protect_if_modified,
                        dest: &dest,
                        abs_path: &abs_path,
                        src_md5: &src_md5,
                        cfgfiledict,
                        noconfmem,
                    })?;
                }
                // A symlink can never replace a directory: it lands at the
                // first `dest.backup.NNNN` instead and the directory is kept.
                // Directory-target symlinks always reach here and still exit 0.
                // Portage: vartree.py, mergeme
                if moveme
                    && !protected_path
                    && write_dest == dest
                    && std::fs::symlink_metadata(&dest)
                        .map(|m| m.file_type().is_dir())
                        .unwrap_or(false)
                {
                    write_dest = new_backup_path(&dest);
                    eprintln!("Installation of a symlink is blocked by a directory:");
                    eprintln!("  '{}'", dest.display());
                    eprintln!("This symlink will be merged with a different name:");
                    eprintln!("  '{}'", write_dest.display());
                    eprintln!();
                }

                let mtime = mtime_secs(
                    &std::fs::symlink_metadata(&src)
                        .map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // `moveme == false` (confmem rejected this update): skip the
                // write entirely. `mtime` still uses the source's mtime for
                // CONTENTS either way.
                // Portage: vartree.py, mergeme
                if moveme {
                    if let Some(parent) = write_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    // Installed by atomic rename, like a regular file.
                    replace_symlink_atomic(&target, &src, &write_dest, mtime)?;
                }
                // CONTENTS records the logical path/target, never the ._cfgNNNN_
                // variant a protected write may have landed at.
                contents.push_str(&format_contents_line(
                    "sym",
                    &abs_path,
                    None,
                    Some(&target_str),
                    Some(mtime),
                ));
            } else if file_type.is_dir() {
                // A newly created directory takes the source's mode/owner; an
                // existing one is left alone (a shared `/usr/lib` must never
                // have ownership clobbered by every package installing into it).
                // Portage: vartree.py, mergeme
                let dir_existed = dest.is_dir();
                std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                if !dir_existed {
                    use std::os::unix::fs::PermissionsExt;
                    let src_meta =
                        std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?;
                    lchown_or_chown(&dest, src_meta.uid(), src_meta.gid(), false)?;
                    std::fs::set_permissions(
                        &dest,
                        std::fs::Permissions::from_mode(src_meta.permissions().mode()),
                    )
                    .map_err(|e| format!("{}: {e}", dest.display()))?;
                }
                contents.push_str(&format_contents_line("dir", &abs_path, None, None, None));
                stack.push(relative_path);
            } else if file_type.is_file() {
                let src_md5 = md5_hex(&src)?;
                // A protected path whose on-disk content differs is diverted to
                // a fresh ._cfgNNNN_ sibling, unless cfgfiledict already
                // remembers this src_md5 for this path (`NOCONFMEM` forces
                // re-protection regardless).
                // Portage: dblink._protect
                let mut write_dest = dest.to_path_buf();
                let mut moveme = true;
                if is_protected(root, config_protect, config_protect_mask, &dest) {
                    (write_dest, moveme) = protect_decision(ProtectDecisionArgs {
                        root,
                        category,
                        installed_instance_pf,
                        protect_if_modified,
                        dest: &dest,
                        abs_path: &abs_path,
                        src_md5: &src_md5,
                        cfgfiledict,
                        noconfmem,
                    })?;
                }

                let mtime = mtime_secs(
                    &std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // `moveme == false` (confmem rejected this update): skip the copy
                // entirely. `mtime` still uses the source's mtime for CONTENTS.
                // Portage: vartree.py, mergeme
                if moveme {
                    if let Some(parent) = write_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    // A destination already a regular file with the same mode and
                    // byte-identical content is left in place (inode, ownership
                    // and xattrs survive; only mtime is refreshed). A diverted
                    // ._cfgNNNN_ write never takes this path.
                    // Portage: vartree.py, dblink._needs_move
                    if write_dest == dest && !needs_move(&src, &write_dest, xattrs) {
                        filetime::set_file_mtime(
                            &write_dest,
                            filetime::FileTime::from_unix_time(mtime, 0),
                        )
                        .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                    } else {
                        // Atomic replacement: the destination's existing inode is
                        // never opened for write (a safety requirement, not an
                        // optimization).
                        replace_file_atomic(&src, &write_dest, mtime, xattrs)?;
                    }
                }
                // CONTENTS records the logical path and the source's MD5, never
                // the ._cfgNNNN_ variant; the vdb owns the logical path either way.
                contents.push_str(&format_contents_line(
                    "obj",
                    &abs_path,
                    Some(&src_md5),
                    None,
                    Some(mtime),
                ));
            } else if file_type.is_fifo()
                || file_type.is_char_device()
                || file_type.is_block_device()
            {
                // Fifo/device nodes: never protected; only created when the live
                // destination doesn't exist yet (an existing node is left
                // alone). The CONTENTS line is written unconditionally, with
                // no digest/mtime/target. Recreated via mkfifo/mknod (not
                // moved) so `${D}` stays intact.
                // Portage: vartree.py, mergeme
                if std::fs::symlink_metadata(&dest).is_err() {
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    create_special_node(&src, &dest, &file_type)?;
                }
                let node_type = if file_type.is_fifo() { "fif" } else { "dev" };
                contents.push_str(&format_contents_line(
                    node_type, &abs_path, None, None, None,
                ));
            }
        }
    }
    Ok(contents)
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
