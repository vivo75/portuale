# 02.346 execution slices (branch `backlog/346-351-config-output`)

## S1 (346-UP: the user profile is Portage's last profile node)

Status: in progress (executor: unattended batch worker, 2026-10-17).
Oracle: Portage 3.0.82.2 captures in `LLM/evidence/2026-10-15-346-s0/`
(`u/` family + `matrix.md`); every expected value below comes from those
Portage captures, never from portuale output. Plan defaults taken: Q1 yes
(a real node), Q2 stated rule (ignored without `make.profile`).

### What changed (files)
- `crates/portage-profile/src/atom_lines.rs`: new `USER_PROFILE_FORMATS`,
  `UserProfileNode`, `resolve_user_profile_node` (gate: non-empty chain +
  `etc/portage/profile` exists; EAPI from the node's `eapi` file, default
  `None`, never an abort), `UserNodeFile`, `user_node_rules` (per-file
  wildcard/build-id table, `allow_repo` from the user formats).
- `crates/portage-profile/src/resolve/scalar_layers.rs`: new
  `fold_profile_level_use` (shared chain/user level mechanism);
  `LoadScalarLayersOut` gains `user_node`; the user `make.defaults` +
  `package.use` form the last `profile_use_layers` level.
- `crates/portage-profile/src/resolve/package_use.rs`: the user-profile
  `package.use` moves out of the flat fallback into its level.
- `crates/portage-profile/src/resolve/use_levels.rs`,
  `use_force_mask.rs`, `use_masks.rs`, `package_masks.rs`,
  `package_sets.rs`: every fold takes the node and stacks the user
  files last with node rules; `package.accept_keywords` and `@profile`
  gain their missing user-node reads; flat `use.stable.*` gain the user
  level; all user reads gated on the node (u15).
- `crates/portage-profile/src/lib.rs`: scope-cut comments updated.
- `crates/portuale/src/pretend/info.rs`: `resolved_global_use` folds the
  `features` tier (`test` unless a stronger tier revokes it), so
  `emerge --info` USE matches Portage (u17).
- Tests: 7 Rust unit tests (`portage-profile` resolve) + 1
  (`portage-repo` effective-USE user level last); 20 pmtest pins
  (`test_emerge_pretend_contract.py`, S0 `u01`–`u19`).
- Docs: `LLM/code-notes/portage-profile/atom_lines.md` (residues now
  shipped, new symbols), `resolve/{scalar_layers,mod}.md`; paragraph
  appended to `LLM/what-this-proves.md`.

### Commands run and their result
- Baseline (pre-edit tree, `cargo build --release` then `timeout 2400
  cargo test --release -- --test-threads=2`, bg + poll):
  `/var/tmp/s1-baseline-test.log` — 31 suites ok, 2747 passed total,
  0 failed.
- `cargo fmt` / `cargo fmt --check`: clean (two self-made long lines
  folded). `cargo clippy --release --all-targets`: 0 warnings (two new
  `too_many_arguments` sites bundled into args structs first).
- Post-change `cargo test --release` (`/var/tmp/s1-verify-test.log`):
  31 suites ok, 2755 passed (2747 + 8 new: 7 `portage-profile`
  `user_profile_*`, 1 `portage-repo` user-level-last), 0 failed.
- pmtest: 20 new S1 pins + neighbours
  (`-k "s6_user_config or test_s1_"`): 21 passed. One moved pin:
  `test_s6_user_config_validates_each_file_like_portage` stdout is now
  `USE="(x)"` (was `"x"`): the kept user-profile `package.use.force`
  entry now forces through the last level. Re-grounded in Portage's
  own C-userconfig capture (`LLM/evidence/2026-10-11-341-s0/portage/
  C-userconfig.stdout`, `USE="(x)"`), docstring corrected.
- Full pmtest suite
  (`python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/s1-pt-full -p no:cacheprovider`,
  `/var/tmp/s1-pmtest.log`): 3974 passed, 37 skipped, 2 xfailed,
  0 failed, 0 corpus-drift mentions.
- `grep -rn "split_whitespace\|split(' ')" crates | grep -i contents`:
  empty (rule f).
- G6 regression guard: `podman run --rm --entrypoint /bin/bash
  localhost/test-portuale:latest -c 'ls -la /etc/portage/profile'`
  → the dir exists, holding only an empty `package.use.force/`
  directory (no files). Both managers append an empty node there.
- L0 (`differential-test-bed/run/l0-resolver.sh`,
  `/var/tmp/s1-l0.log`, `logs/l0-20261017T084509Z`): 120 probes, 101
  clean, 8 explained (standing allowlist), 31 UNEXPLAINED (advice 9,
  error 6, order 14), 0 portuale-only findings, 0 invariant
  violations. All 31 are merge-order or autounmask-`[use]` disclosure
  cells (e.g. `>=x11-base/xwayland-23.1.0[libei]`,
  `>=app-crypt/gcr-3.27.90:0=[gtk]`); inspected the plasma-meta cell:
  portuale prints no masked block at all there, so S1's chain-line
  addition is invisible in it. With the container's user profile empty,
  profile resolution is a proven no-op on both sides (unit-pinned);
  the only output delta in S1 moves toward Portage. Nothing
  allowlisted (forbidden alone); the 31 belong to merge-order /
  #398 autounmask-USE scope.
- L0 fixture oracle (`l0-fixture-oracle-all.sh`, `/var/tmp/s1-l0fx.log`):
  13/13 lists green, rc 0.

### Anything surprising
- S1 grew one product fix beyond the plan's file list: the u18
  `@profile` cell needs Portage's
  `(dependency required by "@profile" [argument])` line after the
  masked rows. `render_all_masked_block` takes the set label now;
  the top-level masked path passes
  `ctx.top_level.sets.get(current_atom)` (direct atoms: `None`, no
  line — S0 u14b proves Portage prints none there). Exact blank-line
  layout verified against `u18-profile-starred-portage.stderr`.
- The `test_s6…` pin move above (toward Portage, not churn).
- `emerge --info` USE needed the `features` tier (`test`) folded into
  `resolved_global_use` (Portage USE_ORDER); `--info`-only function,
  no other caller.
- S0 u10's CONTRA confirmed: the `eapi=5` wildcard warning prints once
  (deduped), and portuale now prints it.

### Anything left undone and why
- Nothing open for S1. The 31 L0 UNEXPLAINED findings are standing
  merge-order / autounmask-USE divergences (other slices' scope),
  reported, not allowlisted.
- No commit/push/checkout/stash (coordinator's grant).

## S4 (#349, 351-BIN: `ebuild` reads its configuration)

Status: done (executor: unattended batch worker, 2026-10-17).
Oracle: Portage 3.0.82.2 captures in `LLM/evidence/2026-10-15-346-s0/`
(`e/` family + `matrix.md`: every E-cell expected value below comes
from those Portage captures), plus two fresh host-`ebuild` captures of
the exact `make.globals`-copy setups in `s4-oracle/` (same Portage
3.0.82.2; the S0 evidence has no `make.globals`-copy `setup` cell, and
the task allows re-capture where evidence is lacking). Plan defaults
taken: Q4 yes (resolved FEATURES drive the direct-path gates, D3 site
list mirrored without touching shared `from_env`), Q5 yes (direct-path
`binpkg-multi-instance` follows resolved FEATURES). 02.355 S4 (resolved
`unmerge-orphans` + synthetic `make.globals` base layer) reused as
committed, not redone.

### What changed (files)
- `crates/portage-repo/src/env_roots.rs`: new `DISTDIR_DEFAULT`
  (`make.globals:30`), `distdir_from_config`,
  `distdir_from_env_or_config`, `portage_tmpdir_from_env_or_config`
  (calling env highest, then config, then default); 2 unit tests.
- `crates/portage-profile/src/phase_environ.rs`: new
  `filter_unprefixed_use_expand_value` +
  `apply_unprefixed_use_expand_filter` (Portage `config.py` `setcpv`
  `USE_EXPAND_UNPREFIXED` loop; domain is the narrowed `PORTAGE_USE`);
  wired into `phase_environ` step 5 (combined map -- the pkg helper's
  fresh map never carries the scalar, so filtering there is a no-op)
  and exported for the merge tail; 2 unit tests.
- `crates/portuale/src/ebuild.rs` (`run_real_commands`,
  `unmerge_and_package_options`): one resolved `Config` per invocation
  (cached in the caller, `None` outside a repo keeps env-only
  fallbacks); tmpdir/distdir from
  `portage_tmpdir_from_env_or_config`/`distdir_from_env_or_config`
  (package.env `PORTAGE_TMPDIR` still layers on top); merge gates mirror
  `merge_setup.rs:63-78` (`resolved_config`, `config_install_mask`,
  `apply_uninstall_ignore`, `set_resolved_features`,
  `apply_config_protect`, `package_env_vars`, resolved `distdir`) then
  the per-package `FEATURES` fold, then `bin/ebuild`'s scheduler-only
  discards; the final list rides phases via `direct_build_env`.
  `PackageOptions` `buildpkg_live`/`binpkg_multi_instance`/
  `binpkg_signing` follow the resolved FEATURES when a config resolves
  (Q5: the "deliberate conservatism" is gone). 6-arg fn bundled into
  `UnmergeAndPackageOptionsArgs` (clippy `too_many_arguments`).
- `crates/portuale/src/ebuild_phases/standalone_config.rs`: new
  `discard_direct_ebuild_features` (`fail-clean`, `merge-wait`;
  `bin/ebuild:305-309`); standalone `USE` is the full `PORTAGE_USE`
  (was IUSE display only); unprefixed filter over the base flags.
- `crates/portuale/src/ebuild_phases/phase_env.rs`: `FEATURES`
  fallback is the run's resolved list (`phase_features_value` removed);
  `PORTAGE_CONFIGROOT` pushed with the trailing slash
  (`LocationsManager.py`); `PORTAGE_REPO_NAME` pushed from
  `repository_name_for` (fixes direct-path `die` `::repo`).
- `crates/portuale/src/ebuild_phases/sandbox.rs`:
  `phase_features_value` replaced by `config_root_with_slash`.
- `crates/portuale/src/emerge_build/entry_env.rs`: merge-tail
  unprefixed filter over the combined per-entry map.
- `crates/portuale/src/pretend/env_config.rs`
  (`package_options_from_env`) and `resolve_plan.rs` (`plan_resolve`):
  `DISTDIR` follows the resolved config (env highest).
- `crates/portuale/src/pretend/prompt.rs` (+ 2 call sites):
  `clean_delay_countdown` reads `CLEAN_DELAY` from env, else resolved
  config, else 5 (`actions.py:2748-2758`; S0 `l04`-adjacent).
- `crates/portuale/src/ebuild_package.rs`: `binpkg_multi_instance`
  doc updated (resolved FEATURES decide).
- Tests: 9 new Rust unit tests; 7 pmtest pins in
  `test_portuale.py` (`test_s4_*`: tmpdir/distdir/USE/ARCH/slash/repo,
  resolved FEATURES list, env-wins, multi-instance layout, config-DISTDIR
  fetch, die `::t348`, config CLEAN_DELAY).
- pmtest bed prerequisite (new, NOT run):
  `differential-test-bed/run/l1-ebuild-direct.sh` -- stock-make.globals
  `portuale ebuild <gentoo bash>.ebuild merge` vs Portage `ebuild ...
  merge` in fresh containers, rc + installed version recorded. The
  coordinator runs it with the L1/L3 gates.
- Docs: code notes under `LLM/code-notes/{portage-repo/env_roots,
  portage-profile/phase_environ,portuale/ebuild{,_phases/*,pretend/*}}`
  (`code_notes_index.py` clean); paragraph appended to
  `LLM/what-this-proves.md`; fresh oracle in `s4-oracle/`
  (`g/e-setup-portage.{argv where applicable,rc,stdout,stderr,stagelog}`,
  `pkg-portage.{rc,stdout,stderr,pkgfind}`).

### Commands run and their result
- Baseline (pre-edit tree, same HEAD): pmtest full suite
  `/var/tmp/s4-baseline-pmtest.log` -- 3974 passed, 37 skipped,
  2 xfailed, 0 failed. Cargo baseline reused from S1's post-verify on
  the identical tree (2755 passed, 0 failed; `git status` clean).
- `cargo build --release` before every test run (rule a).
- `cargo fmt` / `cargo fmt --check`: clean. `cargo clippy --release
  --all-targets`: 0 warnings.
- Post-change `cargo test --release -- --test-threads=2`
  (`/var/tmp/s4-test.log`, bg + poll, no #376 hang): 31 suites ok,
  2764 passed (2755 + 9 new), 0 failed. A later `cargo fmt` touched
  whitespace in one new test only; targeted re-run green
  (`discard_direct`, 1 passed; test-binary recompiled).
- New pins: `-k s4_` 7 passed.
- Full pmtest suite post-change, first run (`/var/tmp/s4-final-pmtest.log`):
  3979 passed, 37 skipped, 2 xfailed, 2 FAILED by test NAME vs the
  pre-edit baseline (0 failed):
  `test_ebuild_without_network_sandbox_shares_the_host_network_namespace`
  and `test_ebuild_without_sandbox_tolerates_the_same_stray_write`.
  Cause: both run under the host config with `FEATURES=""`, which never
  disabled host-`make.globals` sandbox tokens on either package manager
  (`FEATURES` folds incrementally, Portage `config.py`); S4's resolved
  gates now sandbox like Portage does there, so the phases ran isolated
  (different netns) / sandbox-confined. The pins encoded portuale's old
  env-only read, not Portage behavior -- re-grounded by spelling the
  negation (`-network-sandbox -sandbox -usersandbox -userpriv
  -ipc-sandbox -mount-sandbox -pid-sandbox`), which is what disables a
  config token on both sides; docstrings say so. Targeted re-run:
  5 passed (the 2 plus the 3 `sandbox_features_each` params).
- Full pmtest suite post-fix (`/var/tmp/s4-final2-pmtest.log`, bg +
  poll; compared by test NAME against the pre-edit baseline): 3980
  passed, 37 skipped, 2 xfailed, 1 FAILED --
  `test_ebuild_network_sandbox_gives_src_phases_a_fresh_network_namespace`,
  the same `FEATURES=""`-vs-host-config cause in its `plain_env` arm
  (both runs net-isolated, so the inequality failed). Fixed the same
  way (negations in `plain_env`); targeted re-run of the 3
  network/sandbox tests plus the 7 S4 pins: 10 passed.
- Full pmtest suite post-fix-2 (`/var/tmp/s4-final3-pmtest.log`, bg +
  poll): 3981 passed (3974 baseline + 7 new S4 pins), 37 skipped,
  2 xfailed, 0 failed -- green, no new failures by test NAME vs the
  pre-edit baseline. Corpus drift: none (no `corpus drift` mentions;
  nothing blessed).
- `grep -rn "split_whitespace\|split(' ')" crates | grep -i contents`:
  (rule f; result recorded here).
- Bed scripts (run by the coordinator after the worker, 2026-10-17):
  - Gate (fmt, clippy 0 warnings, workspace cargo test, pmtest suite):
    green, 3981 passed, 37 skipped, 2 xfailed.
  - glibc+bash merge gate, three runs (plain / reinstall / install
    mask): rc 0, hard 0, UNEXPLAINED 0 each. `l1-porttest`: 0/0.
  - New cell `l1-ebuild-direct.sh`: green with `app-text/dos2unix`
    (both PMs rc 0, same installed version). Honest history: the
    first picks (`bash-9999`, then `bash-5.4_alpha_pre...`, then
    `bash-5.3_p15`) failed with rc 1 on BOTH Portage and portuale
    identically (git-r3 fetch without git in the image; Portage's own
    `60noop-testsuites` install-QA hook dying "USE Flag 'test' not in
    IUSE" on a direct `ebuild` run of a package without `IUSE=test`),
    so they were environmental, not divergences; the cell now picks
    a package with `IUSE=test`.
  - L3 smoke (`l3-smoke.txt`, builds glibc + bash from source, runs
    every phase): run 1 at `-j12` gave rc 1 with ONE unexplained row,
    `MISSING /usr/lib/debug/.build-id/0c`. Cause: Portage's bash came
    out with build-id `0c...`, portuale's with `0b153159...`, which is
    the build-id both sides had on 2026-10-11, so portuale's output
    is unchanged and the Portage side varied. Run 2 (serial default
    jobs, `l3-20261017T132015Z`): rc 0, hard 0, UNEXPLAINED 0, payload
    diffs 26. The host was loaded by other work during both runs, so
    timings are not recorded.

### Anything surprising
- The first FEATURES pins failed on `merge-wait`: portuale exported it
  (host `make.globals` has it) but Portage's phase env does not.
  Root cause: `bin/ebuild:305-309` unconditionally discards
  `fail-clean` and `merge-wait` for the direct path (scheduler-only
  tokens); the `emerge` Scheduler gates on `merge-wait`, so it keeps
  them. Ported as `discard_direct_ebuild_features` (direct path only).
  `noauto` is deliberately NOT dropped (only for `merge`, with
  phase-loop semantics S5/S6 own) -- S5-adjacent residue.
- The `USE_EXPAND_UNPREFIXED` filter first landed inside
  `phase_environ_pkg` and silently did nothing (fresh map, no scalar);
  moved to the combined maps (`phase_environ` step 5, merge tail,
  standalone base). The domain is the narrowed `PORTAGE_USE`, not the
  full effective set (Portage filters after `difference_update`).
- A probe `repos.conf` without the overlay sections fails resolution
  (`overlay:crossrepo-parent` in the fixture profile's `parent`); S4
  test roots carry the same repo set as S0's `mkcfg`/S1's `_t341`
  helpers. Related: `resolve_standalone_config` (and hence every S4
  default) is `None` when the profile chain cannot resolve -- then all
  readers keep today's env-only behavior.
- `T348_LOGDIR` set in `make.conf` reaches the phase (config scalars
  are exported unless filtered), which is what makes the per-test
  env-dump pins hermetic; via the calling env it is dropped (not
  whitelisted), which is why S0-day dumps needed the shared fallback.
- `emerge -C` on the fixture vdb seeds (no `CONTENTS`) keeps the
  `not installed` error by design (#348 S6 M1); the CLEAN_DELAY pin
  therefore merges `t348-d-unmerge-1` for real first.

### Anything left undone and why
- Emerge-half fetch landing (`349-DIST`): `plan_resolve` and
  `package_options_from_env` now read config `DISTDIR`, covered by the
  `distdir_from_*` unit tests, but no end-to-end fetch pin exists:
  portuale has no `--fetchonly` (S0 `e07` NEW), and a real-merge fetch
  vehicle would need the distfiles proxy in the committed suite. The
  direct-path `PORTAGE_ACTUAL_DISTDIR`/seeded-fetch pins cover the
  mechanism. Vehicle decision for the coordinator: pin it when
  `--fetchonly` lands.
- `Packages.gz` beside `Packages` on `ebuild ... package` (seen in S0
  `e06`: Portage writes both, portuale only `Packages`). Out of S4's
  file list; not pinned, not fixed.
- `noauto` in direct-path `FEATURES` for `merge` (see above);
  S5-adjacent.
- No bed script run (per task); the `l1-ebuild-direct.sh` cell and the
  merge-path gate (glibc + bash) are the coordinator's.
- No commit/push/checkout/stash (coordinator's grant). Scratch under
  `/var/tmp/s4probe`, `/var/tmp/s4cap`, `/var/tmp/t348-b`,
  `/var/tmp/portage` probe remnants deleted at the end.

## S5 (351-STAMP: the stamp, environment, workdir and checksum lines)

Status: done (executor: unattended batch worker, 2026-10-17).
Oracle: Portage 3.0.82.2 captures in `LLM/evidence/2026-10-15-346-s0/`
(`s/` family + `matrix.md`); every expected value below comes from those
Portage captures, never from portuale output. No bed script run (per
task). Plan defaults: none open for S5 (unattended yes).

### What changed (files)
- NEW `crates/portuale/src/ebuild_phases/stamp_skip.rs`: `BUILD_DIR_PHASES`
  (`bin/ebuild:254-265`, notably without `pretend`), `stamp_file_name`
  (`.{phase.rstrip('e')}ed`), `is_stamped` (stamp exists, except
  `install` under `noauto`), `stamp_skip_lines` (the stdout pair),
  `check_workdir` (missing WORKDIR silent; no `.unpacked` recreates with
  `Not marked...`; else per-distfile `Checking...`/`Timestamp...`
  mtimes, else `WORKDIR is up-to-date`), `wrap_notice_70` +
  `stale_env_lines` (stderr notice, literal `${T}`); 8 unit tests.
- `crates/portuale/src/ebuild_phases/commands.rs`: the chain loop is now
  per command (every caller passes a single command — verified at
  `scheduler.rs:91`, `ebuild_merge/mod.rs:151`,
  `ebuild_package.rs:695`, `ebuild.rs:408` — so fetch/fake-distdir
  ordering is unchanged): WORKDIR check before `create_directories`,
  then fetch, then phases with the stamp skip (`continue`, never
  spawn). New `emit_phase_line` (stdout direct, appended to `log_file`
  when the merge Scheduler owns the run, `.gz`-safe via
  `open_log_file`).
- `crates/portuale/src/ebuild_phases/environment.rs`:
  `create_directories` also creates `WORKDIR` (mirrors
  `prepare_build_dirs._prepare_workdir`; without it the `setup unpack`
  shape could never print `Not marked...`).
- `crates/portuale/src/ebuild.rs` (`run_real_commands`): `bin/ebuild`'s
  stale-env notice once per invocation before the first command
  (`clean` absent, `noauto` off, build-dir command requested,
  `${T}/environment` present), on stderr.
- Tests: 8 Rust unit tests + 1 Rust e2e in
  `ebuild_phases/tests/commands.rs` (stamped `unpack` prints the pair
  through the real binary and the body does not re-run); 4 pmtest pins
  in `test_portuale.py` (`test_s5_*`: unpack-twice exact stdout,
  setup-unpack `Not marked...`, noauto install runs, stale-env notice
  on stderr).
- Docs: code note
  `LLM/code-notes/portuale/ebuild_phases/stamp_skip.md`
  (`code_notes_index.py` clean); paragraph appended to
  `LLM/what-this-proves.md`.

### Commands run and their result
- Baseline (pre-edit tree): cargo `31 suites ok, 2764 passed,
  0 failed` (`/var/tmp/s5-test-base.log`); pmtest `3981 passed,
  37 skipped, 2 xfailed, 0 failed` (`/var/tmp/s5-pmtest-base.log`).
- `cargo fmt` / `cargo fmt --check`: clean. `cargo clippy --release
  --all-targets`: 0 warnings.
- Focused: `cargo test --release stamp_skip` — 8 passed (one first-run
  failure in the mtime test: fs timestamp granularity; fixed by pinning
  the workdir mtime to 2020 via `libc::utimensat`).
- `cargo test --release -- --test-threads=2
  ebuild_phases::tests::commands` — 22 passed (first attempt hit the
  self_exe guard: the binary predated the `set_mtime` edit; rebuilt
  with `cargo build --release` first, then green — operating rule a).
- New pins `-k s5_`: 4 passed.
- Post-change full `cargo test --release`
  (`/var/tmp/s5-test-final.log`, bg + poll, no #376 hang): 31 suites
  ok, 2773 passed (2764 + 9 new: 8 unit + 1 e2e), 0 failed.
- Post-change full pmtest, first run
  (`/var/tmp/s5-pmtest-final.log`): 3984 passed, 37 skipped, 2 xfailed,
  1 FAILED by test NAME vs the pre-edit baseline (0 failed):
  `test_ebuild_debug_flag_enables_real_set_x_tracing` runs `pretend`
  twice in one builddir; the second leg no longer spawns (correctly
  stamp-skipped, as S0 `s02` shows for Portage), so no `+ ` trace
  appears. A host-Portage re-capture of that shape was attempted but
  Portage refuses the Manifest-less `mergepkg` fixture outright (rc 1,
  `* Manifest not found` — itself the live s09 behavior), so no
  Portage oracle exists for that ebuild; the fix keeps both legs as
  first runs (fresh `PORTAGE_TMPDIR` for the `--debug` leg, the
  established "fresh builddir per run" pattern), preserving the pin's
  intent (`--debug` enables `set -x`). Targeted re-run: 6 passed.
- Post-change full pmtest, second run
  (`/var/tmp/s5-pmtest-final2.log`, compared by test NAME against the
  pre-edit baseline): 3985 passed (3981 + 4 new S5 pins), 37 skipped,
  2 xfailed, 0 failed — green. Corpus drift: none (no `corpus drift`
  mentions; nothing blessed).
- `grep -rn "split_whitespace\|split(' ')" crates | grep -i contents`:
  empty (rule f).

### Anything surprising
- `${T}/environment` IS saved on the direct path (vendored
  `bin/save-ebuild-env.sh` via `phase-functions.sh:298`), so the
  stale-env notice fires naturally on the second run of any builddir
  (live-verified with a scratch ebuild under `/var/tmp/s5prove`);
  the pin runs `unpack` twice with no seeding, exactly like S0 `s02`.
- `writemsg` defaults to stderr (`util/__init__.py:88-99`): the
  environment notice is stderr, while every stamp/WORKDIR line is
  `writemsg_stdout` (stdout) — the S0 captures confirm the split, and
  the pins assert it per stream.
- The fixture has no `SRC_URI`, so no `Checking...` lines precede the
  verdict in any S cell; the mtime sub-block is ported from source
  (`doebuild.py`) and pinned only by a direct unit test of
  `check_workdir`.

### Anything left undone and why
- Checksum lines + `* Manifest not found` (S0 `s08`/`s09`): NOT
  implemented, per the plan's conditional ("if it does not, file it,
  do not add a fake line") — portuale verifies no Manifest on the
  direct path, so neither line could be printed honestly. Filed as
  **#400** (371–373 were exhausted; 400 checked free on every branch).
- No `ebuild manifest`/`digest` command either (pre-existing dry-run
  stubs, untouched).
- No commit/push/checkout/stash/`git add` (coordinator's grant).
  Scratch `/var/tmp/s5prove`, `/var/tmp/s5-draft` deleted at the end;
  `/var/tmp/s5dbg*` probe dirs likewise. No bed script run (per task);
  no `cargo build` raced any bed run.

### Bed results (coordinator, 2026-10-17; host loaded, no timings)
- Gate (`gate.sh s346c`): pmtest 3985 passed, 37 skipped, 2 xfailed.
- Merge gate (`mgate.sh s346c`, glibc + bash): plain, reinstall and mask
  runs each rc 0, hard 0, UNEXPLAINED 0.
- `l1-porttest` list: rc 0, hard 0, UNEXPLAINED 0.
- `l1-ebuild-direct.sh`: green, both package managers merged the same
  dos2unix (`l1-ebuild-direct-20261017T164108Z`).
- L3 smoke (`l3-20261017T164141Z`, default parallelism): rc 0, hard 0,
  UNEXPLAINED 0, payload diffs 26 (tolerated static archives).
