# crates/portuale/src/ebuild_merge/tests/collisions.rs
Collision, stale-entry, blocker and owner-lookup coverage for the merge path.

## fn stale_merging_entry_is_invisible_and_replaced_by_the_next_merge
- Backlog #183: a stale `-MERGING-<pf>` entry left by a killed merge is invisible to every portuale vdb reader, and the next merge of the same package wipes it and publishes the Portage entry -- Portage's `dblink.treewalk()`'s own `self.dbdir = self.dbtmpdir; self.delete(); ensure_dirs(self.dbtmpdir)` prologue plus the `vardbapi._excluded_dirs` reader skip (`vartree.py`).
- Ground truth: the `l32` C4 control cell (`findings/l5.md` Group 4) leaves exactly `-MERGING-slow-a-1.0` after the SIGKILL, no `slow-a-1.0`, and the resumed merge reinstalls from scratch.
- Seed: a killed merge's leftovers, a half-written `-MERGING-` entry for the package about to be merged and nothing live. Every reader skips it: not an installed package, owns no path, indexes no sonames. The next merge wipes the stale entry and publishes the live one, leaving no temporary behind.

## fn directory_target_symlink_over_a_real_directory_is_ignored_and_backed_up
- Portage's `os.walk` puts a symlink pointing to a directory into `dirs`, never into `filelist`/`linklist` (`vartree.py:4625-4681` comment) -- so it is never collision-checked.
- Seed shape: the linux-firmware `nvidia/ad10x` shape (new image symlinks, live filesystem directories). Yields no `symlink_collisions` entry, and `merge_tree` lands the symlink at the first `dest.backup.NNNN` (Portage's `mergeme()`'s own symlink-over-directory branch) keeping the directory, exiting 0 -- even with `protect_owned` on (the reporter's own `FEATURES`, which has `protect-owned` but not `collision-protect`) and an additional unclaimed ordinary collision present (which alone never aborts under `protect-owned` either).
- The unclaimed ordinary file is merged over, like Portage's "merged despite file collisions" path.

## fn blockers_from_flat_deps_matches_only_blocker_atoms_against_the_vdb
- `blockers_from_flat_deps` is the `blockers` term of Portage's `mypkglist = others_in_slot + blockers`, factored out of `blocked_installed_packages` for `merge_binpkg`'s own use.
- #116: the expectations are the pre-slice ones, unchanged.

## fn find_owners_reads_contents_through_the_packages_db_seam
- Reads through the `mrg_director::PackagesDb` seam (`VdbReader::contents_files`), which keys by a split `(package, version)` and strips the `CONTENTS` record's one leading `/`. Pins the two conversions the production path performs on a hand-built vdb entry: the directory name's `pf` splits back into that key, and the logical absolute collision path meets the stripped contents path while the returned owner key/values keep the old `category/pf` + absolute path shape exactly.
