# crates/portuale/src/emerge_build/tests/source_merge.rs
Source-merge loop tests (serial, parallel, keep-going, and kill behavior).

## fn source_merge_with_resolved_config_threads_use_and_slot_into_the_phase
- #37 S2 end-to-end, under both backends. The resolved (folded) `FEATURES` must win over `phase_env_vars`' raw process-env base. `build-info/USE` is what Portage's `__dyn_install` writes from the phase `${USE}`; the `.keep_*` marker is written by the external `ebuild-helpers/keepdir` subprocess, which is why an unexported `SLOT` used to produce a bare `-` (S0's `l2-env-*` findings).

## fn source_merge_second_package_sees_env_d_installed_by_the_first
- Backlog #332 end to end: the merge's `env-update` rewrites `<root>/etc/profile.env` (`ENVDRELOAD_VAR=1`); Portage reloads config before every package task (`Scheduler._allocate_config`), as the bed's `helper-doins` / `PT_HELPER_DOINS` pair showed.

## fn source_merge_protects_the_config_fold_and_env_d_added_paths
- Backlog #333: `CONFIG_PROTECT` resolved from the config (`/etc /usr/share/cfgprotme`), none in the process env. `cfgprotlate-a` installs `/etc/env.d/99cfgprotlate` (`CONFIG_PROTECT="/usr/share/cfgprotlate"`) and its merge runs env-update; `cfgprotlate-b` then installs one file under each path over a locally modified copy. Expected value from Portage 3.0.82.2 in the bed image (`env -i emerge`, same shape: pmtest `differential-test-bed/scripts/333-config-protect.sh`): both local files survive with `._cfg0000_x` / `._cfg0000_y`. The config's fold protects `x` (`dblink._get_protect_obj`); the per-task `config.reload()` protects `y`.

## fn source_merge_omits_a_vdb_aux_file_the_ebuild_unset_in_an_earlier_phase
- Backlog #160: Portage's `config.environ()` `filter_calling_env` (bug #189417) is what keeps a variable the ebuild `unset` in an earlier phase unset: every phase after `${T}/environment` exists spawns with only the `special_env_vars.environ_whitelist` keys from the config/calling env, and the saved environment supplies the rest. `unsetcflagspkg`'s `src_compile` is `dev-build/ninja-1.13.2-r1`'s own `unset CFLAGS` shape, so Portage's `__dyn_install` writes no `build-info/CFLAGS` and the vdb gets no `CFLAGS` row (the l3-20260925T074707Z Class 4 row).
- The `chgrp` comment: Portage's `bin/phase-functions.sh:1257` chgrps `${T}/environment` to `${PORTAGE_GRPNAME:-portage}`; unprivileged, the default `portage` group makes that fail with a per-phase `chgrp: … Operation not permitted` line on harness stderr. Ownership is unchanged by the gid redirect, so the aux-file assertion is not weakened.

## fn reinstall_leaves_a_byte_identical_file_a_preinst_helper_would_remove
- Backlog #158: without `REPLACING_VERSIONS` the fixture's preinst `rm`s the file; with it the preinst returns early and `merge_tree`'s `needs_move` (Portage's `_needs_move`) leaves the existing inode. The property that keeps Portage's OWNER (`/usr/share/man/man1/awk.1`'s `1:1`).

## fn source_merge_pre_cleans_a_stale_installed_image
- Backlog #42 regression on the serial `emerge` source path, exactly like Portage's `_emerge/EbuildBuild._start_pre_clean`. Previously `install` printed "already installed; skipping" and merged the stale image, which is how a `-B` after a source merge shipped stripped binaries (#38 S4).

## fn a_hard_failure_kills_still_running_builds_instead_of_waiting_them_out
- Ports Portage's `Scheduler._keep_scheduling`/`_terminate_tasks`: once any package fails without `--keep-going`, Portage sends kill signals to whatever is still running rather than letting it finish, since its result would be discarded regardless. `schedbad` fails in `install` (the last phase); `schedslow` sleeps 120s in `compile`, well before `install`. The 60s bound is generously chosen for loaded machines (backlog #308).

## fn run_source_merge_upgrade_replaces_the_installed_version
- Merge `binpkgrmpkg-1.0` (`New`), then 2.0 (`Upgrade`). The fixture's five hooks each append `<phase>-<PVR>` to a ROOT log.
