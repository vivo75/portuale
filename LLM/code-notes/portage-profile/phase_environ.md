# crates/portage-profile/src/phase_environ.rs
Portage `config.environ()` for build-phase environments: the export set a source build's phases see.

## module
- Backlog #37, slice S1 (`docs/history/037_Build-phase-env-completeness.plan.md`, gates G1/G2/G3 as decided 2026-09-13); the S0 recon that fixed the spec is `TEST/findings/l2.md` "S0 recon (#37, 2026-09-13)".
- `config.environ()` (`config.py:3263-3350`): iterates every string value, skips `environ_filter`; `PORTAGE_USE` itself is in `environ_filter`, so only `USE` reaches the phase; pops `AA` for every EAPI >= 4 (`:3331-3333`; portuale's EAPI floor is 5+, so always).
- The `environ_whitelist` restriction (`:3275-3305`) only applies from the second phase on, once `$T/environment` exists; portuale runs every phase in a fresh shell with the same env (`ebuild_phases::run_commands_async`), so the phase-1 full set is the spec and the phase-2+ re-export narrowing is deliberately not replicated. The "ebuild `unset` persists" nuance of bug 189417 is filed as a pre-existing gap, not this module's.
- Every `USE_EXPAND` variable gets one entry even when empty (S0: 49 such placeholders in the Portage oracle env): `ABI_X86="64"` when `abi_x86_64` is in USE, `L10N=""` otherwise (the `_lazy_use_expand` singletons at `config.py:2247`).
- The phase runner prepends its helper dirs to whatever `PATH` pair it is handed.
- G2: Portage's `PORTAGE_COMPRESS=""` is the documented "disable compression" (`bin/ecompress:254`).

## const ENVIRON_FILTER
- Transcribed verbatim from the vendored 3.0.82.2 checkout; refresh when the vendored portage moves.

## const PORTUALE_COMPUTED
- Covers `ebuild_phases::phase_env_vars` / `run_commands_async` (the `doebuild_environment()` dynamic set, `doebuild.py:381-545`) and the per-entry threading of `emerge_build::entry_build_env`.

## fn portage_use
- G3 (2026-09-13): the same value feeds both consumers (exported `USE` and the binpkg `metadata/USE` / `Packages` index).
- S0 oracle: `abi_x86_64 amd64 elibc_glibc kernel_linux` for an empty-IUSE package (the implicit profile flags, nothing the package doesn't declare or inherit implicitly).

## fn phase_environ
- With a package, `VIDEO_CARDS=""` even though the profile sets `nvidia` (the package doesn't declare `video_cards_nvidia`); with no package a profile scalar (`VIDEO_CARDS="nvidia"`, `ARCH`) must survive, hence `or_insert`, not `insert`.

## tests: with_test_env / synthetic_root
- Same thread-local override `lib.rs`'s `with_test_env` uses; `config_env_all` returns exactly the map.
- The #37 plan's trap §7.5: a synthetic root has no `make.globals`, so everything from there is seeded in `make.conf` explicitly. Mirrors the Portage `arch/amd64/make.defaults` + `base/make.defaults` lines the S0 oracle env came from.

## tests: phase_environ_exports_the_profile_family_and_folded_incrementals
- `ARCH`/`ELIBC`/`KERNEL`/`ABI`/`DEFAULT_ABI`/`MULTILIB_ABIS`/`LIBDIR_*`/`COMMON_FLAGS`/`CFLAGS`, `USE_EXPAND*`/`IUSE_IMPLICIT`/`USE_EXPAND_VALUES_ARCH`, `ENV_UNSET`: the S0 oracle rows.
- `IUSE_EFFECTIVE` for an empty IUSE is the implicit set; `USE_EXPAND` placeholders (`L10N=""`, `ABI_X86="64"`) derive from the package's USE.

## tests: phase_environ_filters_blacklists_and_never_clobbers_computed_keys
- The env token `evil` stacks on the profile FEATURES list (incremental fold, exactly like Portage); env `USE` is absent for `pkg == None`.

## tests: the_three_key_sets_are_transcribed_consistently
- `PORTAGE_USE` and `O` are the S0 corrections: filtered by Portage.
