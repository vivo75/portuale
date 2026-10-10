# crates/portuale/src/emerge_build/tests/logs.rs
Build-log path, `PORTAGE_LOGDIR` symlink, and capture tests.

## fn ensure_portage_logdir_symlink_gains_the_gz_suffix_when_compressed
- Ports Portage's `prepare_build_dirs.py` `compress_log_ext`: with `FEATURES=compress-build-logs` the log file is `<...>.log.gz` in both layouts.

## fn tail_of_gunzips_a_compressed_build_log
- Ports Portage's `Scheduler.py` / `doebuild.py`, which wrap a `.gz`-suffixed log in `gzip.GzipFile(mode="rb")` before reading.

## fn capture_log_also_captures_pkg_preinst_and_pkg_postinst_output
- Under `capture_log` (always true for `--jobs` > 1, opt-in via `--quiet-build`/`-q` otherwise; Portage's `Scheduler._background_mode`), the `install` phase output already went to its own `build.log`, but `pkg_preinst`/`pkg_postinst` (run separately, not part of `install`'s own `actionmap_deps` chain; see `merge_after_install`) previously had no log file threaded to them at all.
- The `${T}/build.log` the test reads is removed by the post-merge clean; kept via `FEATURES=noclean` (the clean itself is pinned separately, backlog #42).
