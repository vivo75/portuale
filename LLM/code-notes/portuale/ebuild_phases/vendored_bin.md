# bin/ (vendored Portage phase runtime), tests in crates/portuale/src/ebuild_phases/tests/environment.rs
What the 3.0.83 re-sync (#505, plan 02.442 S6) changed in the vendored scripts and why the portuale-side pieces stay untouched.

## test vendored_filter_readonly_variables_matches_portage_3_0_83
- Sources `isolated-functions.sh` and `phase-functions.sh` under `env -i` bash and feeds `__filter_readonly_variables --filter-locale` a `declare -p` dump: `LOCPATH` and `LANG` go, `QEMU_*` and ordinary variables stay. Also greps `phase-functions.sh` for portuale's two local changes so a future re-sync that drops one fails here.

## bin/phase-functions.sh
- Local change 1: `___save_and_filter_ebuild_env` replaces the three `__save_ebuild_env | __filter_readonly_variables` pipelines (brush strategy #2). Local change 2: `filtered_vars+=( "PORTUALE_.*" )` (feat#157 S6.4). `bin/README.md` said "one" in one place; the header comment now names both. Reapplied in S6 by `git diff --no-index` of the 3.0.82.2 upstream file against the old vendored file, then `patch` onto the 3.0.83 file (all six hunks applied with offsets only).
- 3.0.83 changes arriving with it: `LOCPATH` filtered with `--filter-locale` (`c15ab2464`); QEMU variables excluded from the bash-variable list so an ebuild-set `QEMU_*` survives `__filter_readonly_variables` (bug 978685, `f7bfe34de`), which is why a source-built package's `environment.bz2` now lists `QEMU_SOFTMMU_TARGETS`/`QEMU_USER_TARGETS` like Portage; `__ebuild_arg_to_phase` with an out-variable (fewer subshells); `printf -v` in `__dyn_install`.

## bin/isolated-functions.sh, misc-functions.sh, phase-helpers.sh
- New `__ebuild_ipc` and `__ebuild_exit` (`isolated-functions.sh`) call `ebuild-ipc.py` or write to `PORTAGE_EBUILD_EXIT_FD`. Every call site is under `[[ -n ${PORTAGE_IPC_DAEMON} ]]` (`die`, `misc-functions.sh` tail, `__ebuild_main` tail, `has_version`/`best_version`), and portuale drops `PORTAGE_IPC_DAEMON` (`commands.rs` `spawn_bash_script`, `merge_script.rs`), so `ebuild-ipc.py` is not vendored and the portuale `ebuild-ipc` shim stays unreachable. `has_version` without the daemon still runs `portageq-wrapper` with `env SYSROOT=/ [EPREFIX=...]`.
- `eapply_user` now writes `build-info/user_patch.digests` and `build-info/USER_PATCHES` (sha256 of each applied patch; consumed by S10, #510). `install_qa_check` reads `file -S -r -0 -f -` once per scan for NEEDED.ELF.2 soname inference instead of one `file` per object. `save-ebuild-env.sh` lists the two new functions so they are not saved into `environment`.
