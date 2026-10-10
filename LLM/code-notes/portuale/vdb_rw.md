# crates/portuale/src/vdb_rw.rs
The read-write layer of `portuale vdb mount --rw` over the read-only view.

## module
- Origin: backlog #317. Design: `LLM/superpowers/specs/2026-10-05-rw-fuse-vdb-design.md`; plan: `LLM/02.317-rw-fuse.opus.md`; the Portage operation sequences: `LLM/evidence/317-s0-capture.md`.
- S1: volatile names and `mkdir` categories holding no entry yet. S2: staged `-MERGING-<pf>` directories published with one `insert_entry` transaction at the rename. S3: rewrites and removals inside live entries.

## fn is_volatile_name
- The names Portage's lock code creates next to entries (`locks.py` `lockfile` with `wantnewlockfile`, `hardlock_name`, `_lockfile_was_removed`): `<cat>/.<pn>:<slot>.portage_lockfile`, their hardlink checks (`..*.portage_lockfile.hardlock-*`), plus `-inode-test` names.

## enum Parent::Live
- A live entry holds new names Portage creates in it (`write_atomic` temps, `_umask_test` files) until they are renamed onto a field.

## const MERGING
- The prefix Portage gives an entry while it is being merged (`const.py` `MERGING_IDENTIFIER`).

## enum Fh::Live
- Portage's `PORTAGE_UPDATE_ENV` rewrite of `environment.bz2` in `pkg_postinst` arrives as one live rewrite, stored at `close`.

## fn stamp_state
- Portage `_read_metadata_file` (`vartree.py:157-185`) only trusts a file whose `#dir_mtime=` line equals the directory's `st_mtime_ns`.

## fn replace_from_temp
- A rename carries the temp's bytes, mode and mtime, and moves the directory mtime, so a valid `metadata` stamp turns stale, as on disk (Portage's reader then falls back to the field files).

## fn rename
- Portage `vardbapi.move_ent` (`vartree.py:626-683`, #319): the entry directory moves, possibly into another category.
- Live-entry rewrites are S3.

## fn publish
- A rename onto a non-empty directory fails on a filesystem; Portage's `dblink.treewalk` removes the live entry first.

## fn setattr
- `utime` on a category or the root (Portage `_bump_mtime`): accepted, no lasting effect; the view derives those mtimes.

## fn link
- Portage `_lockfile_was_removed` links the lock file to a hardlock name and compares the inodes.

## tests: a_package_move_through_the_view_equals_the_same_move_on_files
- #319: a package move through the mount. The op sequence is Portage's (`move_ent`, strace capture in `testdata/move-ent.ops`); the files run on a directory is the reference, the view must leave the database equal to it, the entry under its new key only.
