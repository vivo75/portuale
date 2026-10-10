# crates/portuale/src/ebuild_phases/commands.rs
Runs a phase command list (with logging): fetch once, then each command's phase chain.

## fn run_commands_async
- Origin: the `emerge <atom>` build/merge path passes the resolved `USE` flags in `build_env` because `bin/ebuild.sh`'s `use()` reads the `USE` var, which `phase_env_vars` otherwise leaves `""`. A standalone `ebuild <file> <phase>` passes `&[]` (no resolved graph entry).
- Each phase runs in its own fresh shell, not one shared shell: `bin/ebuild.sh`'s tail makes `EBUILD_PHASE` (among others) `readonly`, so a second phase in the same shell cannot re-export it. This mirrors Portage's `doebuild()`, which spawns a fresh `bin/ebuild.sh` process per phase via `spawnebuild()`. Resume markers (`.pretended`/`.setuped`/`.unpacked`/...) make re-running an already-done prerequisite cheap.
- Fetch ordering matches Portage: `pkg_pretend` runs before fetching (PMS's point for that phase is a fast sanity check needing no network); `setup` likewise precedes it. `A`/`AA` are exported into every phase, matching Portage's environment.
- `DISTDIR` is always exported: Portage's `unpack` (a `bin/ebuild-helpers/` script) resolves `${A}` relative to `${DISTDIR}`, not from anything Rust passes it. Found empirically: a fetched-and-verified distfile still made `unpack` report `"either does not exist or is not a regular file"` before the export was added.
- DISTDIR remap: `doins` passes the real dir as `--distdir`, so absolute symlinks under `doins -r` stay symlinks; the fake is `${PORTAGE_BUILDDIR}/distdir`, filled by `prepare_fake_distdir`.
- `AA` is never exported: Portage's `config.environ()` pops `AA` for EAPI 4+ and the floor here is EAPI 5+.
- Fake-distdir rule mirrors `doebuild.py`'s `mydo` test: prepare when the requested command is not `setup` (unless `FEATURES=noauto`), or the chain contains `install`/`unpack`. Portuale's `noauto` still runs prerequisites and `unpack` needs `${DISTDIR}/<A>`.
- After `install`: `_post_phase_cmds["install"]` is unconditional `bin/misc-functions.sh install_qa_check install_symlink_html_docs install_hooks`, never part of `phase_prerequisites` (unlike `ebuild <file> package`'s separate `__dyn_package` call), so this is the one place it runs. `EBUILD_PHASE` stays `"install"` because `_PostPhaseCommands` reuses the install phase's settings. `MISC_FUNCTIONS_ARGS="$@"` with an unquoted `for x in ${MISC_FUNCTIONS_ARGS}` re-splits on whitespace, so one space-joined string equals three argv entries.
- Ordering is load-bearing for `SIZE`: `_post_src_install_write_metadata` and `_post_src_install_uid_fix` run before the QA sequence, so `SIZE` is the pre-transform image (`ecompress`/`estrip` run later). Observed values for `porttest/docs`: 807 bytes pre-transform vs 372 compressed.
- Soname step mirrors `_commands_exit`: `NEEDED.ELF.2` (6 fields, from the post-`estrip` image), generated `PROVIDES`/`REQUIRES`, and `_inject_libc_dep`'s implicit libc dependency.

## fn run_commands
- Only this path needs an async runtime; `emerge`'s dispatch does not, so it pays for none. Must be `new_multi_thread`, not `new_current_thread`: a single-threaded runtime was confirmed empirically to deadlock partway through a multi-phase run (brush-core's `Cargo.toml` requires tokio's `rt-multi-thread` under unix, the same thing rediscovered the hard way).
- `PORTAGE_TMPDIR` default is `/var/tmp` (Portage's `make.globals`); the builddir adds `portage/<cat>/<pf>`. Read once at the CLI boundary (same shape as `ROOT`/`PORTAGE_CONFIGROOT` via `root_from_env`/`config_root_from_env`), so tests pass a value directly rather than calling `std::env::set_var` from parallel test threads (unsound).

## fn run_commands_logged
- `PORTAGE_LOG_FILE` default is `${T}/build.log`. `run_build_scheduler` passes it for parallel `--jobs` builds; the scheduler dumps the file on failure. `.gz`-suffixed paths come from `emerge_build::build_log_path` (`FEATURES=compress-build-logs`).

## fn open_log_file
- Mirrors `EbuildPhase._open_log`: the file is opened `ab`, wrapped per call in `gzip.GzipFile(mode="ab")` when the name ends `.gz`; readers (failure tail, QA scan) wrap it back in `mode="rb"`. The pump thread is that wrapper: the phase writes plain bytes into two pipes, the thread gzip-encodes them into the file. Portage defers its header to the first write too.
- Lifetime contract: declare the sink early and drop the writer holder explicitly; see the call sites (`run_one_phase_bash`, `run_misc_functions_bash`, `run_one_phase_brush`).

## fn pump_one_pipe
- Both pipes' pumps share one `GzEncoder` under a mutex; stdout/stderr chunk interleaving was never deterministic.

## fn open_log_file (pipe branch)
- `O_CLOEXEC` on the pump's read ends is hygiene only: EOF keys off the write ends.
