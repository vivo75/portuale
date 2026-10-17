# crates/portuale/src/pretend/env_config.rs
Config and repo loading, EMERGE_DEFAULT_OPTS, scheduling policy.

## fn resolve_root_deps_running_root
- `ESYSROOT` collapses to `BROOT` when nothing is cross-compiling. A strict no-op when target and running roots coincide (every ordinary `ROOT=/` invocation). `--root-deps` (bare / `=True`) stays the only trigger when `ROOT` is the running root (its Portage debugging purpose). `=rdeps` is a complete no-op at EAPI 7+ and needs nothing here. Approximated as the same running-root walk -- see `portage_repo::root_deps_satisfied_atoms`.

## fn package_options_from_env
- The unmerge half ports `_unmerge_display` (the preview) then `dblink.unmerge()` (`pkg_prerm` from that version's own vdb-saved env -> delete its files -> `pkg_postrm`) + `dblink.delete()` (drop the vdb dir), via `ebuild_merge::unmerge_one_installed`. On success each removed package is deselected (Portage `WorldSelectedPackagesSet.cleanPackage`, called per-package right after its removal). The trailing `for s in setconfig.active: selected.remove(@s)` pass is a no-op here (portuale's world writer already drops `@set` lines on any rewrite). A `pkg_prerm`/`pkg_postrm` failure is logged and removal continues (`unmerge_one_installed`); Portage `unmerge()` `sys.exit`s on a `portage.unmerge()` non-zero, but that return only tracks the file-removal core, which portuale still surfaces as a hard `Err`.
- The compression half: backlog #173's mechanism, reused for backlog #180. The gpkg pair rule is backlog #180's Q6 rule (pmtest `bf0692f`): Portage's `environ_filter` (`_config/special_env_vars.py:280-281`) keeps a calling-environment value out of the phase env, and `bin/gpkg-helper.py` rebuilds `portage.settings` from that filtered env plus the config files. `PORTAGE_BZIP2_COMMAND` keeps the full chain for both formats (whitelisted, never filtered), as does `PKGDIR`.
- `resolved_features`: `Some` means the caller resolved a config (backlog #37 S2/S3). The `config` parameter is independent of it: `None` is reached only when a caller could resolve no chain (today only `execute_unmerge` when `resolve_unmerge_config` fails, which otherwise passes its resolved config); every production caller passes its resolved config, including `execute_unmerge`.
- The config-less arm goes through the same `config_env_var` test hook; only `execute_unmerge`'s `FEATURES=unmerge-backup` quickpkg reaches it, and only when the chain resolution there failed.
- The Q6 split: `xpak` compresses through the full chain (Portage `doebuild.py:697` reads the package's own `mysettings`, env layer highest); `gpkg` through the files only (Portage `bin/gpkg-helper.py:49`).
- `buildpkg_live`: unlike the sibling fields, this does not fall back to `d.buildpkg_live` on an unset `FEATURES` var. Resolved (production): the token's presence is the answer (`-buildpkg-live` was already folded away). Raw fallback: the old default-on-unless-negated read. See `ebuild_package::PackageOptions::buildpkg_live` for the full Portage grounding.
- `binpkg_multi_instance`: honoured by both `package_after_install` and `quickpkg_from_vdb` (the `FEATURES=unmerge-backup` path). See `ebuild_package::PackageOptions::binpkg_multi_instance`.
- Portage `_emerge/unmerge.py::unmerge`'s own removal loop, reached when `emerge -C`/`--unmerge <atoms>` runs **without** `--pretend`: after `_unmerge_display` (the preview above) returns, each `selected` package is really removed -- `>>> Unmerging (N of M) <cpv>...`, then `dblink.unmerge()` (`pkg_prerm` from that version's own vdb-saved env -> delete its files -> `pkg_postrm`) + `dblink.delete()` (drop the vdb dir), via `ebuild_merge::unmerge_one_installed`.
- The caller (`run_unmerge_pretend`) already handled `--ask` (exit 130 on decline) and the `CLEAN_DELAY` countdown before reaching here; `FEATURES=unmerge-backup` is honored inside `execute_unmerge` (a `quickpkg` of each package first).

## fn shell_split
- Enough for the `ionice -c N -p ${PID}` shapes this variable ever carries; an unterminated quote just consumes the rest of the string. Ports Portage `shlex.split`.

## fn args_with_emerge_defaults
- Ports `_emerge/main.py`'s second `parse_opts` pass (`:1352-1360`): the value is read from the resolved config (`config.other_vars`, where `portage-profile`'s `ENV_SCALAR_VARS` stores it -- process env over `make.conf`, including `source`d files), matching Portage's `emerge_config.target_config.settings.get(...)`. An unset variable yields an empty prefix (same allocation-wise shape as before this existed). `--ignore-default-opts` comes from the command line (the first parse pass, before defaults exist: `main.py:1352-1353`).
- Portage `_emerge/main.py`'s second `parse_opts` pass (`:1352-1360`): `shlex.split(settings["EMERGE_DEFAULT_OPTS"])` is **prepended** to argv, so a command-line option always wins over the same option in the variable.

## fn apply_portage_scheduling_policy
- Ports `apply_priorities` (via `run_action`, before any action): `PORTAGE_NICENESS` (`renice -n <n> <pid>`), `PORTAGE_IONICE_COMMAND` (`shlex.split`, `${PID}`-expanded, spawned) and scheduling policy (`set_scheduling_policy` -> `os.sched_setscheduler`). A missing `renice` prints the "renice command was not found" line and gives up; a non-zero exit prints an eerror-style line and continues. `PORTAGE_IONICE_COMMAND` failing because the tool is absent is silent (Portage: "kernel probably doesn't support ionice").
- Investigated and confirmed a non-issue, not a gap: Portage also applies this to a Python `multiprocessing` "forkserver" pid when `multiprocessing.get_start_method() == "forkserver"` (`actions.py:3250-3271`) -- a long-lived daemon process Portage's own `Scheduler` forks worker processes from under that specific start method, so the forkserver's own policy (not just the main process's) determines what those forked workers inherit. Portuale has no equivalent: `emerge_build::run_build_scheduler`'s `-jN` parallelism is OS threads (`std::thread::scope`), and every build subprocess (`ebuild_phases::spawn_trackable`) is spawned directly from whichever thread needs it via a plain `fork`+`exec`, never through a persistent forker. `sched_setscheduler` applied here, on the main thread before any worker thread exists (this runs once at the very top of `run()`, well before `run_build_scheduler`'s `std::thread::scope`), is therefore already sufficient: POSIX thread creation inherits the creating thread's policy by default (`PTHREAD_INHERIT_SCHED`), confirmed empirically on Linux -- so every worker thread, and every subprocess it spawns, already gets the same policy. No Python-reference mirror needed either way: a process/thread concern, not a `--pretend` resolution one.
- Portage `_emerge/actions.py::apply_priorities` (via `run_action`, before any action): applies `PORTAGE_NICENESS` (`renice -n <n> <pid>`), `PORTAGE_IONICE_COMMAND` (`shlex.split`, `${PID}`-expanded, spawned) and `PORTAGE_SCHEDULING_POLICY` / `PORTAGE_SCHEDULING_PRIORITY` (Portage `set_scheduling_policy` -> `os.sched_setscheduler`) to this process, so every build/merge subprocess it later spawns inherits them.
- `sched_setscheduler` applied here, on the main thread, before any worker thread is ever created (this runs once, at the very top of `run()`, well before `run_build_scheduler`'s own `std::thread::scope` call), is therefore already sufficient: POSIX thread creation inherits the creating thread's own scheduling policy by default (`PTHREAD_INHERIT_SCHED`), confirmed empirically on Linux (a `std::thread::spawn`ed thread reports the same `sched_getscheduler` value the main thread set moments earlier) -- so every scheduler worker thread, and every subprocess it in turn spawns, already gets the same policy this call establishes, with nothing further to do.

## fn apply_scheduling_policy
- Default priority is `sched_get_priority_min(policy)`. A failing syscall (e.g. EPERM for a real-time policy without `CAP_SYS_NICE`) prints the "Unable to apply" line. IDs sourced from linux/sched.h (Portage `set_scheduling_policy`).
- Zero-init mirrors CPython building `struct sched_param` as a C11 compound literal with only `sched_priority` assigned (every other member zero, including musl's four SCHED_DEADLINE fields, which the libc crate's glibc definition does not carry -- backlog #61).
- Portage `_emerge/actions.py::set_scheduling_policy`: map `PORTAGE_SCHEDULING_POLICY` to a `SCHED_*` constant (`linux/sched.h` IDs), resolve `PORTAGE_SCHEDULING_PRIORITY` (default `sched_get_priority_min(policy)`), and `sched_setscheduler(2)` this process.
- Zero-init, then set the one field Portage's `os.sched_param` conversion sets: CPython builds `struct sched_param` as a C11 compound literal with only `sched_priority` assigned, so every other member is zero -- including musl's four SCHED_DEADLINE fields, which the libc crate's glibc definition does not carry (a plain struct literal naming only `sched_priority` fails to compile for musl, backlog #61).

## fn buildpkg_from_config
- The INSTALL_MASK half: `MergeOptions::from_env` reads the same two from the environment as its `ebuild <file> merge` fallback; the `emerge <atom>` production paths call the config version to override both fields with the config values. `INSTALL_MASK` is in Portage's `environ_whitelist` and is non-incremental, so the `env` configdict (highest-priority layer) overrides `make.conf`'s value -- an `emerge INSTALL_MASK="…" -k …` invocation. Unset and empty behave the same, matching `MergeOptions::from_env`.
- The buildpkg half ports `create_depgraph_params.py`'s `if "buildpkg" in settings.features: myparams.setdefault("--buildpkg", "y")` (backlog #37 S3).
- The run-wide half of the build-phase flag env: Portage `preinst_mask()` (`bin/misc-functions.sh`) driven by the resolved config rather than the process env: `make.conf`/profile `INSTALL_MASK` folded with the `no{man,info,doc}` `FEATURES` tokens.

## fn config_features_opt
- Uses `Config::resolved_incremental`, with Portage's own `-*` / `-tok` / `+tok` semantics. Callers that need Portage's host-`make.globals` default seed it themselves (see `pkgdir_index_trusted`).

## fn pkgdir_index_trusted
- Backlog #199: ports `bintree.populate`'s `reindex = "pkgdir-index-trusted" not in self.settings.features or force_reindex` (`bintree.py:936-938`): `true` trusts the index (Portage `_populate_local(reindex=False)`), `false` re-stats (`reindex=True`). A `FEATURES=-pkgdir-index-trusted` in `make.conf` or on the calling env reaches the scan; no new env read here.
- The fold starts from Portage's own default: the token is in Portage `make.globals`' own default `FEATURES` (`cnf/make.globals:81`), which Portage always sources from the host installation -- while portuale reads `make.globals` `config_root`-relative, so under a test/fixture root it contributes nothing. `None` therefore stays trusted (byte-identical to the pre-#199 scan, and what Portage computes on a live host); only a folded list lacking the token -- an explicit `-pkgdir-index-trusted` (or `-*`, Portage `resolved_incremental`'s own clear) not undone by a later re-add -- distrusts, exactly when Portage's folded `features` would lack it.

## fn config_features_string
- For `MergeOptions::features` (`merge_binpkg`'s `PORTAGE_UPDATE_ENV` vdb-environment regeneration). Shared with the remote-merge server (`run_remote_plan`, backlog #171).

## fn config_bzip2_command
- Portage's merge-time env save records its live value (`cnf/make.globals:105` defaults it to `bzip2`; a client `make.conf` may set e.g. `lbzip2`/`pbzip2`). Shared with the remote-merge server (`run_remote_plan`, backlog #171) so the scrubbed regen'd env records exactly what a local merge would.

## fn config_install_mask
- Shared with the remote-merge server (`run_remote_plan`, backlog #170).

## fn load_repos_and_config
- Backlog #170/#171: the `INSTALL_MASK` + `no{man,info,doc}` fold, the merge-time `FEATURES`, and `PORTAGE_BZIP2_COMMAND` all come from the placed config.
- `Err` is the bare display text; the caller prefixes it (`emerge:` / `mrg: …`) exactly as the inline preamble did.

## fn load_config_quiet
- For `mrg::make_conf_lookup` (backlog #318); uses `portage_repo::find_repos_quiet`.

## fn resolve_config_for_root
- Resolves the `Config` for `root` (`eroot`) reading `config_root`'s `/etc/portage` overrides. Track X Slice B/C; Portage's running tree inherits `PORTAGE_REPOSITORIES` the same way (`create_trees`' `clean_env`).
- Ascending-priority order, same as `find_repos`' order, which only matters if two overlays' entries could otherwise interfere -- and the `::name` scoping already rules that out regardless. The same list, plus the main repo's name, lets `resolve_config` follow cross-repo `parent` entries (`reponame:path` syntax).
- `repo_masters`: see `portage_repo::RepoConfig::masters`. `repo_aliases`: Portage `repositories.get_location_for_name`; aliases come from `repos.conf`/`layout.conf` (see `resolve_config`).

## fn load_running_config
- Backlog #242 Slice D: per-node USE paint (S0 map `LLM/evidence/2026-10-02-242-display-sites.md` §2). A hermetic test tree carries no `/etc/portage` -- Portage has no such tree at all.

## fn package_options_from_env (#349 S4: DISTDIR)
- `distdir` follows the resolved config when one is in scope (`distdir_from_env_or_config`, calling env highest); config-less keeps the env-or-`make.globals`-default. Portage: `config.py`; `cnf/make.globals:30`.
