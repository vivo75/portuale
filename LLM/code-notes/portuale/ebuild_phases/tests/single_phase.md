# crates/portuale/src/ebuild_phases/tests/single_phase.rs
Single-phase and clean-phase end-to-end tests. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn run_clean_removes_a_stale_installed_marker_image_and_temp
- Backlog #42: the Portage `clean` phase (`bin/ebuild.sh clean` -> `__dyn_clean`) is what Portage's `_emerge/EbuildBuild._start_pre_clean` runs before every build. Given a stale `.installed` marker and a populated `image/` + `${T}`, it must remove all three -- exactly the state that made a rebuild silently skip `install` and repackage an already-`instprep`ped image (#38 S4).

## fn run_clean_with_keepwork_keeps_temp_and_workdir_but_still_drops_the_marker
- `FEATURES=keepwork` (Portage's `__dyn_clean` guard): `clean` still drops the stale `.installed`/`image`, but keeps `${T}` and `WORKDIR` -- the documented Portage way to inspect a build.

## fn run_single_phase_actually_runs_pkg_config_and_pkg_info
- End-to-end proof that standalone `config`/`info` (Portage's `ebuild.rs` routing to `run_single_phase`, see `is_standalone_phase_command`) actually run the `pkg_config`/`pkg_info` phase functions -- not just that `run_single_phase` returns successfully. No `install` chain involved at all, matching Portage's standalone usage (an admin runs `ebuild <file> config` directly against an ebuild, with no merge/vdb step in the same invocation).

## fn run_single_phase_actually_runs_pkg_prerm_and_pkg_postrm
- End-to-end proof that standalone `prerm`/`postrm` actually run the `pkg_prerm`/`pkg_postrm` phase functions -- same shape as the config/info test above, but for the two standalone commands that also have a separate internal use (`ebuild_unmerge::run_unmerge`, see `is_standalone_phase_command` for why that internal use and this standalone path are simply two independent ways to reach the same phase function). No `unmerge`/vdb step involved here, matching Portage's standalone usage.
