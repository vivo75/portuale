# crates/portuale/src/ebuild_phases/stamp_skip.rs
Portage's short-circuit lines around the phase chain (S5, #351 351-STAMP).

## module stamp_skip
- Stamp rule: `doebuild.py` `spawnebuild` checks
  `os.path.join(mysettings["PORTAGE_BUILDDIR"], f".{mydo.rstrip('e')}ed")`
  and skips unless `mydo == "install"` with `noauto` in features. The two
  lines go through `writemsg_stdout` (stdout). The check sits inside
  `spawnebuild`, which recurses through `actionmap[...]["dep"]`, so one
  pair prints per chain phase in chain order — the Rust check lives in the
  per-command phase loop in `commands.rs`, not around it.
- `install` is the only noauto-exempt phase; every other stamped phase
  still skips under `noauto` (S0 cell `s06`).
- The vendored bash's own skip texts (`bin/phase-functions.sh`
  `__dyn_pretend`/`__dyn_setup`/…: `>>> It appears that '${PF}' is already
  pretended; skipping.`) are unreachable once the Rust side skips first;
  they stay as dead fallback under `PORTAGE_QUIET=1`. The S5-before-S6
  ordering (plan risk 6) is what keeps them silent after un-quieting.
- WORKDIR block (`doebuild.py`, around the `>>> Not marked as unpacked`
  / `>>> WORKDIR is up-to-date` lines): gated on `tree == "porttree"`
  and `"unpack" in phases_to_run` (the full dep chain, so `compile`
  prints it too — S0 `s03`). Checked before `prepare_build_dirs`
  creates anything, which is why a fresh build is silent (S0 `s01`)
  while `setup unpack` is not (setup's `prepare_build_dirs` creates
  `WORKDIR`; S0 `s07a`). `create_directories` therefore also creates
  `WORKDIR` (mirrors `_prepare_workdir` in `prepare_build_dirs.py`).
- `alist` for the mtime loop is config `A` (USE-filtered); here
  `package_a(&env, &use_flags)`, the same list the fake distdir uses. A
  deleted distfile (`x_st is None`) does not recreate; only a strictly
  newer one does. No S0 cell covers the `Checking`/`Timestamp` lines
  (the S fixture has no `SRC_URI`); they are ported from source and
  pinned by a direct unit test of `check_workdir`.
- Stale-env notice (`bin/ebuild:363-389` `stale_env_warning`): once per
  invocation before the first non-`digest`/`manifest` arg, when `clean`
  is absent, `noauto` is off, a `build_dir_phases` member
  (`bin/ebuild:254-265`: setup/unpack/prepare/configure/compile/test/
  install/package/rpm/merge/qmerge — notably no `pretend`) is requested,
  and `${T}/environment` exists. `portage.writemsg` defaults to stderr
  (`util/__init__.py:88-99`) — hence stderr, unlike the stamp/WORKDIR
  lines — and the `${T}` stays literal. The `.ebuild_changed` touch in
  the same function is not ported (no valid-cache tracking on the
  direct path).
- `emit_phase_line` (`commands.rs`): stdout on the direct path, appended
  to `log_file` when the merge Scheduler owns the run (Portage's child
  stdout is the build log there); the `.gz` member case reuses
  `open_log_file`.

## residue: the Manifest/digest check (not in S5)
- portuale verifies no Manifest on the direct path, so there is no honest
  `* checking ebuild checksums ;-) ... [ ok ]` to print and no
  `* Manifest not found for '...'` refusal (S0 `s08`/`s09`) — filed for
  Z, not faked.
- No such gap for the notice's trigger: `${T}/environment` is saved by
  every phase run through the vendored `bin/save-ebuild-env.sh`
  (`phase-functions.sh:298`), exactly as in Portage, so the second run
  of any builddir sees the file and warns (live-verified 2026-10-17).
