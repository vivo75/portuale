# crates/portuale/src/ebuild_phases/tests/sandbox.rs
Sandbox/isolation unit tests for phase execution. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn sandbox_wrapped_command_configures_the_real_addrconfig_workaround_addresses
- Ports Portage's `_configure_loopback_interface` (lib/portage/process.py:838-871), the workaround for bug #690758: `ip link set lo up` plus a non-loopback-looking address per family, so glibc's `getaddrinfo()` with `AI_ADDRCONFIG` set doesn't return spuriously empty results for a build that only ever talks to `localhost`.

## fn network_sandbox_exempt_matches_portage_doebuild_spawns_own_formula
- Expected value from Portage's `doebuild` spawns formula: `PROPERTIES=live` only exempts the unpack phase; `PROPERTIES=test_network` only exempts the test phase; `RESTRICT=network-sandbox` exempts every phase.
- USE-conditional groups are already dropped by the time these strings arrive (`restrict_and_properties`'s job), so a raw conditional token string is simply never matched.

## fn feature_gates_use_the_resolved_features_string
- From #37 S3: the phase-execution gates read the resolved `FEATURES` threaded on `extra_env` (`features_string`), never the raw process env -- a `make.conf` `-sandbox` disables the sandbox even when the calling env exports `sandbox`. The helpers take the features string as a parameter precisely so this is directly testable.
- The `extra_env` pair wins over the process env (here: whatever the test runner exported); an absent pair falls back to it.

## fn environ_whitelist_keeps_qemu_prefix_like_portage_3_0_83
- Portage `special_env_vars.py:256` (`f7bfe34de`, bug 978685). Expected values from the host's source and the L1 bash merge (`LLM/evidence/2026-10-24-vdb-env/`).

## fn environ_whitelist_has_the_portage_3_0_83_additions
- Portage `special_env_vars.py` (`5dc9b552e`, `c15ab2464`, `b7052a33d`, `1bad86d92`).

## fn demoted_spawn_runs_as_the_portage_user
- Root-gated (`SKIP: needs root` otherwise): `spawn_bash_script` with `droppriv: Some(ids)` runs `id -u`, which must print the portage uid -- an undemoted child reports 0 and fails the assert (structural negative control).

## fn droppriv_mirrors_portage_actionmap_and_gate
- Portage's truth table verbatim (Portage `doebuild.py, _spawn_actionmap (:1780-1795)` and `actionmap (:1813-1870)`): the five demoting phases, seven non-demoting ones, and each gate clause (`RESTRICT` `userpriv`/`nouserpriv`, `-userpriv`, non-root).

## fn portage_phase_ids_resolves_a_real_user_and_rejects_unknown_ones
- `root` resolves uid 0 with its primary gid present in `groups`; an unknown name returns `None` (degrade, not panic) -- Portage `data.py, portage_uid/portage_gid (:169)` and `userpriv_groups (:224-247)`.

## fn post_phase_perms_force_owner_modes
- Portage `_post_phase_userpriv_perms` (Portage `doebuild.py:2485-2498`): dirs gain owner rwx (0o700 from mode 0), files 0o600, a symlink is chowned-only and its target keeps its mode; the fixture starts at 0o000 because mask 0 only adds bits.

## fn pre_phase_dirs_become_portage_owned
- Portage `prepare_build_dirs.py, prepare_build_dirs (:96-106)` and `_prepare_workdir (:329-368)`: all six spine dirs end owned by the ids at exactly 0o700, except WORKDIR which starts 0o755 and keeps its group/other bits (OR semantics, not chmod-set).

## fn pre_phase_symlinked_spine_entry_is_skipped_not_followed
- A `${T}` symlink stays a symlink, its target keeps its mode and gains no `logging` child; the guard is load-bearing (deleting it creates `outside/logging` through the link).

## fn unshare_flags_root_omits_userns
- Portage `process.py, _exec (:1061)`: root gets no `--map-root-user`, non-root keeps it first. A single-id userns maps only the caller, so portage-owned paths become overflow uid.
