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
