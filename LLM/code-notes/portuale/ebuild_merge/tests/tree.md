# crates/portuale/src/ebuild_merge/tests/tree.rs
`merge_tree` unit tests: atomic replacement, xattrs, ownership, config protection, special nodes.

## fn merge_tree_carries_xattrs_like_portage_movefile
- #329. Same device: Portage renames, so every attribute survives, excluded patterns and `-xattr` included. Cross device: Portage copies, then `_copyxattr`s only under `FEATURES=xattr`, minus `PORTAGE_XATTR_EXCLUDE` (`movefile.py:326-358`). `_needs_move` (`vartree.py:6376-6379`) re-merges a byte-identical file whose xattrs differ, but only under `FEATURES=xattr`.

## fn merge_tree_never_writes_the_existing_inode
- Backlog #96's core invariant. An in-place `std::fs::copy` fails it (the open fd would read the new bytes); the rename-based `replace_file_atomic` passes. This is the property that keeps an mmap'd `libc.so.6`/`libreadline.so.8` alive through its own merge, where `ETXTBSY` does not protect the running process.

## fn merge_tree_replaces_a_running_executable_atomically
- The live shape of backlog #96: a package upgrading a binary that is currently executing (`/bin/bash` on every shell upgrade). The old code's `std::fs::copy` hit `ETXTBSY` and aborted the merge mid-copy, leaving the new files unowned and the vdb unwritten. `/bin/sleep` stands in for the shell: it is the same execve-protected kernel path.

## fn merge_tree_preserves_ownership_from_the_source_when_root
- Ports Portage's movefile `lchown`/`chown` (symlink, regular file) and mergeme's directory `chown`/`chmod` (a newly created directory only); see `lchown_or_chown`'s doc comment. Only exercisable as root, the same precondition the feature itself has; skipped (not failed) otherwise rather than asserting something no local environment could satisfy.
- An already-existing directory is left alone (Portage mergeme's "kept as-is" branch, `vartree.py:5808-5810`).

## fn merge_tree_protects_a_changed_file_under_a_protected_path
- CONTENTS records the logical path with the new content's MD5 (Portage `dblink.mergeme()`'s behaviour; see `merge_tree`'s doc comment).

## fn merge_tree_protect_if_modified_applies_directly_when_dest_still_matches_the_installed_instance
- Ports Portage's `_installed_instance`/`protect_if_modified` (`vartree.py:5849-5866`).

## fn merge_tree_still_protects_a_locally_modified_file_despite_protect_if_modified
- Same setup as the test above, but the live destination no longer matches what the installed instance recorded, so the admin did modify it locally.

## fn merge_tree_force_protects_a_path_the_installed_instance_recorded_but_the_admin_deleted
- Portage bug #523684 (`vartree.py:5852-5859`): with nothing on disk to compare content against, `force = True` still diverts into a fresh `._cfgNNNN_` sibling instead of silently re-creating the path the admin deliberately removed.

## fn merge_tree_remembers_an_already_offered_update_and_leaves_the_live_file_untouched
- Ports Portage's `move_me = protected = bool(cfgfiledict["IGNORE"])` with `IGNORE == 0` (`vartree.py:5877`, "confmem rejected this update"). See `protect_decision`'s doc comment for the full trace.
- CONTENTS still records the new content's source MD5: Portage `mergeme()`'s `mymtime = mystat.st_mtime_ns` (the source's mtime, set before the `if moveme:` gate and untouched when skipped) flows into `_format_contents_line` regardless of `moveme`.

## fn merge_tree_noconfmem_reprotects_an_already_offered_update
- Unlike the default (dest directly overwritten, see the test above), `noconfmem=true` (Portage `--noconfmem`/`cfgfiledict["IGNORE"]`) forces re-protection every time. `new_protect_filename`'s "reuse the last file when content already matches" logic (this slice's third piece) reuses `._cfg0000_` rather than spawning a `._cfg0001_` with identical content, so the visible difference from the default is not a new numbered file: the logical path itself is protected instead of overwritten.

## fn merge_tree_protects_a_symlink_source_replacing_a_regular_file_dest
- Ports Portage `dblink._protect()`'s type-independent comparison (`vartree.py:5434-5480`): `dest_md5`/`dest_link` are computed from the live destination's on-disk type regardless of the incoming source's type.

## fn merge_tree_protects_a_regular_file_source_replacing_a_symlink_dest
- Mirror image of the test above: the occupying symlink may be admin-installed or left over from a previous, differently-shaped package version.

## fn create_special_node_reports_a_permission_failure_cleanly_rather_than_panicking
- Confirmed empirically both via a plain standalone `mknod(2)` call and via this very function, as portuale's unprivileged dev/test user. A privilege-free carve-out exists for `mknod(path, S_IFCHR, 0)` specifically (the kernel's overlayfs "whiteout" convention; `dev_t == 0` is never a usable device), which is why the test passes `/dev/null` itself as `src`, not an arbitrary regular file: only a char device's nonzero `rdev` exercises the privileged path.
- Not reproducible as a live end-to-end test in this environment, unlike the `fif` case; this narrower test confirms the permission error surfaces as an ordinary merge failure, not a crash.
- Device-node creation (`mknod(2)` with `S_IFCHR`/`S_IFBLK`) genuinely requires root/`CAP_MKNOD` for a real (nonzero major:minor) device.
