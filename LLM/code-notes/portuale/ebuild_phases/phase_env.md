# crates/portuale/src/ebuild_phases/phase_env.rs
The environment variables and PATH a phase runs with.

## fn phase_env_vars
- Call-site extras (`ebuild_package`'s `PKGDIR`/`PORTAGE_BINPKG_TMPFILE`) arrive via `extra_env`.
- Downstream `cmd.envs` is last-wins, so seeding config base first, computed literals next, `extra_env` last gives the right precedence.
- The standalone base is computed once for both the `USE` entry and the seeding pairs.

## fn eclass_locations_value
- `bin/ebuild.sh` evals the value once, unconditionally, right after being sourced; `inherit()` walks the array for `<location>/eclass/<name>.eclass` per top-level `inherit` line. Portuale once never populated the var, so `inherit()` died for every eclass (seen live with `sys-fs/fuse`, `app-editors/nano`, `app-arch/xz-utils`, each on a different eclass).
- Masters chain: `eclass_locations = [master.location for master in repo.masters] + ([repo.location] if new)`, exported `reversed()` — own repo first, masters in declared order. `RepoConfig::masters` (same chain `blocked_installed_packages` and `pretend.rs` use for profile/USE stacking) defaults to the main repo alone, empty for the main repo itself.
- Any `repos.conf`/`config_root` failure degrades to the containing repo alone (same graceful-degrade precedent as `blocked_installed_packages` for `find_repos(config_root).ok()?`): never a false negative that breaks a lookup that used to work. A master-less, repo-less file honestly finds nothing, as Portage would.

## fn portuale_bin_value
- Exported for the vendored `bin/portageq-wrapper` shim and the `__helper` shims; the shim execs `$PORTUALE_BIN portageq ...`. Under test the libtest harness binary is avoided because its stdout banner would become the shim's answer. Non-test resolves once at startup (never `<path> (deleted)`).

## fn portage_python_value
- Native mode uses the `portuale-python` shim next to the phase runtime; `real` mode uses `/usr/bin/python`.

## fn phase_env_vars (FEATURES)
- The merge path's base has no `FEATURES`, so the process value stands (still overridden downstream by `extra_env`); the standalone `phase_environ` base already folds run-wide + package.env + calling env, which a raw literal must not clobber. No-config standalone runs keep the process value.

## fn phase_env_vars (RESTRICT/PROPERTIES)
- From the package's already-USE-reduced `RESTRICT`/`PROPERTIES` (`str(self._pkg.restrict)`/`str(self._pkg.properties)`). Bash consults these directly: the `RESTRICT=test` skip, `RESTRICT=nostrip`/`RESTRICT=strip`. The raw `RESTRICT`/`PROPERTIES` are never exported (only the ebuild's own bash sees the raw unreduced values via its sourced metadata). `depend` reduces on the config `USE` set (Portage's `PORTAGE_USE`); see `restrict_and_properties`.

## fn phase_env_vars (PORTAGE_PYM_PATH)
- The `lib/` dir of the Portage checkout, where the `portage` python package lives. Upstream `bin/` helpers that import portage (`ebuild-pyhelper` and its `chmod-lite`/`ebuild-ipc` symlinks) start with `cd "${PORTAGE_PYM_PATH}" || exit 1`; unset, every one aborts and breaks eclass `has_version`/`best_version` (e.g. `autotools.eclass`'s automake probe). The vendored `portageq-wrapper` shim is native and never touches the variable; the `cd` is what upstream scripts do in `real` oracle mode. Setting it does not regress the "safe cwd for bug #469338" branch (`${PORTAGE_BUILDDIR}/empty` still preferred).

## fn phase_env_vars (VDB forwarding)
- The database selection is `mrg`-exported for children (`PORTUALE_VDB_*`). `PORTUALE_VDB_IPC` is the parent pipe of a redb run. `PORTUALE_PYTHON_HELPERS` rides along so the `chmod-lite` shim sees `real` mode inside the phase too.
- The phase's `portageq` never resolves make.conf for the backend itself.

## fn phase_env_vars (DISTDIR remap)
- Applied after the pairs above so the remapped values are last set. Portage's `KeyError` guard (no `DISTDIR`) leaves both untouched. Filled by `prepare_fake_distdir`.

## fn phase_env_vars (locale)
- For EAPI 5+'s `posixish_locale` Portage even asserts `LC_ALL` absent. The `--shell brush` path still inherits the host's `LC_ALL` in the embedded shell; the category exports override it for every subprocess the phase spawns.

## fn phase_path
- `FEATURES=xattr` is on by default (`make.globals` appends it). The `xattr` dir holds the `install` wrapper, so every phase `install` (`doexe`, `dobin`, ebuild code) goes through `install-xattr` — or `install.py` (native via `portuale-python`) — and keeps xattrs. No loop: `install-xattr` runs `/usr/bin/install` (or `$REAL_INSTALL`); `install.py`'s `Which("install", exclude=__PORTAGE_HELPER_PATH)` skips the wrapper.
- The base is the last `extra_env` `PATH` pair — `portage_profile::phase_environ` exports it only when `env.d` sets `PATH` (Portage's "ignore PATH from the calling environment" rule).

## fn phase_setup_script
- Single-quote escaping (not the old `{:?}` double-quoted export, which let the brush shell word-split/expand/command-substitute): values can now be arbitrary config/ebuild text (`CFLAGS` with `$(...)`, `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES` globs, descriptions with quotes). `Bash` needs no text: see `run_one_phase_bash`/`run_misc_functions_bash`.
- The `unset` loop mirrors `run_one_phase_bash`'s `env_clear`.
