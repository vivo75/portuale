# crates/portuale/src/ebuild_phases/tests/phase_runs.rs
Phase-run integration tests (subprocess-driven). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn a_missing_phase_runtime_is_an_error_not_a_panic_for_remote_and_ebuild
- No doc comment; the body pins that a run pointed at a directory without a phase runtime exits 1 with one message naming the variable (no panic/abort -- the release profile aborts on panic), and that `mrg --remote-binpkg` fails before touching the client.

## fn features_test_passthrough_actually_runs_src_test
- A subprocess invocation per run, not `std::env::set_var` -- `run_commands`' doc already establishes that mutating process-global env vars is unsound from parallel test threads. Spawning the `portuale` binary gives each invocation its own independent environment, exactly the way Portage's own `FEATURES` behavior is env-var-scoped.
- `CARGO_BIN_EXE_<name>` is only set for `tests/` integration targets, not for a unit test compiled into the crate's own binary -- derive the sibling `portuale` binary's path from this very test binary's own path instead (`target/<profile>/deps/portuale-<hash>` -> `target/<profile>/portuale`).
- Portage's `__dyn_test` (`phase-functions.sh:559`): `src_test` is a no-op ("Test phase [not enabled]") unless `FEATURES` contains `test` -- previously always the case here regardless of the outer `FEATURES` the invoker actually asked for, since the phase's own exported `FEATURES` was unconditionally blanked (`phase_features_value`'s doc).

## fn phase_has_version_uses_native_portageq
- From feat#157 S6 / #151: `has_version`/`best_version` in a phase go through the tracked `bin/portageq-wrapper` shim to the native `portuale portageq`, on the files tree and (with the feature) on a sqlite database, whether or not `3rdparty/portage` is present. Only the sqlite block adds variants.
- An empty ROOT-owned database: samepkg is only "installed" on the files tree, so a native helper reading the wrong backend would answer differently. Fresh builddir per variant: a finished phase leaves its `.pretended` marker behind and is then skipped.
- `has_version` without `-r` reads the vdb at ROOT: an existing but empty tree answers 1 (a missing one exits 64).

## fn replacing_the_running_binary_mid_run_keeps_later_phases_working
- #331: replacing the running binary's file mid-run must not break the later phases. `pkg_setup` atomically replaces `PORTUALE_BIN` (a private copy of the release binary, the way `cargo build` or a merge of a new portuale does), then `src_unpack` calls `has_version` through `portageq-wrapper`. Before the fix the second phase got `PORTUALE_BIN=<path> (deleted)` from `current_exe()` and the shim died with `No such file or directory`. Portage has no self-exec: its helpers are files under `PORTAGE_BIN_PATH` that a portage merge replaces in place, and later phases run the new files, which is what a path resolved once gives too.
- An external `cp`, not `std::fs::copy`: a sibling test thread that forks while this process holds the copy open for writing would keep that fd until its exec, and exec'ing the copy then fails with ETXTBSY.

## fn portage_ipc_daemon_in_the_calling_env_never_reaches_a_phase
- #326 S2/D6: `PORTAGE_IPC_DAEMON=1` in the calling environment must not reach the phase, so `has_version` keeps going through `portageq-wrapper` (`phase-helpers.sh:911-915`) and answers for real. (Without the drop, the phase would take the `ebuild-ipc` branch and die on its 127.)

## fn portuale_bin_runs_helper_ping_inside_a_phase
- #326 S2/D8: `$PORTUALE_BIN __helper ping` from inside a phase prints the fixed token (proves `PORTUALE_BIN` is a working binary, never the libtest harness).

## fn portage_python_is_the_shim_unless_real_mode
- #326 S2.3/D2: `PORTAGE_PYTHON` in the phase env is the `portuale-python` shim in native mode and `/usr/bin/python` with `PORTUALE_PYTHON_HELPERS=real` (which is also forwarded). Fresh builddir per run: a finished phase leaves its marker behind and is then skipped (so the second run would print nothing).

## fn relocated_unpack_and_docompress_use_only_native_or_vendored_helpers
- #326 S2 relocated e2e (the #322 shape): the phase runtime comes from the embedded copy (`PORTUALE_BIN_DIR` at an extracted tree, which is what a relocated binary extracts; in-tree an unset `PORTUALE_BIN_DIR` would resolve to the live tree instead) with `PORTUALE_PORTAGE_CHECKOUT=/nonexistent`, so every helper the phases reach is native or vendored. `unpack` of an odd-mode tarball must leave the Portage-oracle modes (the `helper-unpack` probe shape), and `docompress` must compress through the vendored `ecompress-file`.
- No `dodoc`/`doins`: those route through `doins.py`, which has no native port yet (a later slice); `cp` keeps this probe on the S2 helpers only, like the pmtest `helper-unpack` and `helper-docompress` probes.
- A missing `chmod-lite` is silent in Portage (`find:` on stderr, exit unexamined) and leaves odd modes: both halves are pinned. A missing `ecompress-file` is fatal in Portage (`__helpers_die`); both the absence of its error and the compressed result are pinned.

## fn ebuild_setup_without_a_checkout_has_no_pym_path_complaint
- Backlog #326 S1, env-level: with no Portage checkout, a phase still runs without any `PORTAGE_PYM_PATH` complaint -- without `PORTAGE_PYM_PATH does not exist` or `null directory`.
- `clean` would be the obvious probe (Portage's `__dyn_clean`, `bin/phase-functions.sh:316`, `cd`s into it unconditionally at `:331-332` upstream / `:401-402` vendored), but standalone `portuale ebuild … clean` is a recognized-but-unimplemented dry-run stub (`ebuild.rs`'s own test names it) that never executes a phase. `setup` is implemented and reaches the same variable twice: Portage's `bin/ebuild.sh`'s own top-level cwd choice, and the end-of-phase env-save `cd "${PORTAGE_PYM_PATH}"` (vendored `bin/phase-functions.sh:1260`, upstream `:1189`).
- A subprocess invocation per run, not `std::env::set_var` (see `features_test_passthrough_actually_runs_src_test`): each run gets its own environment. `PORTUALE_BIN_DIR` is left unset so the normal runtime (build tree or embedded) is used.

## fn install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer
- Regression test for an upstream brush bug (fixed in the pinned fork, see LLM/what-this-proves.md's eclass section for the full writeup): `bigfixture.eclass` defines ~400 functions so that Portage's `bin/phase-functions.sh`'s own post-phase `__save_ebuild_env | __filter_readonly_variables` pipe (both sides shell functions) carries well over the OS pipe buffer size (~64KiB on Linux) worth of `declare -f` output. Before the fix, brush ran a function used as a non-last pipeline stage inline rather than as a background task, so the pipeline-spawning loop blocked on `__save_ebuild_env` fully returning before even spawning `__filter_readonly_variables` to drain it -- a reproducible deadlock, not a slow completion (confirmed against app-arch/xz-utils and sys-fs/fuse before the fix, which both inherit the `multilib` eclass family). Run on a background thread with a hard deadline so a regression here fails this test outright instead of hanging the whole suite.
- A generous deadline: this drives a full brush phase chain (many subprocesses), so under a heavily parallel `cargo test` it can legitimately take tens of seconds -- a pipe-buffer deadlock would still hang indefinitely and be caught.
