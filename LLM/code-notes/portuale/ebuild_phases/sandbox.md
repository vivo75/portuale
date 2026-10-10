# crates/portuale/src/ebuild_phases/sandbox.rs
The phase shell backend, the calling-environment filter, and sandbox/isolation.

## enum ShellBackend
- `Bash` matches `_doebuild_spawn` almost exactly: `cmd = "{ebuild.sh} {phase}"` via `portage.process.spawn()`; `EBUILD_SH_ARGS="$*"` picks `<phase>` from argv and the tail calls `__ebuild_main ${EBUILD_SH_ARGS}` and exits.
- `Brush` was the original default (zero-dependency / minimal-Linux fit). The `declare -f` bug: the redirect is torn off the `cat` line and re-emitted after the body with its `"${var}"` target mangled; a `<<-` body is re-indented with spaces so its terminator no longer matches. Tripped by `toolchain-funcs.eclass`'s `_tc-has-openmp` (among others): the written `${T}/environment` then fails to parse and the next phase's `source "${T}/environment" || die` aborts the build. Tracked in `docs/brush-pin.md`. The options table transcribes `bin/ebuild`'s argparse only, hence `--shell` lives outside it.

## const SANDBOXED_SRC_PHASES
- Portage's `_unsandboxed_phases` are `clean`/`config`/`setup`/`pre|post*`/…; `_ipc_phases` another list. Both collapse to this set for the phases run here.
- `nofetch` excluded: Portage's `spawn_nofetch` uses a private tmpdir and neither unshares nor wraps.

## const ENVIRON_WHITELIST
- Copied verbatim from a live `special_env_vars.environ_whitelist` (plus the `^(CCACHE_|DISTCC_).*` regex); entries never set simply never pass through.
- Without the filter, `EMERGE_DEFAULT_OPTS` and portuale-only vars (e.g. `PORTAGE_RUNNING_ROOT`) leaked into the vdb once the `environment` regen started reading the live phase env.

## fn filter_calling_env
- Bug #189417's wording: "variables unset by the ebuild remain unset". Portage exempts init/fetch phases and standalone-`ebuild` `setup` (`EMERGE_FROM == "ebuild"`).
- Without it: `dev-build/ninja`'s `src_compile` `unset CFLAGS` never reached `src_install`, so `__dyn_install`'s write-if-present loop wrote a `build-info/CFLAGS` Portage never writes and the vdb carried a `CFLAGS` row Portage omits.

## fn phase_features_value
- Passed through rather than blanked for: `bin/estrip`'s `compressdebug`/`installsources`/`nostrip`/`splitdebug`/`xattr` (via `prepstrip`/`prepallstrip` in `install`); `phase-functions.sh`'s `__dyn_test` (`FEATURES=test` is what makes `src_test` run at all — previously a silent unconditional no-op), `nostrip`/`ccache`/`distcc`/`noauto`; `misc-functions.sh`'s `noclean`/`keepwork`/`sfperms`/`suidctl`/`selinux`/`packdebug`/`chflags`/`binpkg-do{compress,strip}`.
- Tokens Rust acts on (`sandbox`/`usersandbox`/`{network,ipc,mount,pid}-sandbox`, `distlocks`, `buildpkg-live`, `binpkg-multi-instance`) are never re-gated by bash here — confirmed by grepping every `contains_word … "${FEATURES}"` site in `bin/*.sh`. An unknown token, or one whose binary is missing (`ccache`/`distcc`/`selinux`), is exactly as inert as under Portage.

## fn network_sandbox_exempt
- Kept pure (not reading the process env) so tests need not mutate it in parallel; see `run_commands`'s doc for why env reads live at the CLI boundary. A live/VCS checkout needs the network by definition; a `test_network` test suite declares its own need; `RESTRICT` is an explicit per-package opt-out.

## fn sandbox_binary
- `X_OK` matches Portage's `os.access(_, os.X_OK)`. `None` is Portage's `if not free and not (fakeroot or sandbox_capable): free = True`.

## fn phase_isolation
- `_ipc_phases` (`setup`/`pretend`/`config`/`info`/`pre|postinst`/`pre|postrm`) is Portage's third exemption clause, but all are outside `SANDBOXED_SRC_PHASES`, so no check needed.

## fn sandbox_wrapped_command
- `FEATURES=sandbox` prepends the binary (Portage's `spawn_sandbox`: `args = [SANDBOX_BINARY, mycommand]`); `sandbox` exits non-zero once its log gains a violation.
- Loopback setup is Portage's `_configure_loopback_interface`: `ip link set lo up` plus the `AI_ADDRCONFIG`-workaround addresses (bug #690758 — some glibc `getaddrinfo()` calls with `AI_ADDRCONFIG` return nothing when a family has zero non-loopback addresses, so Portage adds one anyway). The IPv6 add may fail silently (`2>/dev/null`, like every shim command), exactly as Portage's `has_ipv6()` guard skips it on IPv6-less hosts — identical net effect.
