# crates/portuale/src/ebuild_phases/tests/commands.rs
Phase-command end-to-end tests. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn install_lands_a_real_file_under_a_real_d
- End-to-end proof: an `install` run against the fixture ebuild (`fixtures/repo/dev-libs/phasepkg`, whose `src_install` calls Portage's `insinto`/`doins`) lands a file under `${D}`, via the unmodified `bin/*.sh` this module drives -- not a mock of any kind. A fresh, uniquely named `PORTAGE_TMPDIR` per test run (passed directly to `run_commands`, never via the environment -- see its doc for why) keeps this safe to run alongside other tests in the same process.

## fn a_brush_phase_does_not_leak_its_umask_into_the_host_process
- #308: an in-process `Brush` phase must not leave its umask behind. `bin/ebuild.sh` runs `umask 022` and brush's builtin is the process-wide `umask(2)`; Portage runs each phase in its own process. The test moves the umask, which would itself race every parallel test's file modes, so the body runs in a re-executed child of this test binary (`PORTUALE_UMASK_CHILD`) and the parent only checks it. The `SAFETY` comment marks `umask(2)` as infallible in the single-test child.

## fn install_dies_like_portage_when_s_is_missing
- Backlog #190 (S0 repro): Portage's `bin/phase-functions.sh` `__dyn_install` -- like its `prepare`/`configure`/`compile`/`test` twins (`phase-functions.sh:444,480,512,546,638`) -- dies with `The source directory '${S}' doesn't exist` when `S` is not a directory. The default is `S=${WORKDIR}/${P}` (`bin/ebuild.sh:500`); EAPI 8 has no `${WORKDIR}` fallback (`bin/eapi.sh:8`: `___eapi_has_S_WORKDIR_fallback` is EAPI 0-3 only), and with `A` empty but `src_install` defined, `__has_phase_defined_up_to install` is true, so neither fallback fires and the phase dies. A source-less ebuild with no `S` must therefore fail the `install` run with Portage's message.

## fn install_writes_the_real_build_time_file
- Backlog #47: Portage's `_post_src_install_write_metadata` (lib/portage/package/ebuild/doebuild.py:2727-2732) writes `build-info/BUILD_TIME` (epoch seconds) before any metadata key, and the vdb copy + `_consolidate_to_metadata_file` carry it. The install chain must leave that file behind for the merge to copy.

## fn install_qa_check_docompress_compresses_docs_and_repairs_symlinks
- End-to-end proof of the `binpkg-docompress` transform path (#38 S1): with `PORTAGE_COMPRESS=bzip2` and `FEATURES=binpkg-docompress` in the phase env, real, unmodified `install_qa_check` (`misc-functions.sh:147-152`) runs `bin/ecompress` over `${D}` -- the >128 B doc (the `PORTAGE_DOCOMPRESS_SIZE_LIMIT`) becomes `BIG.txt.bz2`, the smaller one is untouched, and the dangling link into the now-compressed doc is repaired (and suffixed) by `ecompress`'s own `fix_symlinks`. Uses the default `bash` backend (`ShellBackend::Bash`), the same one a live `emerge` runs. `binpkg-dostrip` stays on: this test is about compression.

## fn run_instprep_applies_only_the_complement_of_the_install_qa_gate
- #38 S4: the merge-time `instprep` phase (`misc-functions.sh:265-308`) is the complement of the `install_qa_check` gate. With `binpkg-docompress` absent the install chain leaves `BIG.txt` plain; `run_instprep` with the token present is a no-op (Portage's `! contains_word` gate), and without it compresses the doc and repairs the link, then marks `.instprepped` so a re-run skips.

## fn install_runs_the_portage_post_install_qa_check_and_strips_a_genuinely_empty_dir
- End-to-end proof that `_post_phase_cmds["install"]` (`EbuildPhase.py:424`/`442-461`) actually runs now: unmodified `bin/misc-functions.sh install_qa_check`'s own `95empty-dirs` QA check (`bin/install-qa-check.d/95empty-dirs`) strips a genuinely empty directory from the install image for any EAPI 8+ ebuild (Portage's `___eapi_has_strict_keepdir`, unconditional, not gated on any `FEATURES` flag) -- a bare `dodir` with nothing ever installed into it must be gone from `${D}` by the time `install` finishes, while a `keepdir`'d one (the ebuild-author idiom this QA check's message recommends) survives untouched.

## fn install_lands_a_real_file_under_a_real_d_via_real_bash
- `ShellBackend::Bash` counterpart of the brush proof above -- same fixture, same assertions, proving the `bash <bin_dir>/ebuild.sh <phase>` subprocess backend (`run_one_phase_bash`) produces an identical result to the embedded-brush backend, not just that it runs without erroring.

## fn install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset
- End-to-end proof of `fetch_sources`'s own `A`/`AA` computation through the CLI path, deterministic and offline: `dev-libs/verifiedfetchpkg`'s own checked-in `Manifest` entry matches a payload file this test pre-seeds into `DISTDIR` (the valid BLAKE2b-512/SHA-512 digests of the literal bytes `"hello from verifiedfetchpkg\n"`, confirmed via the `b2sum`/`sha512sum` system tools) -- so the "already verified" skip-fetch path fires and no network access is attempted at all, while still exercising the full SRC_URI grammar: an arrow-rename (`-> verifiedfetchpkg-1.0.tar.gz`) and a `test?` USE-conditional group that must stay excluded from `A` (portuale's always-empty USE set) but still appear in `AA` (PMS's own "every file regardless of USE" definition).
- `AA` is unset: Portage's `config.environ()` pops it for every EAPI >= 4 (lib/portage/package/ebuild/config.py:3331-3333); the EAPI floor here is 5+. The ebuild's own `echo "AA=${AA}"` therefore writes an empty value, matching an EAPI-8 phase.

## fn features_xattr_routes_phase_installs_through_the_xattr_wrapper
- #329 end to end in a phase: under `FEATURES=xattr` a phase's `doexe` and plain `install` reach `ebuild-helpers/xattr/install` first, and with `PORTAGE_INSTALL_XATTR_IMPLEMENTATION=python` (set by the ebuild; Portage does not whitelist it) the wrapper runs `install.py` through `portuale-python`, portuale's native one. The image files keep the source's `user.*` xattr; without `xattr` they lose it. Portage does the same in the bed image (pmtest `differential-test-bed/scripts/329-xattr.sh`, both with and without `-xattr`).

## fn install_really_inherits_a_real_eclass_and_calls_its_own_function
- End-to-end proof of `eclass_locations_value`: `dev-libs/eclasspkg` inherits a (fixture-only) eclass, `pilotcheck.eclass`, via unmodified `bin/ebuild.sh`'s own `inherit()` function -- previously portuale never populated `PORTAGE_ECLASS_LOCATIONS` at all, so this would have `die`d immediately with `"pilotcheck.eclass could not be found by inherit()"`. `src_install` calls a function the eclass defines (`pilotcheck_hello`), proving the eclass's own content -- not just its own existence -- is usable afterward.

## fn a_corrupt_saved_environment_fails_the_next_phase_in_both_backends
- B2: a saved `${T}/environment` that does not parse must fail the next phase loudly -- Portage's `bin/ebuild.sh:580`'s own `source "${T}"/environment || die "error sourcing environment"` -- never silently continue and run the `default` phase function against a half-loaded environment (the #38 G3 smoke's empty image with rc 0). `run_single_phase` runs exactly one phase, with no `actionmap_deps` chain that would re-save a fresh environment first: the same shape a builddir resume has when portage starts the next phase. The post-phase save writes `${T}/environment`; the test breaks it with a genuine syntax error (an unterminated quote -- an unterminated here-document is only a warning to bash, not a parse error, so it would not fail the bash control).

## fn brush_phase_env_is_filtered_of_bash_special_variables
- B3: Portage's `bin/phase-functions.sh` `__filter_readonly_variables` lists bash's special variables by running `env -i -- "${BASH}" -c …`, so the embedded brush shell must carry a real `$BASH` -- and brush's own brace expansion must produce that list's fields even under the function's own `local IFS` (the two brush-side fixes are both `brush-pin.md`'s tracked work). When either is missing, the saved `${T}/environment` carries `BASHOPTS`/`EUID`/`PPID`/`SHELLOPTS`/`UID`, and re-sourcing it prints `cannot mutate readonly variable` (plus `env: ''` when `$BASH` itself is empty).

## fn expanded_name_export_assigns_in_both_backends
- B4: declaration builtins must assignment-expand an expanded name (brush fix 06, backlog #94). `export ${var}=value` silently no-op'd under brush, so `_tc-getPROG`-shaped code saw empty variables; the fixture's `src_install` dies unless the expanded export arrived. Both backends must install it and agree on the installed file set, not merely on the exit code.

## fn install_inherits_a_real_eclass_that_only_exists_in_the_overlays_own_master_repo
- End-to-end proof that the masters-chain fix actually unblocks an `inherit()` call `PORTAGE_ECLASS_LOCATIONS` resolution alone couldn't reach before: an overlay ebuild inheriting an eclass that only exists in its own master repo, never redeclared locally -- exactly the gap this module's doc comment (before this slice) named as out of scope.

## fn debug_flag_exports_portage_debug
- Task #56, proven for real not simulated: passing `debug: true` exports `PORTAGE_DEBUG=1` into the phase's own environment (see `run_one_phase`'s setup block) -- proven here by having the fixture's own `src_install` record the value it actually observed, rather than asserting on captured `set -x` trace output (which would need redirecting the whole test process's stdout/stderr, a much heavier and flakier mechanism for the same underlying claim). Portage's `bin/ebuild.sh:479`'s own `[[ ${PORTAGE_DEBUG} == 1 ]]` guard is what turns this exported value into the `set -x` xtrace a human running `ebuild <file> install --debug` directly would see; that guard itself is unmodified bash portuale doesn't reimplement, so proving the export is correct is sufficient here.

## fn phases_export_portage_actual_distdir_so_doins_keeps_absolute_symlinks
- Portage's `config.environ()` (lib/portage/package/ebuild/config.py:3403-3409) exports `PORTAGE_ACTUAL_DISTDIR` as `DISTDIR`, and `bin/ebuild-helpers/doins:80-82` passes it as `doins.py --distdir`. Without it `--distdir` defaults to `""`, which `_parse_args` turns into `b"/"`, so `doins -r` dereferences every absolute symlink. A dangling one (porttest/helper-doins's `abs-link`, found by the #326 Z nopy leg) then fails with `FileNotFoundError`, while Portage installs the link as a link.

## fn phases_see_the_per_package_fake_distdir
- Portage gives every phase `DISTDIR=${PORTAGE_BUILDDIR}/distdir` -- one symlink per file of the package's own `A` (`_prepare_fake_distdir`, lib/portage/prepare_build_dirs.py:518-549) -- and the directory in `PORTAGE_ACTUAL_DISTDIR` (config.py:3403-3409). A distfile that is in the distdir but not in this package's `A` is not visible through `DISTDIR`. Expected values: Portage's source above, and `ebuild --skip-manifest <fixture> install` (Portage 3.0.82.2 on the dev host, same two distfiles) writing the same four lines.

## fn fake_distdir_is_prepared_only_when_portage_prepares_it
- Portage's doebuild.py:1463-1469: the fake distdir is prepared for a command other than `setup` unless `FEATURES=noauto`; a `setup`-only chain, or any chain under `noauto` that has no `install`/`unpack`, creates none.

## fn pretend_alone_succeeds_with_no_explicit_phase_functions
- `pretend` alone (the shortest prerequisite chain -- see `phase_prerequisites`) still exercises the full environment-setup + `bin/ebuild.sh`-sourcing + `__ebuild_main` path without needing `src_install` at all, proving the slice works even for an ebuild with no explicitly defined phases (every phase function `phasepkg` doesn't define comes from EAPI defaults, ported here for free -- see this module's doc).

## fn open_log_file_gzip_pump_round_trips
- `FEATURES=compress-build-logs` pump round-trip (Portage's `EbuildPhase._open_log`'s `gzip.GzipFile(mode="ab")`): bytes written to the sink's two handles land gzip-encoded in the `.gz` file -- one member per `open_log_file` call, so a second call appends a second member -- and decode back losslessly. A plain path still writes through unencoded. Closing the write ends (then joining) finishes the member -- the same drop order the phase call sites use. Both members, both streams -- stdout/stderr chunk order across the two pipes was never deterministic (Portage appends both fds to one gzip stream too), so the comparison is unordered.

## fn install_runs_the_portage_post_install_qa_check_and_strips_a_genuinely_empty_dir (#516)
- The merge path runs the post-`install` QA sequence; the direct `ebuild` path never does.

## fn install_qa_spawn_pops_ebuild_phase_so_use_test_does_not_die (#516)
- The post-`install` QA spawn carries no `EBUILD_PHASE` (Portage's `MiscFunctionsProcess._spawn` pops it), so a repo-local check calling `use test` on an ebuild without that flag passes instead of dying.
