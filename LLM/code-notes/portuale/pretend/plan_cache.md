# crates/portuale/src/pretend/plan_cache.rs
Conservative on-disk plan cache for `emerge --pretend` (backlog #441 S3).

## module
- Scope: pretend-only runs with no side effects (see `ineligible_reason`).
  `--ask` is refused on the pre-`--pretend`-drop request (Portage drops it
  under `--pretend` in `_emerge/actions.py`, but a run that asked is out of
  scope); `--resume`/`--skipfirst` (saved state), `--fetchonly`/`-F`
  (downloads), `--buildpkgonly`/`--buildpkg`, removal actions, and
  `@preserved-rebuild` (preserved-libs registry + `NEEDED.ELF.2` + ELF
  content are not key inputs) never cache. Standalone actions
  (`--search`, `--info`, `--sync`, `--regen`, removals, ...) return before
  the cache point anyway. Anything unsure means no caching.
- What is cached: the stdout/stderr bytes and exit code of the
  resolve+display step only. The `--getbinpkg` refresh still runs first
  (its notes print live; the refreshed `Packages` content feeds the key),
  and the GLEP 42 news notice prints live after every replay. Strong review
  `LLM/2026-10-20-world-plan-preelaboration.strong-review.md` §2.1 is why:
  L3a replay is unsafe for anything but the plan section.
- Key (SHA-256, `sha2` is already a dependency; never `DefaultHasher`,
  per the strong review §1): full argv; the whole environment minus
  `_`, `OLDPWD`, `SHLVL` and the cache controls (a control var in the key
  would make `VERIFY=1` runs miss); tty facts (`isatty` stdout/stderr,
  hostname for the title escapes; `TERM`/colour vars ride the env);
  cwd (the `./path` atom check); the executable content hash (covers the
  embedded `make.globals`, which the host file never overrides --
  see `scalar_layers.rs`); schema version; `/etc/portage` recursively,
  `<eroot>/etc/profile.env`, the running root's tree when it differs;
  every repo's `timestamp.chk` (or git `HEAD`+ref), `layout.conf`,
  `profiles/` tree and md5-cache category mtime+count; VDB generation +
  entry count + world/world_sets atoms; local `Packages` + pool listing +
  every binhost's refreshed `Packages`; DISTDIR listing; root paths.
- Additions beyond the coordinator's list (all conservative -- only ever
  cause misses): DISTDIR listing (`f`/`F` bracket and download totals read
  it live; see `fetch_restrict.rs`), hostname/cwd, `profile.env`,
  running-root config, and the post-hoc Manifest/ebuild fingerprints
  below. Residuals: in-place content edits preserving size+mtime (same
  class as the brief's accepted md5-cache hole); eclass-only fallback
  reads for non-plan candidates; VDB in-place edits (Portage's own
  `_bump_mtime` staleness model); the entry-count and explicit root-path
  lines are redundant sentinels (generation mixes the count in;
  `ROOT`/`PORTAGE_CONFIGROOT` ride the env) kept for defence in depth.
- Post-hoc fingerprints: the pre-resolve key cannot enumerate the
  `Manifest` (download sizes) or the resolved `.ebuild` (metadata
  fallback), so `collect_posthoc` records `(size, mtime)` -- or `Absent`,
  which is a verified state too -- for every merging ebuild entry, and
  `load_and_verify` re-stats them before any replay. Any stat failure
  refuses the store; a mismatch is a miss. `Manifest` presence changes the
  totals (see `fetch_bytes_to_download`), so absence must be recorded.
- Storage: `$PORTUALE_CACHE_DIR` (created when missing) else
  `/var/cache/portuale` only when it exists and is writable; one file per
  key, temp+rename, `0600`, versioned header with exact-length validation.
  Reads are lock-free; corrupt/short/wrong-version is a silent miss.
  Poisoning: refuse files not owned by the euid or root, or group/world-
  writable; never read through a stale path (fstat, not stat). GC keeps
  the newest 32 by mtime; entries over 4 MiB are emitted but never stored.
- Controls: `--no-cache` (portuale-only, like `--solver=`; deliberately
  absent from `--help`, whose text is corpus-pinned),
  `PORTUALE_NO_CACHE=1`, `PORTUALE_CACHE_DEBUG=1` (one hit/miss line per
  eligible run, stderr only), `PORTUALE_CACHE_VERIFY=1` (a hit also
  recomputes fresh and aborts loudly with exit 99 on any divergence).
  `PORTUALE_PROFILE=1` disables the cache (profile spans would pollute
  the captured stderr). Default: on only when the dir is configured and
  writable, so contract-suite behaviour does not change.
- Capture: fd-level (`libc::dup2` over 1/2 into `0600` temp files; stdout
  is flushed before the swap, both streams after the body). Safe because
  tests spawn the binary as a subprocess (fd tricks never run in the
  `cargo test` process) and the resolve is single-threaded; a body panic
  restores the fds via `Drop`. `PLAN_CACHE_CAPTURING` suppresses
  `pretend_end_news_notice` inside the capture (it also fires on the
  gate-early-exit paths); the caller prints the notice live afterwards.
- `ExitCode` has no accessor, so the stored code rides the `Debug`
  trailing digits (`ExitCode(unix_exit_status(N))`), pinned by
  `exit_code_to_u8_matches_from_u8`. A parse failure falls back to 1.
- Tests (`pretend/tests/plan_cache.rs`): replay==fresh on five argv; one
  sensitivity test per key component (miss + byte-identical to the
  cache-off baseline); `--ask`/real-merge exclusion; news liveness (hit
  + only the count changes); verify accept/reject (exit 99); format
  round-trip and every truncation/corruption/permission/schema fallback;
  GC keeps 32. The mutation protocol (each test shown to fail with its
  key line removed; `count` and the explicit root-path lines documented
  redundant above) was run 2026-10-22, all as expected.

## fn ineligible_reason
- The `--ask` input is the pre-drop request: `validate_and_early_actions`
  clears `opts.ask` under `--pretend` (Portage semantics,
  `_emerge/actions.py`), so the cache reads it before the drop.

## fn build_key
- `eroot` is the target root here (`resolve_config_for_root` passes
  `eroot: root`), so `<eroot>/etc/profile.env` is `<ROOT>/etc/profile.env`.
  An unreadable mid-walk file refuses the key (`Err`); a missing top dir
  hashes as absent. Absolute `source` lines in make.conf resolve under
  the config root (`make_conf.rs`), so they stay inside the hashed tree.

## fn collect_posthoc
- Only merging outcomes (`New`/`Reinstall`/`Upgrade`/`Downgrade`) with a
  resolved repo: installed/binary entries never read `Manifest`/ebuild,
  and the version formats the `{package}-{version}.ebuild` path exactly
  like `md5_cache_entry_is_valid` does. A resolve error carries no
  entries, so nothing is ever stored for it.

## fn load
- The ownership/permission gate is the poisoning defence (backlog #441
  S3 §4): dir writability gates the cache on, per-file ownership gates
  every replay.

## fn store
- The `stored` debug line doubles as the cold-miss line (a cold run
  prints exactly one line); lookup misses on absent files stay silent.
