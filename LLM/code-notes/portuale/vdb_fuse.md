# crates/portuale/src/vdb_fuse.rs
The FUSE adapter of `portuale vdb mount`: `fuser` callbacks to the pure views.

## module
- Origin: feat#157 / backlog #305, plan S7.2. The pure `vdb_view::View` holds every decision (tree, inodes, attributes, generation pinning, EROFS, the generated `metadata` file) and is unit tested without a mount. With `--rw` (#317) the callbacks go to `vdb_rw::RwView` instead, the read-write layer over the same view.
- Exercised by the S7.4 commands on a FUSE-capable host, not by `cargo test` (needs `/dev/fuse` and `fusermount3`).

## impl Filesystem for Fs: init
- Pass O_TRUNC to `open` instead of a separate setattr(size=0) first: Portage's in-place `> environment.bz2` (pkg_postinst) is then one live rewrite, stored at `close`.

## fn copy_file_range
- Portage's `shutil.copyfile` tries copy_file_range first (S0) and falls back to plain writes on EOPNOTSUPP, so answering EOPNOTSUPP is correct.

## fn serve (the `--rw` store import)
- Portage's emerge wrote world, the counter, and the other stores on disk under ROOT while the entries went to the database; taking them in at unmount is #317 S4.

## fn serve_background
- `--rw`: the daemon's last words (the store import at unmount) go to a file next to the scratch directory, which the next mount of the same database clears but this file survives (#320).

## tests: a_detached_daemon_writes_its_stderr_to_the_log_file
- #320: a detached daemon's stderr goes to the log file, so the line printed after `session.run()` returns is not lost.
