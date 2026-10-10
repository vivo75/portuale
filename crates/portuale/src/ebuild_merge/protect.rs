//! CONFIG_PROTECT: protected paths, ._cfg names, atomic replaces, xattrs.

use crate::ebuild_merge::*;

/// Real `ConfigProtect.isprotected()` (`lib/portage/util/__init__.py`):
/// longest-prefix match against `config_protect` (a whitespace-separated
/// path list, `root`-joined) minus `config_protect_mask`. A protect/mask
/// entry that names a real, on-disk directory matches any path under it
/// (`/etc` matches `/etc/foo` but not `/etcfoo`); one that doesn't (a
/// literal file, or a path that doesn't exist at all) only ever matches
/// exactly. `pub(crate)`: `ebuild_unmerge`'s own `FEATURES=unmerge-orphans`
/// handling reuses this exact real check (real `self.isprotected(obj)`).
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

/// Real `new_protect_filename()` (`lib/portage/util/__init__.py:1803`):
/// the next unused `._cfgNNNN_<basename>` sibling of `dest`, unless the
/// *highest-numbered existing* `._cfgNNNN_<basename>` sibling already
/// holds the same content/target as `newmd5` -- in which case that
/// existing file is reused instead of allocating a new one. `newmd5`
/// mirrors real `_protect()`'s own `(dest_link or src_md5)` call
/// argument: the pending update's own content MD5 for a regular file, or
/// (for a symlink) the *comparison* target-string this call site chose
/// -- real portage's own naming is misleading here, "newmd5" folds both
/// "an MD5 hex string" and "a raw symlink target string" into the same
/// parameter, compared against whichever kind the last `._cfgNNNN_` file
/// itself turns out to be. `dest` itself is never touched here, the
/// caller writes to the returned path instead. Narrower than real in one
/// way: real `new_protect_filename(mydest, newmd5, force)` returns
/// `mydest` unchanged outright when `mydest` doesn't exist yet and
/// `force` is false -- moot here since every call site already only
/// calls this after confirming `dest` exists (see `merge_tree`'s own obj/
/// sym branches), so `force`'s only real effect (forcing that early
/// return to still allocate a number) never applies and isn't threaded
/// through.
pub(crate) fn new_protect_filename(dest: &Path, newmd5: &str) -> Result<PathBuf, String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{}: has no parent directory", dest.display()))?;
    let basename = dest
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: not a valid filename", dest.display()))?;

    let mut max_num: i64 = -1;
    let mut last_pfile: Option<PathBuf> = None;
    if let Ok(entries) = portage_util::read_dir_entries(parent) {
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix("._cfg")
                && rest.len() > 5
                && rest.as_bytes()[4] == b'_'
                && &rest[5..] == basename
                && let Ok(n) = rest[..4].parse::<i64>()
                && n > max_num
            {
                max_num = n;
                last_pfile = Some(parent.join(entry.file_name()));
            }
        }
    }

    if let Some(old_pfile) = &last_pfile
        && let Ok(meta) = std::fs::symlink_metadata(old_pfile)
    {
        if meta.file_type().is_symlink() {
            if let Ok(target) = std::fs::read_link(old_pfile)
                && target.to_string_lossy() == newmd5
            {
                return Ok(old_pfile.clone());
            }
        } else if meta.is_file()
            && let Ok(md5) = md5_hex(old_pfile)
            && md5 == newmd5
        {
            return Ok(old_pfile.clone());
        }
    }
    Ok(parent.join(format!("._cfg{:04}_{basename}", max_num + 1)))
}

/// Real `dblink._new_backup_path` (`vartree.py:_new_backup_path`): the
/// first `dest.backup.NNNN` (zero-padded 4 digits from `0000`) whose
/// `lstat` fails -- i.e. no file, symlink, or directory there yet.
/// Used by `merge_tree`'s own symlink-over-directory branch below: real
/// `mergeme()` never overwrites a real directory with a symlink, it
/// merges the symlink under the backup name and keeps the directory.
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

/// A free temporary sibling of `dest`, in the same directory (so the
/// final `rename(2)` is always same-filesystem), named like real
/// `movefile()`'s own merge temporary: `.{basename}._portage_merge_.{pid}`
/// (`lib/portage/util/movefile.py:322`'s `NamedTemporaryFile` prefix),
/// with a counter suffix only on the (pid-reuse / parallel-merge)
/// collision case. Checks with `symlink_metadata`, so an existing
/// symlink or directory at the candidate counts as taken.
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

/// Real `_cmpxattr(src, dest, exclude)` (`movefile.py:111-136`): the
/// non-excluded attribute names and values are equal on both files, or
/// the filesystem does not support xattrs at all.
pub(crate) fn xattrs_equal(src: &Path, dest: &Path, exclude: &str) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let (s, d) = (src.as_os_str().as_bytes(), dest.as_os_str().as_bytes());
    let (Ok(src_attrs), Ok(dest_attrs)) =
        (crate::helpers::xattr_list(s), crate::helpers::xattr_list(d))
    else {
        // Real re-raises anything but ENOTSUP; `xattr_list` already maps
        // ENOTSUP to an empty list, so an error here is unreadable --
        // treat it as "differs" so the file is replaced.
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

/// Carries `src`'s xattrs onto the not-yet-renamed temporary `tmp`, the
/// way real's move would have left them on `dest` (see [`XattrPolicy`]).
pub(crate) fn carry_xattrs(
    src: &Path,
    tmp: &Path,
    dest: &Path,
    xattrs: &XattrPolicy,
) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let src_dev = std::fs::metadata(src).map(|m| m.dev()).ok();
    let dest_dev = dest
        .parent()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.dev());
    let (s, t) = (src.as_os_str().as_bytes(), tmp.as_os_str().as_bytes());
    if src_dev.is_some() && src_dev == dest_dev {
        // Real's same-device `rename` keeps every attribute and cannot
        // fail on one, so neither does this copy: a failure only warns.
        if let Err(line) = crate::helpers::copy_xattr(s, t, b"") {
            eprintln!("!!! {}", String::from_utf8_lossy(&line));
        }
        return Ok(());
    }
    if xattrs.enabled {
        crate::helpers::copy_xattr(s, t, xattrs.exclude.as_bytes()).map_err(|line| {
            format!(
                "{}\n!!! Failed to copy extended attributes. In order to avoid this error, set FEATURES=\"-xattr\" in make.conf.",
                String::from_utf8_lossy(&line)
            )
        })?;
    }
    Ok(())
}

/// Real `movefile()`'s own **atomic replacement** for a regular file:
/// copy the source's bytes to [`unique_sibling_path`]'s temporary, apply
/// real `movefile()`'s `_apply_stat` (owner/group, mode) and the
/// source's own mtime, then `rename(2)` it over `dest`. The existing
/// `dest` inode is never opened for write -- see this module's own doc
/// comment ("KNOWN, DOCUMENTED GAPS") and backlog #96 for why that is
/// load-bearing: an in-place `std::fs::copy` truncates the live inode,
/// which is `ETXTBSY` for a running executable but succeeds for an
/// mmap'd shared library, so a `sys-libs/readline` merge rewrote
/// `libreadline.so.8.3` underneath every running bash and a `glibc`
/// merge would rewrite `libc.so.6` underneath the entire system. Real is
/// safe by construction: same-device `os.rename` (`movefile.py:231`),
/// cross-device copy to `dest + "#new"` then rename (`:346`).
///
/// `mtime` is the source's own mtime in seconds (already computed by the
/// caller for `CONTENTS`), so the temporary gets exactly the value the
/// caller will record -- never a second stat that could disagree. On any
/// failure the temporary is removed before returning; the destination is
/// left untouched.
pub(crate) fn replace_file_atomic(
    src: &Path,
    dest: &Path,
    mtime: i64,
    xattrs: &XattrPolicy,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = unique_sibling_path(dest)?;
    let outcome = (|| -> Result<(), String> {
        std::fs::copy(src, &tmp).map_err(|e| format!("{}: {e}", src.display()))?;
        let src_meta = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
        // `std::fs::copy` already carries the mode bits over on Unix;
        // the explicit chmod fixes up a umask-masked temporary. The
        // chown is not redundant -- `std::fs::copy` never touches
        // ownership. See `lchown_or_chown`'s own doc comment.
        lchown_or_chown(&tmp, src_meta.uid(), src_meta.gid(), false)?;
        std::fs::set_permissions(
            &tmp,
            std::fs::Permissions::from_mode(src_meta.permissions().mode()),
        )
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
        filetime::set_file_mtime(&tmp, filetime::FileTime::from_unix_time(mtime, 0))
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        carry_xattrs(src, &tmp, dest, xattrs)?;
        std::fs::rename(&tmp, dest)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), dest.display()))
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    outcome
}

/// Real `movefile()`'s atomic replacement for a symlink
/// (`lib/portage/util/movefile.py:213-265`): create the new link at
/// [`unique_sibling_path`]'s temporary, apply `lchown` + the source's
/// own mtime, then `rename(2)` it over `dest`. The remove-then-symlink
/// pair this replaces could leave the path briefly absent and, more
/// importantly, was the second in-place mutation `merge_tree` had; one
/// rule now covers both branches: never write through an existing
/// inode. `rename(2)` replaces an existing file or symlink atomically
/// (a real directory cannot appear here: the symlink-over-directory case
/// is diverted to `new_backup_path` by the caller).
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

/// Real `dblink._protect()`'s own decision, shared by `merge_tree`'s
/// `obj`/`sym` branches (`vartree.py:5434-5480`/`5831-5901`): `dest_md5`/
/// `dest_link` are computed from the *live destination's own lstat'd
/// on-disk type* -- independent of the incoming source's own type, so a
/// type-changing update (a symlink replacing a previously-installed
/// regular file at the same path, or vice versa) is real-protected too,
/// closing the "like-for-like only" v1 cut this module's own doc comment
/// used to document. `src_md5`/`src_link` mirror real `mymd5`/`myto`:
/// always an MD5-shaped string either way (a symlink source's own is the
/// target string's own MD5, real bug #485598), `src_link` only `Some`
/// for a symlink source. Real `force` (from `dest_link != src_link` on a
/// type mismatch) is deliberately not threaded through, for the exact
/// reason `new_protect_filename`'s own doc comment already gives: it
/// only ever changes behavior when `dest` doesn't exist, and this
/// function -- like every existing call site before it -- only reaches
/// `new_protect_filename` after confirming `dest` exists, *except* the
/// one new case below that deliberately doesn't.
///
/// Real `_installed_instance`/`k = self._installed_instance.
/// _match_contents(dest_real)` (`vartree.py:5849-5866`) is real now too:
/// `installed_instance_pf` (the *previous* same-slot instance this
/// merge is upgrading over, if any) is consulted via `owned_node_value_
/// pf` for whatever it recorded at `abs_path`. Two distinct real
/// behaviors, both gated on a match (`k is not False`):
///   - `dest_mode is None` (the live destination doesn't exist at all --
///     the admin deleted or renamed a path the *previous* package
///     installed): real `force = True`, which (since `_protect()`'s own
///     `if protected and dest_mode is not None:` main block is skipped
///     entirely when `dest_mode is None`, leaving `protected`/`move_me`
///     at their initial `True` values) always diverts into a fresh
///     `._cfgNNNN_` sibling -- bug #523684, prompting the admin instead
///     of silently re-creating a path they deliberately removed.
///   - `dest_mode is not None` and real `FEATURES=config-protect-if-
///     modified` (`protect_if_modified`) is on: if the live destination
///     still matches *exactly* what the previous instance's own real
///     `CONTENTS` recorded (an `obj`'s content MD5, or a `sym`'s own
///     target string), the admin never touched it since that install --
///     so it's not "modified" in the sense this feature cares about,
///     and `protected` is cleared outright, applying the new version's
///     content directly even though it differs from `src`. Distinguishes
///     "this file's own default content changed between package
///     versions" from "the admin hand-edited this file locally", which
///     the plain `src_md5 == dest_md5` comparison below can't tell
///     apart on its own.
///
/// Returns `(write_dest, moveme)` -- real `_protect()` returns three
/// values (`dest, protected, moveme`), but every real call site only
/// ever needs `protected` to decide *whether* to call `_protect()` at
/// all (already handled by this function's own caller, `is_protected`)
/// and `moveme` to decide whether `mergeme()`'s own `if moveme:` gate
/// (`vartree.py:5547`/`5749`) actually performs the file write, so this
/// port only threads those two through. Real `moveme` is `False` in
/// exactly one case here: `already_offered && !noconfmem` (real `move_me
/// = protected = bool(cfgfiledict["IGNORE"])` with `IGNORE == 0`,
/// `vartree.py:5877`) -- "confmem rejected this update"
/// (`mergeme()`'s own `zing = "---"`). Real `cfgfiledict` is deliberately
/// left untouched in that one branch too: reaching it requires `src_md5
/// == cfgfiledict.get(dest_real)[0]` in the first place (that's the very
/// definition of "already offered"), so the trailing real `if move_me:
/// cfgfiledict[dest_real] = [src_md5] elif dest_md5 == cfgfiledict.get
/// (dest_real)[0]: del cfgfiledict[dest_real]` (`vartree.py:5888-5895`)
/// hits neither branch: `move_me` is `False` (skipping the first), and
/// `dest_md5 != src_md5` is already established by the earlier `if
/// src_md5 == dest_md5` check having failed (skipping the second, since
/// it can only match by being equal to `src_md5` too). Every other
/// return path keeps `moveme` `true`, matching real `_protect()`'s own
/// `move_me = True` initial default, never cleared on any other branch.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn protect_decision(
    root: &Path,
    category: &str,
    installed_instance_pf: Option<&str>,
    protect_if_modified: bool,
    dest: &Path,
    abs_path: &str,
    src_md5: &str,
    cfgfiledict: &mut BTreeMap<String, String>,
    noconfmem: bool,
) -> Result<(PathBuf, bool), String> {
    let matched: Option<(String, String)> =
        installed_instance_pf.and_then(|pf| owned_node_value_pf(root, category, pf, abs_path));

    let Ok(dest_meta) = std::fs::symlink_metadata(dest) else {
        // Real `dest_mode is None`.
        if matched.is_some() {
            // Real bug #523684: force-diverts even though there's
            // nothing on disk to compare against yet.
            cfgfiledict.insert(abs_path.to_string(), src_md5.to_string());
            return Ok((new_protect_filename(dest, src_md5)?, true));
        }
        return Ok((dest.to_path_buf(), true));
    };

    let (dest_md5, dest_link): (Option<String>, Option<String>) =
        if dest_meta.file_type().is_symlink() {
            let target = std::fs::read_link(dest)
                .ok()
                .map(|t| t.to_string_lossy().to_string());
            let md5 = target.as_deref().map(|t| md5_hex_bytes(t.as_bytes()));
            (md5, target)
        } else if dest_meta.is_file() {
            (md5_hex(dest).ok(), None)
        } else {
            (None, None)
        };

    if protect_if_modified && let Some((node_type, value)) = &matched {
        let unmodified_since_installed = match node_type.as_str() {
            "obj" => dest_md5.as_deref() == Some(value.as_str()),
            "sym" => dest_link.as_deref() == Some(value.as_str()),
            _ => false,
        };
        if unmodified_since_installed {
            return Ok((dest.to_path_buf(), true));
        }
    }

    if dest_md5.as_deref() == Some(src_md5) {
        return Ok((dest.to_path_buf(), true));
    }

    let already_offered = cfgfiledict.get(abs_path).map(String::as_str) == Some(src_md5);
    if already_offered && !noconfmem {
        return Ok((dest.to_path_buf(), false));
    }

    let newmd5 = dest_link.as_deref().unwrap_or(src_md5);
    let write_dest = new_protect_filename(dest, newmd5)?;
    cfgfiledict.insert(abs_path.to_string(), src_md5.to_string());
    Ok((write_dest, true))
}

/// Real `vardbapi._conf_mem_file`: `<root>/var/lib/portage/config`, a
/// real, persisted "which src MD5 has already been offered for this
/// path" memory (real `grabdict`/`writedict`'s own `"path value\n"`
/// format) -- without it, re-merging an already-protected update would
/// spawn a fresh `._cfgNNNN_` file every single time, even though the
/// admin has already been shown this exact change once. Portuale's
/// own `ebuild` CLI has no `--noconfmem` flag, so behavior always
/// matches real portage's own default (`--noconfmem` off).
///
/// Stored by the root's [`portage_vdb::InstalledDb`] (feat#157 S1.4:
/// `config_memory` / `set_config_memory`; on `files` the same
/// `<root>/var/lib/portage/config` file, read and written exactly as
/// before).
///
/// `pub(crate)`: also read by `ebuild_unmerge::run_unmerge` (real
/// `_unmerge_pkgfiles()`'s own `stale_confmem` cleanup,
/// `vartree.py:2747`/`2931-2932`/`3106-3109` -- a removed file's
/// `_conf_mem_file` entry is dropped once nothing still owns that path).
/// A missing or unreadable store is empty.
pub(crate) fn read_cfgfiledict(root: &Path) -> BTreeMap<String, String> {
    portage_vdb::for_root(root)
        .config_memory()
        .map(|memory| memory.entries)
        .unwrap_or_default()
}

/// Replace the config memory, unconditionally (one `WriteTxn`, committed
/// at once; on `files` a plain in-place write).
pub(crate) fn write_cfgfiledict(root: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_config_memory(&portage_vdb::ConfigMemory {
        entries: map.clone(),
    })
    .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}
