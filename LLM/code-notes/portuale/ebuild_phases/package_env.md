# crates/portuale/src/ebuild_phases/package_env.rs
`package.env` matching and its incremental variables.

## const PACKAGE_ENV_INCREMENTALS
- Full Portage list (`const.py`).

## fn package_env_key_allowed
- Portage's gate rejects `protected_keys` ∪ `non_user_variables` (PROFILE_ONLY ∪ env_blacklist ∪ CONFIG_PROTECT); portuale additionally rejects its `ENVIRON_FILTER` export-filter list — a deliberate conservative narrowing, except the compression family.
- `USE` is Portage's special case (popped before the call, re-appended after `package.use`); portuale folds it via `Config::package_env_use` instead. `ENV_BLACKLIST` is Portage's `env_blacklist` (covers `PKGUSE`, the only `protected_pkg_keys` member). `ENVIRON_FILTER` keys Portage accepts into the config but never exports to a phase (`config.environ()`); the `package_env_vars` layer exists only in the phase env, so they drop. `PORTUALE_COMPUTED` keys the phase runner computes and must always win (`FEATURES`/`PORTAGE_TMPDIR` residues plus owned `PATH`, `DISTDIR`, `D`, ...). `profile_only_variables` is the profile's dynamic `PROFILE_ONLY_VARIABLES` (`ARCH`, `ELIBC`, `IUSE_IMPLICIT`, `USE_EXPAND*`, ...).
- Portage blanks a variable with `VAR=""` and does not strip `__`-prefixed names here.
- Compression exemption: Portage honors per-package `BINPKG_COMPRESS` (and its FLAGS sisters: `doebuild.py` reads all three from the package's `mysettings`, none in `non_user_variables`); every other `ENVIRON_FILTER` key stays rejected.

## fn fold_package_env_incremental
- A package.env `-tok` cannot prune a calling-env token (verified: calling-env `-tok` prunes a package.env addition). `base_lower` is `""` when the run-wide env lacks the key.

## fn match_package_env_vars
- Portage keeps parse order vs `ordered_by_atom_specificity`: pre-existing narrowing, portuale keeps parse order.
- Scalar-drop is Portage's `USE_ORDER` (`env` layer above `pkg`): the process value wins. `calling_env` is `portage_profile::config_env_all()` (same source as `phase_environ` step 2), compared by key presence — not `base_env` as a whole, which also holds config scalars a package.env value legitimately outranks.
- Merge path matches a resolved graph entry (`emerge_build::entry_package_env_vars`, `options.build_env`); standalone path matches the ebuild's md5-cache identity with the narrow base.

## fn match_package_env_scalar_raw / incremental_raw
- The gated phase-env path (`match_package_env_vars`) keeps its own loop; per-key recomputes where a flat pair would corrupt — `PORTAGE_TMPDIR` and `FEATURES` (both `PORTUALE_COMPUTED`) — match raw and re-derive instead.

## fn standalone_package_env_lookup
- Shared by the standalone `PORTAGE_TMPDIR` and resolved-`FEATURES` lookups so path-parsing happens once. Uses `resolve_standalone_config` (the `package_env_vars` table) and `standalone_cpv_slot` (md5-cache `SLOT`).
