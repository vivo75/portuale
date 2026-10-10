# crates/portuale/src/ebuild_phases/run_phase.rs
Running one phase on brush or bash.

## fn run_one_phase
- An earlier version shared one shell across a whole command's prerequisite chain and failed with "cannot mutate readonly variable" on the second phase — the readonly mechanism working as designed. Not a new mechanism: Portage relies on the same resume markers across its separate spawns.

## fn real_bash_path
- `__filter_readonly_variables` (`bin/phase-functions.sh`) lists bash's special variables via `env -i -- "${BASH}" -c '…'`; a real bash sets `$BASH` to itself, but an embedded brush `Shell` is built without a `shell_name`, so brush never sets it and the command comes back empty (`env: '': No such file or directory`). Same binary the `Bash` backend spawns.

## fn new_brush_phase_shell
- Needed so the `bin/*.sh` construct of listing bash's special variables works. `BASH` is set by a shell itself, not taken from the environment, so `phase_setup_script` is not involved.

## struct UmaskGuard
- `bin/ebuild.sh` runs `umask 022`, `phase-functions.sh` `umask 002`. Without the guard one in-process phase leaves its umask behind for every later file the host process (or a parallel test) creates. `/proc/self/status` avoids the `umask(0)` dance.

## fn run_one_phase_brush
- Early return would drop `pump` (declared after `shell`) first and hang the join.
- The exit code (not just a printed error) is what makes a broken setup a hard stop: a broken saved environment must fail the phase, never silently skip the ebuild's functions. `Ok(nonzero)` is a phase failure, like the bash child's status — not a spawn failure.
- `source "${T}"/environment || die` sits at `bin/ebuild.sh`'s top level among the sourcing guards.

## fn run_one_phase_bash
- Mirrors `_doebuild_spawn` more directly than brush's two-step dance. `EBUILD_SH_ARGS="$*"` picks `<phase>` up from argv; the tail really calls `__ebuild_main ${EBUILD_SH_ARGS}` and exits.
- Blocking `std::process::Command`, not `tokio::process`: matches `fetch.rs`'s `wget` precedent (no tokio `process` feature).
- Curated env keeps `EMERGE_DEFAULT_OPTS` and portuale-only vars out of the phase and, since the vdb-`environment` regeneration, out of the vdb.
- A guard scoped to the log-file block would drop (join) before the child spawns, with `cmd` still holding its write-end copies — hanging forever. Hence the escape to the outer scope.
