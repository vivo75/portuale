# crates/portuale/src/helpers/chmod_lite.rs
Native `chmod-lite` helper: recursive permission normalization ported from Portage.

## module
- #326 S2.4, D2/D3: ports Portage's `bin/chmod-lite.py`'s `main()` over `apply_recursive_permissions(filename, filemode=0o644, filemask=0o022, dirmode=0o755, dirmask=0o022)` (`3rdparty/portage/lib/portage/util/__init__.py:1251`), which wraps `apply_secpass_permissions` (`:1323`) over `apply_permissions` (`:1145`, the mode/mask arithmetic below).
- Portage's `bin/ebuild-pyhelper` records the caller's cwd in `__PORTAGE_HELPER_CWD` and `chmod-lite.py` chdirs back to it after the imports; the `bin/chmod-lite` shim keeps the caller's cwd, so this helper takes its arguments relative to the current directory, as raw bytes throughout (argv paths may not be UTF-8).
- Fidelity notes (all from the Portage sources above): minimal chmod calls (a path is chmod'ed only when the arithmetic says a bit must change); symlinks never followed and never chmod'ed (`follow_links=False`, and `apply_permissions` clears `new_mode` for symlinks); a directory is chmod'ed before it is walked (bug 554084); the top-level `FileNotFound` is swallowed for backward compatibility (a missing top returns True), while one deeper in the walk is ignored as an `InvalidLocation`; `EPERM` prints `Operation Not Permitted: ...` and `ENOENT` prints `File Not Found: '...'` (the default `onerror`); anything else Portage would let propagate (a traceback, exit 1); `main()` returns `os.EX_OK` unconditionally (exit 0 even when some permissions were left unapplied).

## fn run
- D2: `PORTUALE_PYTHON_HELPERS=real` brings back today's behaviour: exec the checkout's upstream script with the same argv.

## fn exec_real_chmod_lite
- `portage_checkout()` reads only env and the build path, never `bin_dir()`, so this is safe from the helper entry (D7).

## fn apply_recursive
- Port of `apply_recursive_permissions(top, uid=-1, gid=-1, ...)`: with no owner/group request the secpass/chown half is a no-op and only the mode arithmetic remains. Returns true when an unexpected error occurred (Portage: an uncaught exception). Backward compatibility: a missing top is silently OK.

## fn walk
- Depth-first walk of `dir` (already chmod'ed by the caller, bug 554084): files get `filemode`/`filemask`, subdirectories `dirmode`/`dirmask` before recursing. A directory that cannot be listed is silently skipped (Portage `os.walk` ignores its errors), and entries that disappear mid-walk are ignored (`InvalidLocation`). The stat errno mapping: EPERM prints, anything else (EACCES, EROFS, ...) propagates as a traceback.

## fn apply_one
- Port of `apply_permissions`' mode half (uid/gid always -1 here): with `mask >= 0`, adds the `mode` bits and clears the `mask` bits, but only chmods when something would actually change. `st_mode` carries the full `0o7777` bits so owner-exec and the special bits survive (the mode only adds, the mask only clears).

## fn report_stat_error
- Port of `_do_stat`'s error mapping for the top-level stat: EPERM prints through `onerror`, anything else Portage would let propagate.

## mod fixture_tests
- #326 S2: every `fixtures/helpers/chmod-lite/<case>` oracle -- rebuild the tree from `in.manifest`, run the native helper as a child process (cwd = tree root, the same top-level-argv shape as Portage's `find ... -exec`), and compare the resulting lstat manifest with `out.manifest` (or `out.root.manifest` as root), plus rc and stderr. Includes the non-UTF-8 case (byte paths throughout).
- HOST QUIRK (fixtures/helpers/README.md): zfs rejects non-UTF-8 names with EILSEQ; stage under temp_dir (tmpfs here) and skip with a printed reason when even that fails.
- Portage's call shape (`phase-helpers.sh:511`): top-level entries, symlinks excluded (`! -type l`).

## fn chmod_lite_normalises_a_0600_file_to_0644
- The isolation proof's counterpart (see the S2 report): with the native helper this case normalises; the proof breaks the helper and watches this assertion fail. Kept as its own test so the failure names the behaviour, not the fixture loop.

## test helpers (decode, parse, build_tree, result_manifest)
- Manifest `%XX` escapes decode to raw bytes (paths are `/`-separated; every byte outside `[A-Za-z0-9._/+-]` is escaped). Modes are set deepest-first so `0000` dirs can still be populated. The `result_manifest` lstat walk has the same shape as the generator's.
