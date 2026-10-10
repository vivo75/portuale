# crates/portage-repo/src/tests/info_pkgs.rs
`--info` display data tests: the info_pkgs table, ebuild/installed/binary candidates, and env-file parsing.

## fn resolve_installed_info_reads_the_vdb_use_line_and_the_differing_vars
- dev-libs/infoinstpkg-1.0: vdb IUSE="alpha beta" USE="alpha", CFLAGS/CHOST recorded, while `test_config`'s make.conf sets neither.

## fn parse_env_assignments_matches_portage_aux_env_search_forms
- Portage `vartree._aux_env_search` (`dbapi/vartree.py:1082-1123`): plain, `declare -FLAGS`, `declare`, and `export` prefixes; single-line quoted values; multi-line continuations joined with `\n` separators (only the first fragment has none); last assignment wins; an escaped trailing `\"` does NOT close (it swallows following lines instead).
- The escaped `\"` swallows every following line through EOF, then the trailing char is dropped -- Portage's own EOF-without-closer path, verbatim (`UNWANTED=zz`, not `zzz`).

## fn parse_env_assignments_skips_continuations_of_unwanted_keys
- Portage builds the value (consuming lines) before its `if key in variables` check.

## fn resolve_installed_info_unsets_everything_without_environment_bz2
- Portage's `_aux_env_search` with no `environment.bz2` returns {} -- all five `mydesiredvars` are `Unset:`, even when a same-named individual vdb file exists (Portage never opens it for this block). The stray `CFLAGS` file pins exactly that.

## fn resolve_installed_info_reads_mydesiredvars_only_from_the_saved_env
- `environment.bz2` carries CHOST; the stray individual CFLAGS file is ignored (Portage's `_aux_env_search`); CXXFLAGS/FEATURES/LDFLAGS are in neither, so `Unset:`.

## fn resolve_installed_info_wraps_a_globally_forced_flag
- dev-libs/infoforcedpkg-1.0: vdb IUSE="globalforceflag otherflag" USE="globalforceflag". `test_config()` is a minimal synthetic config (no profile actually read), so `use_force` is set directly here -- Portage's `pkg_use_display` wraps a forced flag `(globalforceflag)` regardless of its own enabled state; otherflag renders plain `-otherflag`.

## fn resolve_installed_info_flags_a_vdb_recorded_pkg_info_phase
- infoinstpkg's vdb has NO DEFINED_PHASES file at all -> Portage's `if metadata["DEFINED_PHASES"]:` is falsy -> `pkg_info()` IS attempted (the falsy-check quirk, `actions.py:2350`).
- infodashphases records `DEFINED_PHASES="-"` (truthy, doesn't name `info`) -> NOT attempted.

## fn resolve_info_binary_candidate_picks_a_pkg_info_defining_local_binpkg
- dev-libs/binaryinfopkg: $PKGDIR binary only (no ebuild), its Packages entry has EAPI 8 + DEFINED_PHASES=info + a baked USE="alpha". Portage's `(bindb, "binary")` selects it for `--usepkg --info`.
- A binary with DEFINED_PHASES="-" (binaryonlypkg) is not a pkg_info() candidate.
