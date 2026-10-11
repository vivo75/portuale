# crates/portuale/src/env_update.rs
Portage's `env_update()`: regenerate shell/systemd/ld.so state from `/etc/env.d` after a merge.

## module
- Call-site grounding: Portage's `dblink.merge()` (`lib/portage/dbapi/vartree.py:5198-5209`) runs `env_update()` unconditionally after a successful merge that installed something, even when `pkg_postinst` itself failed ("It's stupid to bail out here, so keep going regardless of phase return code"), gated only on the merge's own `CONTENTS` being non-empty.
- Scope cuts were confirmed with the user before implementing (v1 scope, portuale's own "narrow, documented" pattern).
- No cross-process mtime cache: Portage's `portage.mtimedb["ldpath"]` persists across a long-lived Portage session, but portuale's CLI is a fresh process per command, so every invocation is treated as a first run instead. Any candidate lib dir (`LDPATH` entries from env.d, an existing `usr/lib*`/`lib*` directory) found on disk post-merge counts as "changed" and triggers `ldconfig`. This exactly matches Portage's own genuine first-run behaviour (empty `prev_mtimes`); the only divergence is a repeat merge into the same `ROOT` that touched no lib dir, where Portage would skip `ldconfig` and portuale re-runs it anyway -- never wrong, just occasionally extra (cheap, idempotent) invocations.
- `getlibpaths()`'s `/etc/ld.so.conf.d/*.conf` (`include` directive) parsing is not reproduced -- a rare, admin-configured mechanism, not populated by anything portuale's fixtures do.
- No `CHOST`/`CBUILD` cross-compile handling at all (portuale has no cross-compile concept anywhere else either) -- always takes `env_update()`'s `else` branch, `<ROOT>/sbin/ldconfig`. Note this is the target `ROOT`'s own binary, not a host `PATH` lookup -- Portage's `_doebuild_spawn`-adjacent code relies on this being chroot-compatible with `-r target_root`.
- No `EPREFIX` support anywhere in portuale -- the bfd-linker alternate `/usr/etc/ld.so.conf` is never written.
- env.d files declaring their own extra `SPACE_SEPARATED`/`COLON_SEPARATED` keys are not honoured -- only the two hardcoded default sets (`CONFIG_PROTECT`/`CONFIG_PROTECT_MASK`; `PATH`/`LDPATH`/`MANPATH`/etc.) are treated as cumulative.
- `getconfig()`'s shlex-based parser is not reproduced -- env.d files use the same simple per-line `KEY="value"` shortcut `ebuild_merge::parse_slot` takes for `SLOT`.
- `/etc/ld.so.conf` is unconditionally rewritten every run (Portage's `env_update()` only rewrites it -- and only then considers `ldconfig` worth running for that specific reason -- when its content actually changed); moot here since portuale's `ldconfig`-triggering decision doesn't depend on that comparison at all, and `/etc/ld.so.conf` itself isn't vdb-tracked (no `CONFIG_PROTECT`/unmerge interaction to preserve).

## fn parse_env_d_line
- Same shortcut `ebuild_merge::parse_slot` takes for `SLOT`; Portage's `getconfig()` shlex parser (multi-line values, embedded escapes) is not reproduced.

## fn info_dir_values
- Collates the same way `run_env_update` collates every other `COLON_SEPARATED` key. Order is `_emerge/post_emerge.py::post_emerge`'s `infodirs = INFOPATH.split(":") + INFODIR.split(":")`, read from the settings reloaded after `env_update`. Empty entries are dropped (Portage's `chk_updated_info_files` skips `""` itself); duplicates are kept (Portage passes them through -- the dir-mtime memo makes a second visit a no-op). Shared by `info_dirs_inodes` (the unmerge-time inode set) and `info_files` (the post-merge regeneration list) so there is exactly one env.d parser for both.

## fn info_dirs_inodes
- Ports `dblink._unmerge_pkgfiles()`'s `infodirs_inodes` (`lib/portage/dbapi/vartree.py:2830-2846`). Uses `os.stat` semantics (follows symlinks, unlike the `lstat`-based inode keys portuale's own `dirs`/`protected_symlinks` otherwise use, since an `INFOPATH` entry naming a symlink should still match the directory it resolves to). Same tolerance `run_env_update` has for a missing `/etc/env.d`. Caller: `ebuild_unmerge::run_unmerge` threads the result into `cleanup_info_dir`'s `inode_key in infodirs_inodes` check.

## fn candidate_lib_dirs
- Ports the `potential_lib_dirs`/`getlibpaths`-adjacent candidate set. The `libexec` exclusion matches Portage's `os.path.basename(y) != "libexec"`.

## fn run_ldconfig
- Portage `os.path.join(eroot, "sbin", "ldconfig")` invoked as `[ldconfig, "-X", "-r", target_root]` with `cwd="/"`: a genuine unmodified subprocess (portuale's own "real subprocess, accepted dependency" pattern already established for `wget` in the fetch slice). The missing/non-executable tolerance is Portage's `env_update()`'s own -- most `ROOT`s in portuale's synthetic fixtures never install an `ldconfig` at all. Portage's `env_update()` only ever warns on a nonzero exit, never aborts the merge over it -- mirrored by ignoring the exit status once spawned.

## tests: info_dir_values_lists_infopath_before_infodir
- Expected value order from Portage's `post_emerge.py`.
