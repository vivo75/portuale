# crates/portage-profile/src/phase_environ.rs
Portage `config.environ()` for build-phase environments: the export set a source build's phases see.

## module
- Backlog #37, slice S1 (`docs/history/037_Build-phase-env-completeness.plan.md`, gates G1/G2/G3 as decided 2026-09-13); the S0 recon that fixed the spec is `TEST/findings/l2.md` "S0 recon (#37, 2026-09-13)".
- `config.environ()` (`config.py:3263-3350`): iterates every string value, skips `environ_filter`; `PORTAGE_USE` itself is in `environ_filter`, so only `USE` reaches the phase; pops `AA` for every EAPI >= 4 (`:3331-3333`; portuale's EAPI floor is 5+, so always).
- The `environ_whitelist` restriction (`:3275-3305`) only applies from the second phase on, once `$T/environment` exists; portuale runs every phase in a fresh shell with the same env (`ebuild_phases::run_commands_async`), so the phase-1 full set is the spec and the phase-2+ re-export narrowing is deliberately not replicated. The "ebuild `unset` persists" nuance of bug 189417 is filed as a pre-existing gap, not this module's.
- Every `USE_EXPAND` variable gets one entry even when empty (S0: 49 such placeholders in the Portage oracle env): `ABI_X86="64"` when `abi_x86_64` is in USE, `L10N=""` otherwise (the `_lazy_use_expand` singletons at `config.py:2247`).
- The phase runner prepends its helper dirs to whatever `PATH` pair it is handed.
- G2: Portage's `PORTAGE_COMPRESS=""` is the documented "disable compression" (`bin/ecompress:254`).
- Portage `config.environ()` for portuale's build-phase environment -- the **export set** a source build's every phase (and the `install_qa_check` misc-functions call after it) must see, built from the resolved [`Config`] instead of a curated allowlist. Backlog #37, slice S1 (`docs/history/037_Build-phase-env-completeness.plan.md`, gates G1/G2/G3 as decided 2026-09-13); the S0 recon that fixed the spec is `TEST/findings/l2.md` "S0 recon (#37, 2026-09-13)".
- The config's `env` layer is **the whole calling environment**:   `self.backupenv = os.environ.copy()`; `configdict["env"] =   LazyItemsDict(self.backupenv)` (`config.py:551,567`), highest   `USE_ORDER` priority -- so for the export set, a process-env key   overrides the same key from `make.conf`/profile.
- `config.environ()` (`config.py:3263-3350`) iterates every string   value, skips `environ_filter` (`special_env_vars.py:257-348`,   [`ENVIRON_FILTER`]), then forces `PORTAGE_FEATURES = FEATURES`   (`:3326`) and `USE = PORTAGE_USE` (`:3329`; `PORTAGE_USE` itself is   in `environ_filter`, so only `USE` reaches the phase), and pops `AA`   for every EAPI >= 4 (`:3331-3333`; portuale's EAPI floor is 5+, so   always). The `environ_whitelist` restriction (`:3275-3305`) only   applies from the *second* phase on, once `$T/environment` exists;   portuale runs every phase in a fresh shell with the same env   (`ebuild_phases::run_commands_async`), so the phase-1 full set is   the spec and the phase-2+ re-export narrowing is deliberately not   replicated (the "ebuild `unset` persists" nuance of bug 189417 is   filed as a pre-existing gap, not this module's).
- Incrementals are exported folded and sorted (`config.regenerate()`:   `" ".join(sorted(myflags))`) -- `FEATURES`, `ENV_UNSET`, the   `USE_EXPAND*`/`IUSE_IMPLICIT` name lists.
- `IUSE_EFFECTIVE` is the package's declared `IUSE` (bare names)   plus the profile-level implicit set (`config.py::   _calc_iuse_effective`, portuale's [`Config::iuse_effective`]).
- Expected values were verified live against Portage in the S0 recon (S0 oracle env / oracle rows; the `ebuild_phases` tests cite them the same way).

## const ENVIRON_FILTER
- Transcribed verbatim from the vendored 3.0.82.2 checkout; refresh when the vendored portage moves.
- Portage `special_env_vars.py:257-348` (vendored 3.0.82.2).

## const PORTUALE_COMPUTED
- Covers `ebuild_phases::phase_env_vars` / `run_commands_async` (the `doebuild_environment()` dynamic set, `doebuild.py:381-545`) and the per-entry threading of `emerge_build::entry_build_env`.

## fn portage_use
- G3 (2026-09-13): the same value feeds both consumers (exported `USE` and the binpkg `metadata/USE` / `Packages` index).
- S0 oracle: `abi_x86_64 amd64 elibc_glibc kernel_linux` for an empty-IUSE package (the implicit profile flags, nothing the package doesn't declare or inherit implicitly).
- Portage `PORTAGE_USE` (`config.py:2261`, "filtered by IUSE and implicit IUSE"): `enabled ∩ (IUSE ∪ IUSE_EFFECTIVE)`, sorted and deduplicated. This is the value Portage exports as `USE` (`config.py:3329`) **and** writes to a binpkg's `metadata/USE` / the `Packages` index (S0 oracle: `abi_x86_64 amd64 elibc_glibc kernel_linux` for an empty-IUSE package -- the implicit profile flags, nothing the package doesn't declare or inherit implicitly). G3 (2026-09-13): the same value feeds both consumers.
- Portage `config.py:2261,3329`.

## fn phase_environ
- With a package, `VIDEO_CARDS=""` even though the profile sets `nvidia` (the package doesn't declare `video_cards_nvidia`); with no package a profile scalar (`VIDEO_CARDS="nvidia"`, `ARCH`) must survive, hence `or_insert`, not `insert`.
- Returns `(name, value)` pairs in deterministic (sorted-by-name) order. Set-but-empty values are exported as empty (G2: Portage's `PORTAGE_COMPRESS=""` is the documented "disable compression", `bin/ecompress:254`); keys that are unset everywhere are simply absent. Precedence within the set: profile/`make.globals`/`make.conf` scalars first (`Config::other_vars`), then the calling environment on top (Portage's `env` layer is highest), then the folded incrementals and the per-package values, which always win.
- `pkg` is `None` for a run without a resolved candidate (a standalone `ebuild <file> <phase>`): no `USE`/`IUSE_EFFECTIVE` entry is produced (the caller's own base `USE` stands) and every `USE_EXPAND` variable is exported as its profile scalar if set, else `""`.
- 2. The calling environment, wholesale (`config.py:551,567`) --    highest layer, so it overrides step 1 for the same key.
- 2b. `doebuild_environment()` dynamic defaults that depend only on     the config. `PATH`: when `env.d` sets it, Portage drops the     calling env's `PATH` so the config-file stack's value stands     (`_doebuild_path`, `doebuild.py:365-378`); the phase runner     prepends its helper dirs to whatever `PATH` pair it is handed.     `MAKEOPTS`/`GNUMAKEFLAGS`: filled from the CPU count when     neither `MAKEOPTS` nor `MAKEFLAGS` is set (`doebuild.py:646-653`).
- 3. Incrementals, folded and sorted like Portage `regenerate()`.
- Portage `config.py:3263-3350`.
- The `PATH`/`MAKEOPTS` step: Portage `doebuild.py:365-378,646-653`.

## tests: with_test_env / synthetic_root
- Same thread-local override `lib.rs`'s `with_test_env` uses; `config_env_all` returns exactly the map.
- The #37 plan's trap §7.5: a synthetic root has no `make.globals`, so everything from there is seeded in `make.conf` explicitly. Mirrors the Portage `arch/amd64/make.defaults` + `base/make.defaults` lines the S0 oracle env came from.
- Empty IUSE (the S0 `porttest/emptydirs` shape): only the implicit flags survive -- `baseflag` is enabled globally but undeclared, so Portage hides it from `use()`.

## tests: phase_environ_exports_the_profile_family_and_folded_incrementals
- `ARCH`/`ELIBC`/`KERNEL`/`ABI`/`DEFAULT_ABI`/`MULTILIB_ABIS`/`LIBDIR_*`/`COMMON_FLAGS`/`CFLAGS`, `USE_EXPAND*`/`IUSE_IMPLICIT`/`USE_EXPAND_VALUES_ARCH`, `ENV_UNSET`: the S0 oracle rows.
- `IUSE_EFFECTIVE` for an empty IUSE is the implicit set; `USE_EXPAND` placeholders (`L10N=""`, `ABI_X86="64"`) derive from the package's USE.

## tests: phase_environ_filters_blacklists_and_never_clobbers_computed_keys
- The env token `evil` stacks on the profile FEATURES list (incremental fold, exactly like Portage); env `USE` is absent for `pkg == None`.

## tests: the_three_key_sets_are_transcribed_consistently
- `PORTAGE_USE` and `O` are the S0 corrections: filtered by Portage.

## const ENV_BLACKLIST
- Portage `special_env_vars.py:15-74`.

## fn phase_environ_pkg
- Portage `config.py:2218-2252`.

## fn in_portage_iuse_body
- The hash-form `___in_portage_iuse` body for EAPIs with `iuse_effective` (5+), byte shape from `config.py:2184-2190`. #348 S2 (E6).

## fn standalone_iuse_effective
- `(IUSE_EFFECTIVE, body)` from declared `IUSE` + profile implicit set, gated on `EapiAttrs.iuse_effective`; `None` below it (Portage serves the `PORTAGE_IUSE`-regex form there, which this port never builds — below the EAPI floor). #348 S2 (E6). Portage: `config.py`, `setcpv`.

## fn filter_unprefixed_use_expand_value / fn apply_unprefixed_use_expand_filter (#349 S4)
- Portage `config.py` `setcpv`'s `USE_EXPAND_UNPREFIXED` loop (`for k in ...: var_split = [x for x in var_split if x in use]`): every set unprefixed variable keeps only the tokens the package's USE names, else `""`. The domain is the narrowed `PORTAGE_USE` -- Portage filters after `difference_update` drops non-`IUSE`/non-implicit flags, so only `IUSE ∪ IUSE_EFFECTIVE` membership can keep a token. S0 `e01-full` oracle (`ARCH=""` against `USE="elibc_glibc"`).
- Placement matters: the filter runs over the *combined* map (scalars from steps 1-3 plus the per-package rows), i.e. `phase_environ` step 5 and the merge tail `entry_phase_env_tail` after its `extend` -- never inside `phase_environ_pkg` alone, whose fresh map never carries the scalar. Unset variables stay unset (Portage only writes the key when it is set somewhere).
