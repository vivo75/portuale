# crates/portuale/src/ebuild_phases/tests/runtime_paths.rs
Runtime-path resolution unit tests (bin overlay, bin-dir order, scratch base, pym path, FEATURES layering). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn remove_bin_overlay_dir_is_idempotent
- Backlog #88: the `atexit` handler's contract -- removes a populated overlay, then silently tolerates the missing dir (double exit paths, or an overlay a previous run already reclaimed, must never fail).

## fn resolve_bin_dir_override_beats_the_build_tree_and_never_falls_through
- Backlog #322, `resolve_bin_dir`'s order: a set override wins over the build tree; an override that is wrong is an error naming the variable and never falls through (even though the build tree is fine, and even though an embedded copy could stand in); an empty one is unset. A directory without ebuild.sh is wrong too.

## fn resolve_bin_dir_with_nothing_found_explains_itself
- Backlog #322: neither source present (no embedded copy) is an error that explains itself, with the three facts the user needs (what, where it looked, how to fix it) and no program prefix (callers add their own); one line.

## fn resolve_bin_dir_falls_back_to_the_embedded_copy_only_when_nothing_is_on_disk
- Backlog #322 S2.3: with no override and no build tree, the embedded copy is the answer (never an error); a build tree that has the runtime still wins over it.

## fn the_scratch_base_skips_noexec_directories
- Note: the previous doc comment on this test described a different test (backlog #322 end to end: a run that needs the phase runtime but is pointed at a directory without one exits 1 with one message -- no panic, no abort, since the release profile aborts on panic, which is what produced the core dump -- and `mrg --remote-binpkg` does so before touching the client, here the local-transport ROOT). That text is preserved here verbatim in case it belongs to a neighboring test; the body below only pins `pick_scratch_base`/`is_noexec`.
- With nothing usable the first candidate is returned, so the caller's error names it. The probe treats a directory it cannot inspect as unusable.

## fn pym_path_and_pythonpath_values_follow_the_checkout
- Backlog #326 S1: the `PORTAGE_PYM_PATH` / `PORTAGE_PYTHONPATH` decision factored out of `phase_env_vars` / `invoke_dyn_package` -- a checkout whose `lib/portage/` is a dir yields that `lib`, anything else yields the `/` fallback (resp. no `PORTAGE_PYTHONPATH`). No process-env mutation: the checkout path is passed in directly.

## fn phase_setup_script_exports_extra_env_features_last
- From #37 S2 step 2: `extra_env` carries the resolved `FEATURES` and is appended last, so it must win over `phase_env_vars`' own `phase_features_value()` base (the raw process env). Verified as brush script composition here; the live-phase execution proof is `emerge_build::tests::source_merge_with_resolved_config_threads_use_and_slot_into_the_phase` (which asserts the `FEATURES=` marker under both backends).
