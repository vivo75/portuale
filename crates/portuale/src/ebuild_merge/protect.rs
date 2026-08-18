//! CONFIG_PROTECT: protected paths, ._cfg names, atomic replaces, xattrs.

use crate::ebuild_merge::*;

/// Portage's `ConfigProtect.isprotected`: longest-prefix match of `dest`
/// against `config_protect` minus `config_protect_mask`. A directory entry
/// matches everything under it; anything else matches exactly.
/// Portage: lib/portage/util/__init__.py, ConfigProtect.isprotected
pub(crate) fn is_protected(
    root: &Path,
    config_protect: &str,
    config_protect_mask: &str,
    dest: &Path,
) -> bool {
    fn longest_match(root: &Path, list: &str, dest: &Path) -> usize {
        let dest_str = dest.to_string_lossy();
        let mut best = 0;
        for entry in list.split_whitespace() {
            let ppath = root.join(entry.trim_start_matches('/'));
            let ppath_str = ppath.to_string_lossy().trim_end_matches('/').to_string();
            let is_dir = ppath.is_dir();
            let matched = if is_dir {
                dest_str == ppath_str.as_str() || dest_str.starts_with(&format!("{ppath_str}/"))
            } else {
                dest_str == ppath_str.as_str()
            };
            if matched && ppath_str.len() > best {
                best = ppath_str.len();
            }
        }
        best
    }
    let protected_len = longest_match(root, config_protect, dest);
    if protected_len == 0 {
        return false;
    }
    protected_len > longest_match(root, config_protect_mask, dest)
}

/// Portage's `dblink._new_backup_path`: the first `dest.backup.NNNN` (from
/// `0000`) with no file, symlink or directory yet. Used where a symlink
/// would otherwise overwrite a directory.
/// Portage: lib/portage/dbapi/vartree.py, dblink._new_backup_path
pub(crate) fn new_backup_path(dest: &Path) -> PathBuf {
    let dest_str = dest.display().to_string();
    let mut n = 0;
    loop {
        let candidate = PathBuf::from(format!("{dest_str}.backup.{n:04}"));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
        n += 1;
    }
}

/// A free temporary sibling of `dest` (same directory, so the final rename
/// is same-filesystem), named like Portage's `movefile` temporary, with a
/// counter suffix on collision. Symlinks and directories count as taken.
pub(crate) fn unique_sibling_path(dest: &Path) -> Result<PathBuf, String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", dest.display()))?;
    let name = dest
        .file_name()
        .ok_or_else(|| format!("{}: not a valid filename", dest.display()))?
        .to_string_lossy();
    let base = parent.join(format!(".{name}._portage_merge_.{}", std::process::id()));
    if std::fs::symlink_metadata(&base).is_err() {
        return Ok(base);
    }
    for n in 0.. {
        let candidate = parent.join(format!(
            ".{name}._portage_merge_.{}.{n}",
            std::process::id()
        ));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    unreachable!("the counter loop always finds a free name")
}

/// Portage's `_cmpxattr`: the non-excluded attribute names and values match
/// on both files, or the filesystem has no xattr support.
/// Portage: lib/portage/util/movefile.py, _cmpxattr
pub(crate) fn xattrs_equal(src: &Path, dest: &Path, exclude: &str) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let (s, d) = (src.as_os_str().as_bytes(), dest.as_os_str().as_bytes());
    let (Ok(src_attrs), Ok(dest_attrs)) =
        (crate::helpers::xattr_list(s), crate::helpers::xattr_list(d))
    else {
        // `xattr_list` already maps ENOTSUP to empty, so an error here is
        // unreadable: treat as "differs" so the file is replaced.
        return false;
    };
    let keep = |attrs: Vec<Vec<u8>>| -> std::collections::BTreeSet<Vec<u8>> {
        attrs
            .into_iter()
            .filter(|a| !crate::helpers::xattr_excluded(exclude, a))
            .collect()
    };
    let (src_attrs, dest_attrs) = (keep(src_attrs), keep(dest_attrs));
    src_attrs == dest_attrs
        && src_attrs
            .iter()
            .all(|a| crate::helpers::xattr_get(s, a).ok() == crate::helpers::xattr_get(d, a).ok())
}

/// Portage's `movefile` atomic replacement for a symlink: create the link
/// at a temporary, apply `lchown` and mtime, then rename over `dest`; the
/// path is never briefly absent. A directory here is the caller's backup case.
/// Portage: lib/portage/util/movefile.py, movefile
pub(crate) fn replace_symlink_atomic(
    target: &Path,
    src: &Path,
    dest: &Path,
    mtime: i64,
) -> Result<(), String> {
    let tmp = unique_sibling_path(dest)?;
    let outcome = (|| -> Result<(), String> {
        std::os::unix::fs::symlink(target, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let src_meta =
            std::fs::symlink_metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
        lchown_or_chown(&tmp, src_meta.uid(), src_meta.gid(), true)?;
        let ft = filetime::FileTime::from_unix_time(mtime, 0);
        filetime::set_symlink_file_times(&tmp, ft, ft)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dest)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), dest.display()))
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    outcome
}
