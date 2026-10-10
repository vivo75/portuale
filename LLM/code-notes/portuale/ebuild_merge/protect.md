# crates/portuale/src/ebuild_merge/protect.rs
CONFIG_PROTECT decisions, `._cfgNNNN_` allocation, atomic replaces and xattr comparison.

## fn is_protected
- Portage `ConfigProtect.isprotected` (lib/portage/util/__init__.py): longest-prefix match against `config_protect` (a whitespace-separated path list, `root`-joined) minus `config_protect_mask`.
- A protect/mask entry naming a real on-disk directory matches any path under it (`/etc` matches `/etc/foo` but not `/etcfoo`); one that doesn't (a literal file, or a nonexistent path) only ever matches exactly.
- `pub(crate)`: `ebuild_unmerge`'s `FEATURES=unmerge-orphans` handling reuses this exact check (Portage `self.isprotected(obj)`).
- Portage `ConfigProtect.isprotected()` (`lib/portage/util/__init__.py`).

## fn new_protect_filename
- Portage `new_protect_filename` (lib/portage/util/__init__.py:1803): the next unused `._cfgNNNN_<basename>` sibling of `dest`, unless the highest-numbered existing sibling already holds the same content/target as `newmd5`, which is then reused instead of allocating a new one.
- `newmd5` mirrors Portage `_protect()`'s own `(dest_link or src_md5)` call argument: the pending update's content MD5 for a regular file, or (for a symlink) the comparison target-string the call site chose. Portage's own naming folds both "an MD5 hex string" and "a raw symlink target string" into the same parameter, compared against whichever kind the last `._cfgNNNN_` file itself turns out to be.
- Narrower than Portage in one way: Portage `new_protect_filename(mydest, newmd5, force)` returns `mydest` unchanged outright when `mydest` doesn't exist yet and `force` is false -- moot here since every call site only calls this after confirming `dest` exists (see `merge_tree`'s obj/sym branches), so `force`'s only effect (forcing that early return to still allocate a number) never applies and isn't threaded through.
- `dest` itself is never touched here; the caller writes to the returned path instead.
- Portage `new_protect_filename()` (`lib/portage/util/__init__.py:1803`).

## fn new_backup_path
- Portage `dblink._new_backup_path` (vartree.py): the first `dest.backup.NNNN` (zero-padded 4 digits from `0000`) whose `lstat` fails -- no file, symlink, or directory there yet.
- Used by `merge_tree`'s symlink-over-directory branch: Portage `mergeme()` never overwrites a real directory with a symlink; it merges the symlink under the backup name and keeps the directory.
- Portage `dblink._new_backup_path` (`vartree.py:_new_backup_path`).

## fn unique_sibling_path
- Named like Portage `movefile()`'s own merge temporary: `.{basename}._portage_merge_.{pid}` (lib/portage/util/movefile.py:322's `NamedTemporaryFile` prefix), with a counter suffix only on the (pid-reuse / parallel-merge) collision case.
- Checks with `symlink_metadata`, so an existing symlink or directory at the candidate counts as taken. Same directory, so the final `rename(2)` is always same-filesystem.

## fn xattrs_equal
- Portage `_cmpxattr(src, dest, exclude)` (movefile.py:111-136).
- Portage re-raises anything but ENOTSUP.

## fn carry_xattrs
- Portage's same-device `rename` keeps every attribute and cannot fail on one, so neither does this copy: a failure only warns (`eprintln!("!!! ...")`).
- Cross-device failure message quotes Portage's text: `Failed to copy extended attributes. In order to avoid this error, set FEATURES="-xattr" in make.conf.`

## fn replace_file_atomic
- Portage `movefile()` (lib/portage/util/movefile.py): same-device `os.rename` (movefile.py:231); cross-device copy to `dest + "#new"` then rename (:346); `_apply_stat` sets owner/group/mode.
- The existing `dest` inode is never opened for write -- the module doc's "KNOWN, DOCUMENTED GAPS" and backlog #96, and load-bearing: an in-place `std::fs::copy` truncates the live inode, which is `ETXTBSY` for a running executable but succeeds for an mmap'd shared library, so a `sys-libs/readline` merge rewrote `libreadline.so.8.3` underneath every running bash and a `glibc` merge would rewrite `libc.so.6` underneath the entire system. Portage is safe by construction.
- `mtime` is the source's own mtime in seconds, already computed by the caller for `CONTENTS`, so the temporary gets exactly the value the caller will record -- never a second stat that could disagree.
- `std::fs::copy` already carries the mode bits over on Unix; the chown is not redundant (`copy` never touches ownership; see `lchown_or_chown`).

## fn replace_symlink_atomic
- Portage `movefile()` (lib/portage/util/movefile.py:213-265): create the new link at the temporary, apply `lchown` + the source's mtime, then `rename(2)` it over `dest`.
- Replaces the remove-then-symlink pair, which could leave the path briefly absent and, more importantly, was the second in-place mutation `merge_tree` had. One rule now covers both branches: never write through an existing inode. `rename(2)` replaces an existing file or symlink atomically.

## fn protect_decision
- Portage `dblink._protect`'s decision, shared by `merge_tree`'s `obj`/`sym` branches (vartree.py:5434-5480 / 5831-5901).
- `dest_md5`/`dest_link` are computed from the live destination's own lstat'd on-disk type, independent of the incoming source's type: a type-changing update (a symlink replacing a previously-installed regular file at the same path, or vice versa) is protected too, closing the "like-for-like only" v1 cut the module doc used to document.
- `src_md5`/`src_link` mirror Portage `mymd5`/`myto`: always an MD5-shaped string either way (a symlink source's is the target string's MD5 -- Portage bug #485598); `src_link` only `Some` for a symlink source.
- Portage `force` (from `dest_link != src_link` on a type mismatch) is deliberately not threaded through: it only ever changes behavior when `dest` doesn't exist, and this function only reaches `new_protect_filename` after confirming `dest` exists -- except the missing-destination case below that deliberately doesn't (see `new_protect_filename`).
- Portage `_installed_instance._match_contents(dest_real)` (vartree.py:5849-5866): `installed_instance_pf` (the previous same-slot instance this merge upgrades over, if any) is consulted via `owned_node_value_pf` for whatever it recorded at `abs_path`. Two behaviors, both gated on a match (`k is not False`):
  - `dest_mode is None` (the live destination doesn't exist at all -- the admin deleted or renamed a path the previous package installed): Portage `force = True`; since `_protect()`'s `if protected and dest_mode is not None:` main block is skipped entirely, `protected`/`move_me` keep their initial `True` values and the merge diverts into a fresh `._cfgNNNN_` sibling -- bug #523684, prompting the admin instead of silently re-creating a deliberately removed path.
  - `dest_mode is not None` with Portage `FEATURES=config-protect-if-modified` on: if the live destination still matches exactly what the previous instance's `CONTENTS` recorded (an `obj`'s content MD5, or a `sym`'s target string), the admin never touched it since that install -- not "modified" in the feature's sense -- so `protected` is cleared outright and the new version's content applies directly even though it differs from `src`. Distinguishes "this file's default content changed between package versions" from "the admin hand-edited this file locally", which the plain `src_md5 == dest_md5` comparison can't tell apart on its own.
- Returns `(write_dest, moveme)`: Portage `_protect()` returns three values (`dest, protected, moveme`), but every Portage call site only needs `protected` (decide whether to call `_protect()` at all, handled here by the caller `is_protected`) and `moveme` (`mergeme()`'s `if moveme:` gate, vartree.py:5547/5749). `moveme` is `False` in exactly one case: `already_offered && !noconfmem` (Portage `move_me = protected = bool(cfgfiledict["IGNORE"])` with `IGNORE == 0`, vartree.py:5877 -- "confmem rejected this update", `mergeme()`'s `zing = "---"`). Portage `cfgfiledict` is deliberately left untouched in that branch too: reaching it requires `src_md5 == cfgfiledict.get(dest_real)[0]` (the very definition of "already offered"), so the trailing `if move_me: cfgfiledict[dest_real] = [src_md5] elif dest_md5 == cfgfiledict.get(dest_real)[0]: del cfgfiledict[dest_real]` (vartree.py:5888-5895) hits neither branch: `move_me` `False` skips the first, and `dest_md5 != src_md5` (established by the earlier `src_md5 == dest_md5` check failing) skips the second, which can only match by equaling `src_md5` too. Every other return path keeps `moveme` `true`, matching Portage `_protect()`'s `move_me = True` initial default, never cleared on any other branch.
- Portage `dblink._protect()`'s own decision, shared by `merge_tree`'s `obj`/`sym` branches (`vartree.py:5434-5480`/`5831-5901`).

## fn read_cfgfiledict
- Portage `vardbapi._conf_mem_file`: `<root>/var/lib/portage/config`, a persisted "which src MD5 has already been offered for this path" memory in `grabdict`/`writedict`'s `"path value\n"` format. Without it, re-merging an already-protected update would spawn a fresh `._cfgNNNN_` file every time, even though the admin was already shown this exact change once.
- Portuale's `ebuild` CLI has no `--noconfmem` flag, so behavior always matches Portage's default (`--noconfmem` off).
- Stored by the root's `portage_vdb::InstalledDb` (feat#157 S1.4: `config_memory` / `set_config_memory`; on `files` the same `<root>/var/lib/portage/config` file, read and written exactly as before).
- Also read by `ebuild_unmerge::run_unmerge` (Portage `_unmerge_pkgfiles()`'s stale-confmem cleanup, vartree.py:2747/2931-2932/3106-3109 -- a removed file's `_conf_mem_file` entry is dropped once nothing still owns that path). A missing or unreadable store is empty.
- Portage `vardbapi._conf_mem_file`: a real, persisted "which src MD5 has already been offered for this path" memory (Portage `grabdict`/`writedict`'s own `"path value\n"` format). `_unmerge_pkgfiles()`'s own `stale_confmem` cleanup (`vartree.py:2747`/`2931-2932`/`3106-3109`).
