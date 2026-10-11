# crates/portuale/src/ebuild_merge/tree.rs
Merging the image tree onto ROOT. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn repository_name_for
- Was documented as Portage's own mechanism for naming a repo (`layout.conf`'s `repo-name` key aside, the canonical source is a repo's own `profiles/repo_name` file, first line).

## fn md5_hex
- Shared with the remote-merge server: `remote_bundle::build_bundle` writes `build-info/BINPKGMD5` (backlog #170), the same digest the local `merge_binpkg` records, so a later index rebuild sees a still-current instance either way.

## fn mtime_secs
- `pub(crate)`: `ebuild_unmerge`'s own mtime-staleness check (`remove_contents`) reuses this exact conversion to compare a live file's current mtime against a `CONTENTS`-recorded one.

## fn format_contents_line
- Ports `dblink._format_contents_line`.

## fn merge_tree
- Ports `mergeme()` (lib/portage/dbapi/vartree.py) and `vardbapi._conf_mem_file` semantics.
- Dropped as stale: the old doc's "v1 scope cuts -- no chown, sorted-by-name traversal order". The code does chown (via `replace_file_atomic`/`lchown_or_chown`), so "no chown" is no longer true, if it ever was.
- The symlink branch ports `mergeme()`'s symlink-over-directory branch (`vartree.py`: `if mydmode is not None and S_ISDIR and not protected`). It is the merge-time half of the `find_collisions` exclusion: directory-target symlinks are never collision-checked (Portage's `os.walk` puts them in `dirs`), so they always reach here, and Portage still exits 0. File-target symlinks never reach here: they always aborted in `_collision_protect` first. CONTENTS records the logical `abs_path`, like Portage.
- The symlink CONFIG_PROTECT diversion ports `dblink._protect()`. The hashed value is the target string's own MD5, not file content (Portage bug #485598). `protect_decision` computes the comparison from the dest's own on-disk type, whatever it actually is.
- `moveme == false` ("confmem rejected this update", see `protect_decision`) skips the write entirely, leaving the live destination untouched -- `mtime` still uses the *source's* mtime for CONTENTS either way, matching `mergeme()`'s own `mymtime = mystat.st_mtime_ns` (set before the `if moveme:` gate, never touched when skipped, `vartree.py:5403`/`5547` for the symlink branch, `:5403`/`:5749` for the file branch).
- Symlinks install by atomic rename too (`replace_symlink_atomic`, backlog #96, like `movefile()`).
- The file branch ports `dblink._protect()`: diverted unless cfgfiledict already remembers this exact src_md5 as a previously-offered update for this path (Portage's `"--noconfmem off"` default: apply directly, don't re-protect; `NOCONFMEM` forces re-protection regardless of memory, Portage's `cfgfiledict["IGNORE"]`).
- The `_needs_move` gate ports `mergeme()`'s `if self._needs_move(mysrc, mydest, mymode, mydmode)` (Portage bug #722270; `vartree.py:5916`). A diverted `._cfgNNNN_` write never takes the same-file path: Portage clears `mydmode` whenever `_protect` diverts (`vartree.py:5645-5647`), so it always moves there, and `write_dest != dest` says the same thing here.
- The atomic replacement ports `movefile()`: the destination's existing inode is never opened for write (backlog #96 -- a safety requirement, not an optimization; see `replace_file_atomic`).
- CONTENTS always records the package's logical path (`abs_path`) and the *source's* MD5 -- never the `._cfgNNNN_` variant (Portage's `dblink.mergeme()`: `abs_path= myrealdest, md5_digest=mymd5`, both computed before `_protect()` ever runs).
- The fifo/device branch is only actually created when the live destination doesn't already exist yet (Portage `if mydmode is None:`) -- an existing node at that path is left completely alone, matching Portage's own conservative "don't touch a device/fifo that's already there" behavior. The `CONTENTS` line is written unconditionally either way (Portage `_format_contents_line` call sits *outside* that `if`), with no digest/mtime/target field at all (Portage `abs_path=myrealdest` only).
- The fifo/device branch ports `mergeme()`'s `else:` branch ("we are merging a fifo or device node", `vartree.py:5787-5811`). Portage's `movefile()` has no dedicated fifo/device-node logic of its own -- an ordinary `os.rename()` just works for a special file too, since `rename(2)` doesn't care what type of file it's moving (Portage's `movefile()` comment: "we don't yet handle special, so we need to fall back to /bin/mv" only fires on a genuine cross-device `EXDEV` failure). The "recreate, don't move" shape matches the `sym` branch: every other branch copies/recreates instead, so `${D}` itself stays intact.
- The dir branch ports `mergeme()`: a *newly created* directory gets `os.chmod`/`os.chown`ed to the source's recorded mode/owner (`vartree.py:5865-5867`); an existing directory is the `--- {mydest}/` "kept as-is" branch (`:5808-5810`).

## fn create_special_node
- The "recreate, don't move" equivalent of Portage's `movefile()` ordinary same-device `rename(2)` for a special file. `mkfifo(3)`/`mknod(3)` with the source's type, permission bits, and major/minor (`st_rdev`). Both apply the process umask to the mode given, unlike `std::fs::copy`'s own automatic exact permission-bit preservation for a regular file -- an explicit `chmod` afterward closes that gap, so a real, non-default source mode (e.g. `0600`) survives regardless of this process's own umask.
- Portage `mknod(2)` genuinely requires root/`CAP_MKNOD` for a real (nonzero major:minor) character or block device.
- An unprivileged caller merging a device node from `${D}` (only possible because a privileged build process, e.g. `udev`, put it there) hits the same permission wall Portage would, surfaced here as an ordinary `Result::Err` rather than a panic.

## fn lchown_or_chown
- Portage `movefile()`'s ownership preservation: `lchown(dest, sstat.st_uid, sstat.st_gid)` for a symlink (Portage `movefile.py:255`, via `portage.data.lchown`, a thin `os.lchown` wrapper, `data.py:18`), `os.chown(dest, sstat.st_uid, sstat.st_gid)` for anything else (Portage `_apply_stat`, `movefile.py:28-30`): both set `dest` to the source's `st_uid`/`st_gid`, whatever the build phases or the ebuild's own `fowners` left in `${D}`.
- Not gated on "am I root", matching Portage's own unconditional call with no privilege check at this call site. Run unprivileged with the source already owned by the calling process (the common single-user case), `chown(path, self_uid, self_gid)` is permitted regardless and still succeeds; only a genuinely privileged change without root hits the same `EPERM` Portage would.

## fn needs_move
- Ports `dblink._needs_move` (`vartree.py:6363`) and its `:5916` call gate (Portage `if self._needs_move(...)` gate, bug #722270). The L3 ncurses case keeps `1:1` because the stage3 image's `curses.h`/terminfo are byte-identical to the rebuilt ones, so Portage leaves them alone while a fresh copy would come out `0:0`.
- Under `FEATURES=xattr` Portage also compares xattrs (`_cmpxattr`, `PORTAGE_XATTR_EXCLUDE`-aware, `vartree.py:6376-6379`); `xattrs_equal` (backlog #329). Portage's `filecmp.cmp(shallow=False)` is a byte compare after the mode check.

## fn files_equal
- Ports `filecmp.cmp(shallow=False)`.

## fn read_filling
- A plain `Read::read` may return a short count for regular files too.
