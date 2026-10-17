# crates/portuale/src/ebuild.rs
The `ebuild <file> <command>` CLI: option/command recognition plus real execution.

## module
- CLI-surface recognition mirrors `emerge`'s own `pretend.rs`/`emerge_options.rs` treatment: Portage's options (see `ebuild_options.rs`, transcribed from `bin/ebuild`'s argparse setup) and Portage's commands (from `doebuild()`'s `validcommands` list) are recognized and accepted; genuinely invalid input (unrecognized option, filename not ending in `.ebuild`, unrecognized command, missing file/command) is rejected with a specific message.
- Real execution landed incrementally: task #54 for the `actionmap_deps`-chained phase commands (`pretend`/`setup`/`unpack`/`prepare`/`configure`/`compile`/`test`/`install`; see `ebuild_phases`'s module doc for the architecture and v1 scope cuts), task #55 for `merge` (runs the `install` chain, then `pkg_preinst`, copies `${D}` into `${ROOT}`, writes a vdb entry, then `pkg_postinst`), `unmerge` (runs `pkg_prerm`, deletes the vdb `CONTENTS` paths from `${ROOT}`, runs `pkg_postrm`, removes the vdb entry), and `package` (runs the `install` chain, then invokes `bin/misc-functions.sh`'s `__dyn_package` -- unmodified bash shelling out to the native `portuale` helpers `xpak recompose` / `gpkg compress` (#326 S4-S5, through `bin/portuale-python`) -- producing an XPAK-tagged `.tbz2` at `PKGDIR` plus a `Packages` index entry). Later slices added `qmerge` (`merge` minus the `install` re-run, gated on Portage's `${PORTAGE_BUILDDIR}/.installed` marker that `doebuild()` itself checks), standalone `config`/`info`, `prerm`/`postrm`, and (#348 S3 P2) standalone `preinst`/`postinst` as real single-phase runs via `run_single_phase` -- Portage's own direct `_spawn_phase` for them, no dep chain and no merge involved.
- Every other Portage command (`nofetch`/`depend`/`fetch`/`fetchall`/`digest`/`manifest`/`rpm`/`instprep`/`clean`/`cleanrm`) still falls through to the dry-run stub. `instprep` runs inside every `merge`/`qmerge` (`ebuild_phases::run_instprep`, Portage's `treewalk()` first step); only the standalone command is stubbed.
- Exit codes mirror Portage's `bin/ebuild`: 2 for missing args (argparse `parser.error()`), 1 for everything else invalid (`err()` helper and `doebuild()`'s return for an unrecognized command).
- `-h`/`--help` is checked unconditionally before anything else, matching `bin/ebuild` (argparse checks `-h`/`--help` during parsing itself). `--version` is deliberately not implemented (see `ebuild_options.rs`). The help text is a portuale-specific summary, not a port of the argparse usage block, the same precedent `emerge --help` set.
- Known deviation, in full: Portage's `bin/ebuild` uses argparse's `parse_known_args`, which silently swallows an unrecognized flag into the leftover-positional-args list rather than rejecting it outright -- in practice this usually surfaces later as a confusing "does not end with '.ebuild'" error instead of a clear "unrecognized option" one. Portuale deliberately reports an unrecognized option immediately and specifically instead.

## fn wants_help
- Unlike `emerge`'s own `wants_help`, no short-flag-bundle scanning is needed: Portage's `bin/ebuild` declares no short aliases for any of its six options, so `-h` is never part of a bundle.

## fn run
- `--shell bash|brush` (default `bash`): selects the shell backend for every phase/hook/misc-function (see `ebuild_phases::ShellBackend`, including why the default is `bash`, not the embedded `brush`). A portuale-only flag, not a Portage `bin/ebuild` option, so it is checked here directly rather than through `ebuild_options::lookup_option` (deliberately not added to `ebuild_options::OPTIONS`, which transcribes Portage's argparse setup) -- the same special-casing `wants_help` gets.
- `--debug` (task #56) sets Portage's `PORTAGE_DEBUG`, triggering `bin/ebuild.sh`'s own `set -x` guard during phase execution -- see `ebuild_phases::run_one_phase`'s own setup block. Every other `Kind::Boolean` option is still a pure no-op (the `Some(Kind::Boolean) => {}` arm of the option match), matching the module's long-documented v1 scope; `--debug` is the first one wired to something real.
- The `--shell` special-casing outside the options table is the same "special-cased outside the Portage-options table" treatment `wants_help` gets.
- #348 S3 P1: `test` anywhere in argv forces the test phase for the whole invocation (`EBUILD_FORCE_TEST=1` threaded through `build_env`, `features.add("test")` reproduced as a token fold in `phase_env_vars`, `Forcing test.` on stderr) -- printed before any refusal or gate, so even a refused `test` prints it, exactly like `bin/ebuild:296-302`. The `RESTRICT=test` skip stays bash-side in the vendored `__dyn_test`, untouched.

## fn run_real_commands (#349 S4 shape)
- Resolves one `Config` per invocation (`resolve_standalone_config` over
  the ebuild's own package dir) and derives everything below from it
  instead of re-resolving per lookup. `None` outside any repo checkout:
  every reader keeps its calling-env-only fallback. `PORTAGE_TMPDIR` and
  `DISTDIR` come from `portage_tmpdir_from_env_or_config` /
  `distdir_from_env_or_config` (calling env highest); the per-package
  `package.env` `PORTAGE_TMPDIR` re-derivation still layers on top
  (`resolve_standalone_portage_tmpdir`, `depend`/`clean` exempt).
- The merge gates mirror the `emerge` path's `merge_setup` without
  touching the shared `from_env`: `resolved_config`,
  `config_install_mask`, `apply_uninstall_ignore` (before the features
  fold, like `merge_setup`), `set_resolved_features`,
  `apply_config_protect`, `package_env_vars`, plus the resolved
  `distdir`. Per-package `package.env` `FEATURES` still folds on top
  (`resolve_standalone_resolved_features`), then `bin/ebuild`'s
  scheduler-only tokens are discarded (`discard_direct_ebuild_features`).
  The final list rides into every phase through `direct_build_env`
  (`FEATURES` + `PORTAGE_FEATURES`), so sandbox/isolation/helper gating
  (`features_string`) sees the config value, not the raw process one.
- (Superseded by the above) The default `PORTAGE_TMPDIR` used to be the
  env-or-`/var/tmp` shortcut (`portage_tmpdir_from_env`); the builddir
  adds `portage/<cat>/<pf>` itself.
- (Superseded) Per-package `FEATURES` used to fold onto the raw process
  `$FEATURES` for lack of a resolved run-wide config.
- (Superseded) The merge config used to come from `MergeOptions::from_env`
  alone (`CONFIG_PROTECT[_MASK]`, env `DISTDIR`, the raw-process
  `FEATURES` gates).
- Real execution only when every requested command is implemented for real (the actionmap-chained subset plus `merge`/`qmerge`/`unmerge`/`package`/`config`/`info`/`prerm`/`postrm`/`preinst`/`postinst`): a deliberate simple v1 boundary avoiding partial-real-execution ambiguity for mixed lists like `["install", "merge"]` (e.g. `ebuild foo.ebuild compile clean`); a purely dry-run request keeps the exact pre-existing stub message unchanged.
- Per-package `PORTAGE_TMPDIR` (backlog #99): re-derived from the matched `package.env` value when the calling environment is silent (`env`-over-`pkg`, #101). Skipped for `depend` (metadata extraction stays config-free) and `clean` (operates on the process builddir by definition) -- Portage's own `_check_temp_dir` exempts the same commands (`doebuild.py:756-764`). A missing matched directory fails with Portage's exact message.
- Per-package `FEATURES` (backlog #130): folds a matched `package.env` value onto the resolved run-wide list in Portage's `[run-wide, pkg, calling-env]` order and re-derives the merge gates, matching the resolved-graph paths' `set_resolved_features` call (#349 S4; before, it folded onto the raw process `$FEATURES` for lack of a resolved run-wide config).
- `merge`/`qmerge`/`unmerge` refuse unprivileged callers up front (Portage's `bin/ebuild` has no such guard and just fails mid-phase; portuale gives the same clear error `emerge` does via `privileges::is_privileged`).
- One command at a time, not the whole slice at once, mirroring Portage's `bin/ebuild` `for arg in pargs: doebuild(...)` loop which re-derives the environment fresh per top-level command.
- #348 S3 P3: the direct path runs Portage's `_validate_deps` REQUIRED_USE gate once per invocation (before any phase, after the unsupported-EAPI refusal): `required_use_refusal` over the md5-cache `REQUIRED_USE`, the standalone full effective USE and the implicit-folded IUSE, under `attrs.required_use` (S1). Merge/unmerge internals never re-gate (resolver-owned merge path, never-gated teardown). `clean` stays exempt (dry-run stub, unreachable in the real branch -- and Portage itself returns 0 there).

## fn unmerge_and_package_options
- Takes the caller's once-resolved `Config` (#349 S4; falls back to
  `resolve_unmerge_config`, then to calling-env-only defaults).
- Binpkg scalars from the resolved chain (backlogs #173, #180): calling env over `make.conf`/profile/`make.globals` (the standalone `resolve_config` chain below the env, Portage's `config` precedence), else Portage's `make.globals` defaults (`PackageOptions::default()`). The env half reads through `config_env_var`, the same test hook `env_over_config_scalar` uses. The gpkg compression pair (`BINPKG_COMPRESS`/`BINPKG_COMPRESS_FLAGS[_<NAME>]`) reads files-only below the env (the Q6 rule: Portage's `environ_filter` keeps a calling-environment value out of the helper's rebuilt settings); `PORTAGE_BZIP2_COMMAND` keeps the full chain for both formats (whitelisted, never filtered), as does `PKGDIR`. `BINPKG_FORMAT` resolves first because the compression lookup depends on it (kept as a dedicated wrapper so the #173 block stays verbatim).
- Per-package `BINPKG_COMPRESS*` (backlog #147 S3): standalone `ebuild <file> package` runs `doebuild` on the setcpv'd config (`bin/ebuild:365`), so the package's settings reach the xpak pipe (S0 C2: gzip magic with run-wide zstd). The match plays only when the calling env is silent on `BINPKG_COMPRESS` (Portage's calling-env-wins scalar precedence, #101) and only for `xpak` runs: Portage's gpkg helper reads the global settings (`bin/gpkg-helper.py:49`), never the package's (S0 B-gpkg). No per-entry `FEATURES` fold on purpose: no per-package `FEATURES` token has a Portage-honored standalone-`package` artefact effect (the layout is run-wide -- the STOPPED S1 arm owns that question -- and signing needs its own oracle), so per O9 it is not implemented.
- `DISTDIR` is the caller's resolved value (#349 S4; before, the
  env-var-not-full-config-resolution shortcut); it is shared by both
  real-execution paths that run the `install` chain and therefore a real
  `unpack` (see `ebuild_phases::fetch_sources`).
- `buildpkg_live` / `binpkg_multi_instance` / `binpkg_signing` follow
  the resolved `FEATURES` when a config resolves (#349 S4, Q4/Q5
  defaults; `binpkg-multi-instance` gives Portage's
  `<cat>/<pn>/<pf>-<bid>` layout wherever `make.globals` is read, and
  the flat layout stays only where no config resolves); else the
  calling env with the `make.globals` defaults. (Before: raw `$FEATURES`
  reads, with `binpkg-multi-instance` deliberately conservative.)
- Portage's `"unmerge-orphans" in self.settings.features` reads the
  resolved `FEATURES` (#356 S4; before, the same env-var shortcut
  `collision_protect`/`protect_owned` used).

## tests
- "clean" is a Portage command (from `doebuild()`'s `validcommands`) that portuale still only dry-runs -- exactly the case the stub still covers. `--shell` tests only exercise CLI parsing for the same reason.
- Help is checked before anything else is parsed, so it wins even alongside a valid invocation or an unrecognized option.

## fn unsupported_eapi_refusal
- #348 S5: an ebuild whose head EAPI Portage does not know refuses with the exact `UnsupportedAPIException` text before anything runs — phase chain, standalone phases, `merge`/`qmerge`/`package`, and `clean` (which Portage refuses too, S0 cell `d-eapi99-clean`). `unmerge` and the other dry-run stubs stay ungated (unmerging a corrupt-EAPI install must keep working; no oracle for the rest). The gate fires before the privilege check. Portage: `doebuild.py:565-572`, `exception.py:228-230`.

## fn unsupported_eapi_message
- #348 S5: `Unable to do any operations on '<cpv>', since ... supports EAPI '<eapi>'.` with leading dashes stripped from the EAPI (`exception.py`) and `bin/ebuild`'s `!!! ` + word-wrap at 70 columns (byte-verified against 3.0.82.2 for `99` and `7_alpha`).

## S6 (351-MARK/DIE): direct phases tee through ${T}/build.log
- `run_real_commands` passes `PhaseLog::Foreground` for standalone phases and chained commands (`run_single_phase`/`run_commands` resolve it to the tee). `PORTAGE_LOG_FILE` rides the phase env from there, so `die` prints its build-log line with the S4 `::repo` suffix on stdout. `clean` and the merge/unmerge/package subcommands keep their own routing.
