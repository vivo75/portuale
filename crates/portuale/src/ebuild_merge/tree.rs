//! Merging the image tree onto ROOT.

use crate::ebuild_merge::*;

/// Real portage's own mechanism for naming a repo (`layout.conf`'s
/// `repo-name` key aside, the canonical source is a repo's own
/// `profiles/repo_name` file, first line): walks up from `pkg_dir`
/// (`<category>/<package>`) through every ancestor, returning the first
/// `profiles/repo_name` found. `None` when no ancestor has one at all
/// (e.g. a standalone ebuild file outside any repo checkout).
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

/// Hex MD5 of a file's bytes. Shared with the remote-merge server
/// (`remote_bundle::build_bundle` writes `build-info/BINPKGMD5`, backlog
/// #170): the same digest the local `merge_binpkg` records, so a later
/// index rebuild sees a still-current instance either way.
pub(crate) fn md5_hex(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(md5_hex_bytes(&data))
}

/// Real `dblink._format_contents_line`: `<type> <path>[ <md5>| -> <target>][ <mtime>]\n`.
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

/// `pub(crate)`: `ebuild_unmerge`'s own mtime-staleness check
/// (`remove_contents`) reuses this exact conversion to compare a live
/// file's current mtime against a `CONTENTS`-recorded one.
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

/// Walks `d` (real `${D}`) and merges every entry into `root` (real
/// `${ROOT}`), returning the accumulated real `CONTENTS` text (see this
/// module's own doc comment for the exact line format and the v1 scope
/// cuts -- no chown, sorted-by-name traversal order). `cfgfiledict` is
/// read once by the caller before this runs and written back once after
/// -- real `vardbapi._conf_mem_file` semantics (a single, whole-merge
/// read/update/write, not a per-file one).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn merge_tree(
    d: &Path,
    root: &Path,
    category: &str,
    installed_instance_pf: Option<&str>,
    protect_if_modified: bool,
    config_protect: &str,
    config_protect_mask: &str,
    noconfmem: bool,
    cfgfiledict: &mut BTreeMap<String, String>,
    xattrs: &XattrPolicy,
) -> Result<String, String> {
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
                // Real dblink._protect(): a CONFIG_PROTECT'd `sym` entry
                // whose real, live destination differs is diverted to a
                // fresh ._cfgNNNN_ sibling (real bug #485598: the target
                // string's own MD5 is what's hashed, not file content) --
                // `protect_decision` computes the comparison from the
                // dest's own real on-disk type, whatever it actually is
                // (see that function's own doc comment).
                let mut write_dest = dest.to_path_buf();
                let mut moveme = true;
                let protected_path = is_protected(root, config_protect, config_protect_mask, &dest);
                if protected_path {
                    let src_md5 = md5_hex_bytes(target_str.as_bytes());
                    (write_dest, moveme) = protect_decision(
                        root,
                        category,
                        installed_instance_pf,
                        protect_if_modified,
                        &dest,
                        &abs_path,
                        &src_md5,
                        cfgfiledict,
                        noconfmem,
                    )?;
                }
                // Real `mergeme()`'s own symlink-over-directory branch
                // (`vartree.py`: `if mydmode is not None and S_ISDIR and
                // not protected`): a symlink can never replace a real
                // directory -- the symlink lands at the first
                // `dest.backup.NNNN` instead and the directory is kept.
                // This is the merge-time half of the `find_collisions`
                // exclusion above: directory-target symlinks are never
                // collision-checked (real `os.walk` puts them in `dirs`),
                // so they always reach here, and real still exits 0.
                // (File-target symlinks never reach here: they always
                // aborted in `_collision_protect` first.) `CONTENTS`
                // below still records the logical `abs_path`, like real.
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
                // Real `moveme == false` ("confmem rejected this update",
                // see `protect_decision`'s own doc comment): skip the
                // write entirely, leaving the live destination completely
                // untouched -- `mtime` above still uses the *source's* own
                // mtime for CONTENTS below either way, matching real
                // `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (set
                // before the `if moveme:` gate, never touched when it's
                // skipped, `vartree.py:5403`/`5547`).
                if moveme {
                    if let Some(parent) = write_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    // Real `movefile()` installs a symlink by an atomic
                    // rename too -- see `replace_symlink_atomic`'s own
                    // doc comment (backlog #96).
                    replace_symlink_atomic(&target, &src, &write_dest, mtime)?;
                }
                // Real CONTENTS always records the package's own logical
                // path/target (`abs_path`/`target_str`), never the
                // ._cfgNNNN_ variant a protected write may have actually
                // landed at -- same "logical path, not the protect-file
                // path" rule the `obj` branch below documents.
                contents.push_str(&format_contents_line(
                    "sym",
                    &abs_path,
                    None,
                    Some(&target_str),
                    Some(mtime),
                ));
            } else if file_type.is_dir() {
                // Real `mergeme()`: a *newly created* directory gets
                // `os.chmod`/`os.chown`ed to the source's own recorded
                // mode/owner (`vartree.py:5865-5867`); a destination that
                // already exists as a directory is left completely alone
                // (the `--- {mydest}/` "kept as-is" branch, `:5808-5810`)
                // -- an existing shared directory (e.g. `/usr/lib`) must
                // never have its ownership clobbered by every package
                // that happens to also install into it.
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
                // Real dblink._protect(): a protected path whose real
                // on-disk content differs from what's about to be merged
                // gets diverted to a fresh ._cfgNNNN_ sibling instead of
                // overwritten -- unless cfgfiledict already remembers
                // this exact src_md5 as a previously-offered update for
                // this path (real "--noconfmem off" default: apply it
                // directly, don't re-protect; `NOCONFMEM` forces
                // re-protection regardless of memory, real `cfgfiledict[
                // "IGNORE"]`).
                let mut write_dest = dest.to_path_buf();
                let mut moveme = true;
                if is_protected(root, config_protect, config_protect_mask, &dest) {
                    (write_dest, moveme) = protect_decision(
                        root,
                        category,
                        installed_instance_pf,
                        protect_if_modified,
                        &dest,
                        &abs_path,
                        &src_md5,
                        cfgfiledict,
                        noconfmem,
                    )?;
                }

                let mtime = mtime_secs(
                    &std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?,
                )?;
                // Real `moveme == false` ("confmem rejected this update",
                // see `protect_decision`'s own doc comment): skip the copy
                // entirely, leaving the live destination completely
                // untouched -- `mtime` above still uses the *source's* own
                // mtime for CONTENTS below either way, matching real
                // `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (set
                // before the `if moveme:` gate, never touched when it's
                // skipped, `vartree.py:5403`/`5749`).
                if moveme {
                    if let Some(parent) = write_dest.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    // Real `mergeme()`'s `if self._needs_move(mysrc,
                    // mydest, mymode, mydmode)` gate (bug #722270): a
                    // destination that already is a regular file with
                    // the same mode and byte-identical content is left
                    // completely in place -- inode, ownership and xattrs
                    // survive the rebuild, and only the mtime is
                    // refreshed (`os.utime(mydest, ns=(mymtime,
                    // mymtime))`). A diverted `._cfgNNNN_` write never
                    // takes this path: real clears `mydmode` whenever
                    // `_protect` diverts (`vartree.py:5645-5647`), so it
                    // always moves there, and `write_dest != dest` says
                    // the same thing here.
                    if write_dest == dest && !needs_move(&src, &write_dest, xattrs) {
                        filetime::set_file_mtime(
                            &write_dest,
                            filetime::FileTime::from_unix_time(mtime, 0),
                        )
                        .map_err(|e| format!("{}: {e}", write_dest.display()))?;
                    } else {
                        // Real `movefile()`'s atomic replacement -- the
                        // destination's existing inode is never opened for
                        // write (backlog #96): see
                        // `replace_file_atomic`'s own doc comment for why
                        // that is a safety requirement, not an
                        // optimization.
                        replace_file_atomic(&src, &write_dest, mtime, xattrs)?;
                    }
                }
                // Real CONTENTS always records the package's own logical
                // path (`abs_path`) and the *source*'s own MD5 -- never
                // the ._cfgNNNN_ variant a protected write may have
                // actually landed at (real dblink.mergeme(): `abs_path=
                // myrealdest, md5_digest=mymd5`, both computed before
                // `_protect()` ever runs). The vdb still considers this
                // package the owner of the *logical* path either way.
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
                // Real `mergeme()`'s own `else:` branch ("we are merging a
                // fifo or device node", `vartree.py:5787-5811`): never
                // `_protect()`'d (this branch doesn't call it at all,
                // unlike `obj`/`sym` above), and only actually created
                // when the live destination doesn't already exist yet
                // (real `if mydmode is None:`) -- an existing node at that
                // path is left completely alone, matching real portage's
                // own conservative "don't touch a device/fifo that's
                // already there" behavior. The `CONTENTS` line is written
                // unconditionally either way (real `_format_contents_line`
                // call sits *outside* that `if`), with no digest/mtime/
                // target field at all (real `abs_path=myrealdest` only).
                //
                // Real `movefile()` has no dedicated fifo/device-node
                // logic of its own -- an ordinary `os.rename()` just works
                // for a special file too, since `rename(2)` doesn't care
                // what type of file it's moving (real `movefile()`'s own
                // comment: "we don't yet handle special, so we need to
                // fall back to /bin/mv" only fires on a genuine cross-
                // device `EXDEV` failure). Portuale's own merge step
                // never moves `${D}` content though (every other branch
                // above copies/recreates instead, so `${D}` itself stays
                // intact) -- recreating a fresh node at `write_dest` via
                // real `mkfifo(3)`/`mknod(3)` (matching the source's own
                // real type, permission bits, and -- for a device node --
                // major/minor) is the equivalent "copy" here, the same
                // "recreate, don't move" shape the `sym` branch above
                // already established for symlinks.
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

/// Creates a fresh FIFO or device node at `dest`, matching `src`'s own
/// real type, permission bits, and (for a device) real major/minor
/// (`st_rdev`) -- the "recreate, don't move" equivalent of real
/// `movefile()`'s ordinary same-device `rename(2)` for a special file
/// (see `merge_tree`'s own `fif`/`dev` branch doc comment for why this
/// portuale recreates rather than moves). `mkfifo(3)`/`mknod(3)` both apply
/// the process umask to the mode given, unlike `std::fs::copy`'s own
/// automatic exact permission-bit preservation for a regular file -- an
/// explicit `chmod` afterward closes that gap, so a real, non-default
/// source mode (e.g. `0600`) survives regardless of this process's own
/// umask.
///
/// Real `mknod(2)` genuinely requires root/`CAP_MKNOD` for a real
/// (nonzero major:minor) character or block device -- an unprivileged
/// caller merging a real device node from `${D}` (itself only possible
/// because a privileged build process, e.g. real `udev`, put it there)
/// hits this same real permission wall, surfaced here as an ordinary
/// `Result::Err` rather than a panic.
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

/// Real `movefile()`'s ownership preservation: `lchown(dest, sstat.
/// st_uid, sstat.st_gid)` for a symlink (real `movefile.py:255`, via
/// `portage.data.lchown` -- a thin `os.lchown` wrapper, `data.py:18`),
/// `os.chown(dest, sstat.st_uid, sstat.st_gid)` for anything else (real
/// `_apply_stat`, `movefile.py:28-30`) -- both set `dest`'s owner/group
/// to match the *source*'s own recorded uid/gid from `${D}` (whatever
/// the build phases, or an ebuild's own `fowners`, actually left
/// there).
///
/// Real portage runs merges as root in normal operation, so this always
/// succeeds there; it's not gated on "am I root" here either, matching
/// real's own unconditional call with no privilege check at this call
/// site. Run unprivileged with the source already owned by the calling
/// process (the common single-user case `${D}`'s files are almost
/// always in), `chown(path, self_uid, self_gid)` is permitted regardless
/// and still succeeds. Only a genuinely privileged ownership change
/// attempted without root hits the same real `EPERM` real portage would
/// -- propagated here as an ordinary merge failure, exactly like every
/// other I/O error in this function, never silently swallowed.
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

/// Real `dblink._needs_move` (`vartree.py:6363`): `true` unless the
/// destination already exists as a regular file with the same full mode
/// and byte-identical content. When it returns `false`, real's
/// `mergeme` does not call `movefile` at all (`:5916`'s
/// `if self._needs_move(...)` gate, bug #722270) -- the destination's
/// inode, ownership and xattrs survive the rebuild and only its mtime is
/// refreshed, which is exactly why the L3 ncurses case keeps `1:1`:
/// the stage3 image's `curses.h`/terminfo are byte-identical to the
/// rebuilt ones, so real leaves them alone while a fresh copy would
/// come out `0:0`.
///
/// Under `FEATURES=xattr` real also compares xattrs (`_cmpxattr`,
/// `PORTAGE_XATTR_EXCLUDE`-aware, `vartree.py:6376-6379`):
/// [`xattrs_equal`] (#329). Real's `filecmp.cmp(shallow=False)` is a byte compare after
/// the mode check; [`files_equal`] does the same in fixed chunks.
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

/// Byte-for-byte comparison for [`needs_move`] (real
/// `filecmp.cmp(shallow=False)`, which also short-circuits on a size
/// mismatch first). Chunked so a large distfile-scale object never
/// materializes in memory twice.
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

/// `read` until `buf` is full or EOF (a plain `Read::read` may return a
/// short count for regular files too, which would otherwise misalign the
/// chunk comparison).
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
