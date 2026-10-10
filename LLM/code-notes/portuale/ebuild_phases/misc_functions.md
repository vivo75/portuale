# crates/portuale/src/ebuild_phases/misc_functions.rs
`misc-functions.sh` steps (`install_qa_check ...`, `__dyn_package`).

## fn run_misc_functions
- `doebuild()` invokes `"package"` as a separate script (`misc_sh = ... + " __dyn_%s"`), confirmed by reading `phase-functions.sh`: `__ebuild_main` has no `"package"` branch. `misc-functions.sh` sources `bin/ebuild.sh` (same environment/ebuild-sourcing as `run_one_phase`), captures its positional args into `MISC_FUNCTIONS_ARGS` before sourcing (so `ebuild.sh` arg handling never sees them), then runs `for x in ${MISC_FUNCTIONS_ARGS}; do ${x}; done`.
- `_PostPhaseCommands` passes only `ld_preload_sandbox`, never `networked`: no `unshare`. `_spawn`'s silent `free = True` fallback (missing binary degrades unsandboxed) is matched here with an explicit one-shot warning instead.

## fn run_misc_functions_bash
- Blocking `std::process::Command`, not `tokio::process`: matches `fetch.rs`'s precedent for spawning from inside an `async fn` without tokio's `process` feature.
- The curated env mirrors `run_one_phase_bash`'s `env_clear` (L1-f): only `environ_whitelist` keys pass through, so `EMERGE_DEFAULT_OPTS` and portuale-only vars never reach the phase or, since L1-c, the regenerated vdb `environment`.
- `PORTAGE_IPC_DAEMON` is dropped: there is no IPC daemon, and a leaked value would send `has_version` down the `ebuild-ipc` branch.

## fn shared_runtime
- The four sync entry points (`run_misc_function`/`run_commands_logged`/`run_single_phase`/`run_phase_from_saved_env`) each spun up and tore down a fresh `Builder::new_multi_thread()` runtime: one `emerge` merging many packages paid setup/teardown per phase per package.
- `OnceLock` rather than a threaded-through value avoids a signature change across `ebuild.rs`/`ebuild_merge.rs`/`ebuild_package.rs`/`emerge_build.rs`. Safe because `run_one_phase`'s async body never calls back into any of the four. Process exit reclaims the pool.
