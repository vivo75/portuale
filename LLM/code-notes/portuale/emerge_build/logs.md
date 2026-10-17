# crates/portuale/src/emerge_build/logs.rs
Build logs: per-package `build.log` placement, `PORTAGE_LOGDIR` symlinks, and failure tails.

## fn build_log_path
- `pub(crate)` for the binary-merge digest-failure path (`emerge_getbinpkg`): Portage logs `BinpkgVerifier`'s digest block to this same `PORTAGE_LOG_FILE`.
- `PORTAGE_LOGDIR`/`PORTAGE_LOG_FILE_SEP`/`FEATURES=split-log` use the same "env var, not full config resolution" shortcut the rest of the CLI boundary uses (`DISTDIR`/`FEATURES` tokens, ...).
- The `.gz` suffix (Portage's `compress_log_ext`, `prepare_build_dirs.py:397-399`) applies to both the `${T}` path and the `PORTAGE_LOG_FILE`, so the symlink and its target agree.

## fn ensure_portage_logdir_symlink
- Ports Portage's `prepare_build_dirs()` `PORTAGE_LOGDIR`/`FEATURES=split-log` handling (`prepare_build_dirs.py:368-468`). When `logdir` is given, the build log lives there permanently: `<logdir>/<CATEGORY><sep><PF><sep><logid_time>.log`, or under `split_log`, `<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log`. With `compress` (`FEATURES=compress-build-logs`, `prepare_build_dirs.py:397-399`'s own `compress_log_ext = ".gz"`), both names gain the `.gz` suffix and phase output is gzip-encoded into the file (`ebuild_phases::open_log_file`'s pump thread; Portage's `EbuildPhase._open_log` `gzip.GzipFile(mode="ab")`).
- Everything downstream just opens the path `build_log_path` returned; symlinks are transparently followed by `std::fs`, so this is the only place that needs to know about `PORTAGE_LOGDIR`. No-op when `logdir` is `None` (`PORTAGE_LOGDIR` unset or empty; Portage's own `if mysettings.get("PORTAGE_LOGDIR", "") == "": del it`) or when it cannot be created (Portage's own "Permission issues... Disabling logging").
- `logid_time` is Portage's `os.stat(logid_path).st_mtime`: every phase of the same build (each its own fresh shell, `ebuild_phases::run_one_phase`'s own doc comment) and a resumed one share one timestamp.
- The re-symlink check is Portage's idempotent `make_new_symlink`: skip if it already points at the right place.

## fn tail_of
- Ports Portage's failure display (`Scheduler.py`) and QA scan (`doebuild.py`), which both wrap a `.gz`-suffixed log in `gzip.GzipFile(mode="rb")` before reading.

## fn foreground_log_path (S6, 351-MARK/DIE)
- Portage's `PORTAGE_LOG_FILE` for a foreground build (`prepare_build_dirs.py:398-468`): `${T}/build.log` (`.gz` under `compress-build-logs`), or the real `PORT_LOGDIR` path when usable, with `${T}/build.log` left as a symlink. Returns the real path (never the symlink): `die` prints it as `The complete build log is located at '…'` (`bin/isolated-functions.sh`, `die`).
- `PORT_LOGDIR` (not just `PORTAGE_LOGDIR`) is honored as the deprecated alias, like Portage's settings lookup (`config.py`, `_deprecated_keys`: direct key first, then the alias); the S0 `m07-port-logdir` cell exports that spelling. A stale symlink with no logdir configured is removed rather than followed.
- Callers resolve `logdir`/`sep` through `foreground_log_env` (process env, same as `build_log_path`); `make.conf`-only `PORT_LOGDIR` is equally unread on both paths (pre-existing, out of S6 scope).
