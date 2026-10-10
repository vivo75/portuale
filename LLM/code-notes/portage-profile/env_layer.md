# crates/portage-profile/src/env_layer.rs
The process-environment (`env` `USE_ORDER`) layer over make.conf and the profile.

## module
- Portage passes nearly all of `os.environ` through `backupenv`; portuale uses a curated allowlist instead: it only reads a fixed set of config vars.
- The first list (`ENV_INCREMENTAL_VARS`) is Portage's `INCREMENTALS` portuale models (stacked onto the profile chain + `make.conf` via `apply_incremental`); the second (`ENV_SCALAR_VARS`) is plain last-wins scalars read out of `scalars` / `other_vars` later. `USE_EXPAND` *variable values* (`VIDEO_CARDS=…`) are handled separately, in the expansion loop, once their names are known.

## const TEST_ENV_OVERRIDE
- When `Some`, `config_env_var` reads from this map *instead of* the process environment: the complete env for this thread, so an absent key reads as unset. Lets the env-layer test drive `resolve_config` without `set_var`, which would race every other test thread also calling `resolve_config` (see `with_test_env`).

## fn config_env_var
- `pub` so the `BINPKG_FORMAT` construction sites (portuale's `ebuild` standalone `package`, `portuale::pretend::package_options_from_env`'s config-less arm, backlog #173) read the calling env through the same choke point `env_over_config_scalar` uses: identical production behavior, and the `cfg(test)` `TEST_ENV_OVERRIDE` hook stays available to this crate's own `with_test_env` tests. (External crates built without this crate's `cfg(test)` always take the plain `std::env::var` path; the override map itself stays `pub(crate)` on purpose.)

## fn config_env_all
- Mirrors Portage's `config.__init__`'s `self.backupenv = os.environ.copy()` / `configdict["env"] = LazyItemsDict(self.backupenv)` (`config.py:551,567`): Portage's `env` layer is *all* of the calling environment, not the `ENV_*_VARS` allowlist `resolve_config` reads for `emerge --info`.
- `pub` (not `pub(crate)`) so the `package.env` layering (`portuale::ebuild_phases::match_package_env_vars`, backlog #101) can compare against the same source `phase_environ`'s step 2 uses: Portage's `env` layer outranks its `pkg` layer for a scalar (`config.py:1031-1035`).

## fn apply_env_layer
- Same position Portage's `regenerate()` gives the env layer (after `conf`, before the final USE calculation).
- The `USE` arm: Portage's `USE_ORDER` puts `env` above `pkg` (user `package.use`), the highest tier. `effective_use_flags` replays `env_use_tokens` after the user-level `package.use`. (The global `config.use_flags` above is already env-last for a package with no `package.use` entry.)
- `config_env_var` returns `FEATURES` here as a plain scalar for `other_vars`, but `resolved_incremental` must also see it as the final (highest-priority) source; other `ENV_SCALAR_VARS` entries are genuine last-wins scalars.
- `CONFIG_PROTECT` / `CONFIG_PROTECT_MASK` / `ENV_UNSET`: also in Portage's `const.INCREMENTALS` (`const.py:125-138`), so Portage's `config.regenerate()` stacks the calling env as the final source of their fold like `FEATURES` (`mydbs = configlist[:-1] + [backupenv]`, `config.py:2735-2736`); `emerge --info`'s `settings.get(k)` then prints the stacked, sorted union (backlog #219 fix round 1). Same treatment as `FEATURES` above: the env value is both the final incremental source and the scalar.
- Portage `USE_ORDER` puts `env` above `pkg` (user `package.use`) -- the highest tier. `effective_use_flags` replays `env_use_tokens` after the user-level `package.use`, so `USE="-X" emerge foo` beats a `package.use foo` entry. (The global `config.use_flags` above is already env-last for a package with no `package.use` entry.)
- `FEATURES` is in Portage `const.INCREMENTALS`: a process-env value stacks incrementally on the profile + `make.conf` fold (`-tok` removes, bare adds), it does not replace it. `config_env_var` returns it here as a plain scalar for `other_vars`, but `resolved_incremental` must also see it as the final (highest-priority) source. Other `ENV_SCALAR_VARS` entries are genuine last-wins scalars.
- `CONFIG_PROTECT` / `CONFIG_PROTECT_MASK` / `ENV_UNSET`: also in Portage `const.INCREMENTALS` (`const.py:125-138`), so Portage `config.regenerate()` stacks the calling env as the final source of their fold like `FEATURES` (`mydbs = configlist[:-1] + [backupenv]`, `config.py:2735-2736`) -- `emerge --info`'s `settings.get(k)` then prints the stacked, sorted union (backlog #219 fix round 1). Same treatment as `FEATURES` above: the env value is both the final incremental source and the scalar.

## fn env_over_config_scalar
- Portage's `config.regenerate()`'s `env` `USE_ORDER` layer, the highest-priority source. For variables outside the `ENV_SCALAR_VARS` allowlist (e.g. `BINPKG_FORMAT`, backlog #173) `resolve_config` never folds the env value into `other_vars` itself, so callers that need the full chain read it here instead of `std::env::var` alone (which would miss `make.conf`/profile) or `other_vars` alone (which would miss the calling env). Reads the env through `config_env_var`, so `cfg(test)` threads can drive it via `with_test_env` without touching the process environment.
- The calling-env-over-config value of a plain last-wins scalar: the process environment first (Portage `config.regenerate()`'s `env` `USE_ORDER` layer, the highest-priority source), then the already-folded `make.globals` + profile-chain + `make.conf` value in `other_vars`. For variables outside the `ENV_SCALAR_VARS` allowlist (e.g. `BINPKG_FORMAT`, backlog #173) `resolve_config` never folds the env value into `other_vars` itself, so callers that need the full chain read it here instead of `std::env::var` alone (which would miss `make.conf`/profile) or `other_vars` alone (which would miss the calling env). Reads the env through `config_env_var`, so `cfg(test)` threads can drive it via `with_test_env` without touching Portage's process environment.

## const ENV_INCREMENTAL_VARS
- Config variables portuale honours from the **process environment** -- Portage `config.regenerate()`'s `env` `USE_ORDER` layer, the highest-priority config source (`ACCEPT_KEYWORDS=~amd64 emerge foo`, `USE="-X" emerge bar`, …). Portage passes nearly all of `os.environ` through `backupenv`; portuale uses a curated allowlist instead -- it only reads a fixed set of config vars, and folding `PATH`/`HOME`/… into `other_vars` would pollute `emerge --info`.
