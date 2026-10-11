# crates/portuale/src/remote/tests/stages.rs
Merge-driver and regen-stage behaviour tests.

## tests: merge_driver_never_writes_the_existing_inode
- Backlog #97's core invariant for the remote driver. The old `cp -p "$src" "$dest"` truncates that inode: fatal for an mmap'd `libc.so.6` / `libreadline.so.8` in a running client (backlog #96).

## tests: merge_driver_replaces_a_running_executable_atomically
- The live shape of backlog #97: a client re-merging a binary that is currently executing (its own `/bin/bash`, or glibc's ld.so). The old `cp -p` hit `ETXTBSY` and failed the copy step; the temp+rename replacement succeeds. `/bin/sleep` stands in for the shell: the same execve-protected kernel path.

## tests: merge_driver_aborts_on_unowned_collision
- No protect-owned nuance in v1.

## tests: regen_postinst_rewrites_vdb_env_from_merge_time_env
- Backlog #171: a merge through the driver plus the regen postinst run leaves `var/db/pkg/<cat>/<pf>/environment.bz2` whose decompressed content is the postinst-time env. A variable the test sets only in `pkg_postinst` is present, the stray build-time `x=""` is gone (Portage `ebuild.sh`'s own `unset path seen i x` after sourcing), `FEATURES` / `PORTAGE_FEATURES` are the resolved merge-time value (not the build-time `buildpkg` one). Portage's vdb has `environment.bz2` only.

## tests: regen_runs_without_pkg_postinst
- Backlog #171: Portage's postinst `EbuildPhase` always starts (`phase-functions.sh:1061-1083` runs the hooks, no-op when undefined, then the `PORTAGE_UPDATE_ENV` block), so the phase rc is 0 and the env carries the merge-time `FEATURES`.

## tests: regen_matches_portage_o_shell_and_locale
- Backlog #171b tasks 2-3, stage level: Portage `doebuild.py:475` sets `mysettings["O"]` but `config.environ()` drops it via `special_env_vars.py: environ_filter`), records `SHELL` as `declare --` (bash's own, unexported; Portage has no exported `SHELL` to inherit). Carries the injected locale values as `declare -x`.

## tests: regen_splits_lc_all_like_portage
- Backlog #171c task 1, stage level: bed run `l31-20260927T052232Z` shows Portage with `LANG` + split categories where `mrg` echoed `LC_ALL` back.

## tests: remote_client_config_stacks_server_make_globals
- Backlog #171 follow-up 3 (owner Q9 = b): a synthetic server root carries `usr/share/portage/config/make.globals` with `FEATURES="news sandbox"`; the pulled client `make.conf` says `FEATURES="-news sign"`. Portage's incremental fold (`const.INCREMENTALS`, `config.py:446-499`) resolves that to `sandbox sign`, and the regen'd vdb env carries exactly that list (not the build-time `buildpkg` one). `PORTAGE_CONFIGROOT` is pinned to the synthetic server root (process-global; see `PLACED_CONFIG_ENV_LOCK`) and ambient `FEATURES`/`INSTALL_MASK` cleared so the host cannot leak into the hermetic resolve.

## tests: install_keeps_build_time_env_when_pull_fails
- Backlog #171, failure path: a forced pull failure (no `environment.regen` in the unit).

## tests: install_file_atomic_removes_tmp_when_mv_fails
- Backlog #171 review (local transport; the ssh branch's `rm -f` needs a live client and is not covered here).

## tests: install_file_atomic_ssh_stages_bytes_before_transport
- Backlog #262: `TempDir::new` creates a directory (backlog #260), so staging straight into its path fails with `Is a directory (os error 21)` on every ssh install. The regen'd env never reaches the vdb and the build-time env (with its stray `declare -- x=""`) survives. The assertion pins exactly the staging/transport boundary.

## tests: scrub_bzip2_command_rewrites_only_the_passthrough
- To the resolved client value when one is configured, to the `make.globals` default otherwise.

## tests: ship_old_hook_envs_warns_when_both_pulls_fail
- Backlog #171 review: no more silent staging failures.

## tests: old_hook_uses_server_decompressed_env_without_client_bzip2
- Backlog #171: staged at `$UNIT/old-env/<pf>`.

## tests: old_hook_falls_back_to_plain_when_bz2_present_without_client_bzip2
- Backlog #171 review: an older-`mrg` entry with both files. The bz2 is unreadable without a client bzip2, so the plain file wins.

## tests: old_hook_falls_back_to_legacy_plain_environment
- Backlog #171, legacy fallback: a vdb entry written by an older `mrg`.
