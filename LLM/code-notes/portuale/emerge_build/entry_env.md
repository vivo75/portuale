# crates/portuale/src/emerge_build/entry_env.rs
The per-entry build environment: tmpdir, FEATURES, USE, identity and metadata rows.

## fn entry_portage_tmpdir
- Ports Portage's per-package `PORTAGE_TMPDIR` (backlog #99) via `ebuild_phases::resolve_entry_portage_tmpdir`. A missing matched directory is Portage's `_check_temp_dir` failure.

## fn resolved_features_for
- Ports Portage's per-package `FEATURES` (backlog #98). `FEATURES` is an incremental, so the matched env-file value folds in `[run-wide, pkg, calling-env]` order (`regenerate()`, `config.py:2735`, `:2778-2825`), instead of replacing it. Entries without a `package.env` `FEATURES` entry keep byte-identical output. Raw pieces, not `&MergeOptions`, so `run_buildpkgonly` (backlog #129, no `MergeOptions` in scope) shares this with the merge scheduler's `entry_resolved_features`. `None` when nothing matched: the caller then keeps the run-wide value untouched.

## fn entry_resolved_features
- `calling_features` is the raw calling-environment value as a parameter, not an ambient read, so the fold stays unit-testable. The run-wide base is `options.features` (the resolved list on `emerge` paths, the raw process fallback elsewhere).

## fn build_use_env
- The build-phase env every `emerge <atom>` source build/merge passes so `bin/ebuild.sh`'s `use()` sees the resolved flags. `phase_env_vars`' own `USE=""` stands when empty. `GraphEntry::use_flags_display` is already the package's IUSE, enabled-resolved and bare-name-sorted.

## fn entry_cpv_slot
- The identity Portage's `_grab_pkg_env` matches a build-bound entry against; `entry_package_env_vars` and the per-package `PORTAGE_TMPDIR` resolution share it.

## fn matched_package_env_vars
- Ports Portage's `_grab_pkg_env` folding a matching `/etc/portage/package.env` entry's env file into `configdict["pkg"]`, with Portage's acceptance set (`match_package_env_vars`). Incrementals fold onto `base_env` (the run-wide resolved env) instead of replacing it, matching `regenerate()`'s layer stacking. Raw pieces, not `&MergeOptions`, so `run_buildpkgonly` (backlog #129) shares this with `entry_package_env_vars`.

## fn entry_identity_env
- The per-entry identity Portage's `doebuild_environment()` sets (`doebuild.py:483`) and `EbuildPhase._setup_repo_revisions` builds (`EbuildPhase.py:73-108`). `SLOT` is the ebuild's own `SLOT` (`slot/sub_slot` when a sub-slot is declared, bare slot otherwise), never empty (`entry.slot` unset -> `"0"`). `SLOT` matches the ebuild global Portage's `config.environ()` carries; the `.keep_<cp>-` bug is exactly an empty slot. `PORTAGE_REPO_REVISIONS` is Portage's `json.dumps({}, sort_keys=True)`. `PORTAGE_REPO_NAME` is omitted only when no repo is known at all.

## fn run_wide_phase_env
- `portage_profile::phase_environ(config, None)` plus the dynamic `doebuild_environment()` value needing portuale's compressor table, `PORTAGE_COMPRESSION_COMMAND` (`doebuild.py:697-750`, set for every build regardless of `BINPKG_FORMAT`). `BINPKG_COMPRESS*` are `environ_filter`ed, so they are read from the calling env / config directly; `MAKEOPTS` from the (already-defaulted) export set.

## const EXPORTED_PKG_METADATA
- The package metadata keys Portage's `config.setcpv` loads into `configdict["pkg"]` (`_setcpv_aux_keys`, `config.py:171-189`) that survive `environ_filter` and stay exported in the saved environment: `bin/ebuild.sh` unsets `IUSE`/`REQUIRED_USE`/`PROPERTIES`/`RESTRICT`/`INHERITED`/`EAPI` before sourcing the ebuild (`:634-635`), so only these three keep Portage's `declare -x`; `SLOT`/`EAPI`/`INHERITED` are computed elsewhere. Values from the md5-cache (Portage `aux_get`; an omitted key is `""`).

## fn entry_metadata_env
- The repo-metadata decision point (H4, backlog #41's `repo_aux_metadata`: repo md5-cache -> depcachedir -> depend phase) is reached through the `RepoCache` slot on this production path, the same interchangeable-traits seam the scheduler/merge/news/fetch/packages/binpkg slots use. `Md5Cache::metadata` delegates to the identical function the direct call used, so this is behaviour-neutral.

## fn entry_phase_env_tail
- With a config in scope (`emerge <atom>` and friends, #37 S2) the `USE` is Portage's `PORTAGE_USE`: the resolver's full effective set (`candidate_effective_use_flags`, including the implicit profile flags no package declares in `IUSE`) narrowed by `portage_use` to `IUSE ∪ IUSE_EFFECTIVE`. Without a config (standalone `ebuild <file> merge`/`qmerge`, tests) the previous enabled-IUSE-only `USE` stands. Shared with `run_buildpkgonly`, whose `PackageOptions` carries no `MergeOptions`. The tail is resolved `USE` (or the legacy enabled-IUSE-only `USE` when no config is in scope), the `IUSE_EFFECTIVE`/`USE_EXPAND` rows, and the `SLOT`/repo identity.

## fn entry_build_env
- The run-wide resolved env the caller stashed on `options.build_env` (`portage_profile::phase_environ(config, None)`), then any per-package `package.env` build vars on top of those, then `entry_phase_env_tail`.
- Per-package `BINPKG_COMPRESS` (backlog #147 S2): same re-derivation as `buildpkgonly_entry_build_env`; this env feeds the merge-scheduler builds whose `package_after_install` side effect (`FEATURES=buildpkg` / `-b`) reads it back through the saved `${T}/environment`. The resolved config may be absent on the config-less paths.
- Backlog #332: re-read env.d for this package task (`env_d_reloaded`); unchanged `profile.env` keeps the old path.
- Per-package `FEATURES` (#98): the folded per-entry list overrides the run-wide `FEATURES`/`PORTAGE_FEATURES` pair downstream (last-wins), so the phase env and every `extra_env` gate see it. `None` (no match) appends nothing, so entries without a `package.env` `FEATURES` entry keep byte-identical output.
