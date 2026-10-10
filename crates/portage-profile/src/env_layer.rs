//! The process-environment layer over make.conf and the profile.

use crate::*;

/// Config variables portuale honours from the **process environment** --
/// real `config.regenerate()`'s `env` `USE_ORDER` layer, the
/// highest-priority config source (`ACCEPT_KEYWORDS=~amd64 emerge foo`,
/// `USE="-X" emerge bar`, …). Real portage passes nearly all of
/// `os.environ` through `backupenv`; portuale uses a curated allowlist
/// instead -- it only reads a fixed set of config vars, and folding
/// `PATH`/`HOME`/… into `other_vars` would pollute `emerge --info`.
///
/// The first list is real `INCREMENTALS` portuale models (stacked onto
/// the profile chain + `make.conf` via `apply_incremental`); the second
/// is plain last-wins scalars portuale reads out of `scalars` /
/// `other_vars` later. `USE_EXPAND` *variable values* (`VIDEO_CARDS=…`)
/// are handled separately, in the expansion loop, once their names are
/// known.
pub(crate) const ENV_INCREMENTAL_VARS: &[&str] = &[
    "USE",
    "ACCEPT_KEYWORDS",
    "USE_EXPAND",
    "USE_EXPAND_UNPREFIXED",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_HIDDEN",
    "IUSE_IMPLICIT",
];

pub(crate) const ENV_SCALAR_VARS: &[&str] = &[
    "ACCEPT_LICENSE",
    "ACCEPT_PROPERTIES",
    "ACCEPT_RESTRICT",
    "PKGDIR",
    "DISTDIR",
    "PORTAGE_TMPDIR",
    "PORTAGE_LOGDIR",
    "PORTAGE_BINHOST",
    "PORTAGE_NICENESS",
    "PORTAGE_IONICE_COMMAND",
    "PORTAGE_ELOG_SYSTEM",
    "PORTAGE_ELOG_CLASSES",
    "PORTAGE_ELOG_MAILURI",
    "FEATURES",
    "CHOST",
    "CBUILD",
    "CTARGET",
    "CFLAGS",
    "CXXFLAGS",
    "CPPFLAGS",
    "LDFLAGS",
    "FFLAGS",
    "FCFLAGS",
    "MAKEOPTS",
    "EMERGE_DEFAULT_OPTS",
    "PORTAGE_RSYNC_EXTRA_OPTS",
    "GENTOO_MIRRORS",
];

#[cfg(test)]
thread_local! {
    /// When `Some`, `config_env_var` reads from this map *instead of* the
    /// real process environment -- the complete env for this thread, so
    /// an absent key reads as unset. Lets the env-layer test drive
    /// `resolve_config` without `set_var`, which would race every other
    /// test thread also calling `resolve_config` (see `with_test_env`).
    pub(crate) static TEST_ENV_OVERRIDE: std::cell::RefCell<Option<HashMap<String, String>>> =
        const { std::cell::RefCell::new(None) };
}

/// The process environment as the `env` `USE_ORDER` layer sees it -- real
/// `std::env::var`, except under `cfg(test)` with a `TEST_ENV_OVERRIDE`
/// installed, when that map is authoritative.
///
/// `pub` so the `BINPKG_FORMAT` construction sites (`portuale::ebuild`'s
/// standalone `package`, `portuale::pretend::package_options_from_env`'s
/// config-less arm, backlog #173) read the calling env through the same
/// choke point `env_over_config_scalar` uses -- identical production
/// behavior, and the `cfg(test)` `TEST_ENV_OVERRIDE` hook stays
/// available to this crate's own `with_test_env` tests. (External
/// crates built without this crate's `cfg(test)` always take the plain
/// `std::env::var` path; the override map itself stays `pub(crate)` on
/// purpose.)
pub fn config_env_var(name: &str) -> Option<String> {
    #[cfg(test)]
    {
        if TEST_ENV_OVERRIDE.with(|o| o.borrow().is_some()) {
            return TEST_ENV_OVERRIDE.with(|o| o.borrow().as_ref().unwrap().get(name).cloned());
        }
    }
    std::env::var(name).ok()
}

/// The *whole* process environment, through the same test override
/// `config_env_var` honours -- for `phase_environ`, which mirrors real
/// `config.__init__`'s `self.backupenv = os.environ.copy()` /
/// `configdict["env"] = LazyItemsDict(self.backupenv)`
/// (`config.py:551,567`): real's `env` layer is *all* of the calling
/// environment, not the `ENV_*_VARS` allowlist `resolve_config` reads
/// for `emerge --info`. Non-UTF-8 entries are dropped (real's are
/// `str` already).
///
/// `pub` (not `pub(crate)`) so the `package.env` layering
/// (`portuale::ebuild_phases::match_package_env_vars`, backlog #101) can
/// compare against the same source `phase_environ`'s step 2 uses: real's
/// `env` layer outranks its `pkg` layer for a scalar
/// (`config.py:1031-1035`).
pub fn config_env_all() -> Vec<(String, String)> {
    #[cfg(test)]
    {
        if TEST_ENV_OVERRIDE.with(|o| o.borrow().is_some()) {
            return TEST_ENV_OVERRIDE.with(|o| {
                let mut v: Vec<(String, String)> = o
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                v.sort();
                v
            });
        }
    }
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}

/// Apply the `env` `USE_ORDER` layer: for every allowlisted config var
/// present in the process environment, override (scalars) or stack
/// (incrementals) on top of the profile chain + `make.conf`. Called once,
/// right after `make.conf` -- the same position real `regenerate()`
/// gives the env layer (after `conf`, before the final USE calculation).
pub(crate) fn apply_env_layer(scalars: &mut HashMap<String, String>, config: &mut Config) {
    for &name in ENV_INCREMENTAL_VARS {
        let Some(value) = config_env_var(name) else {
            continue;
        };
        match name {
            "USE" => {
                apply_use_incremental(&value, &mut config.use_flags);
                // Real `USE_ORDER` puts `env` above `pkg` (user
                // `package.use`) -- the highest tier. `effective_use_flags`
                // replays `env_use_tokens` after the user-level
                // `package.use`, so `USE="-X" emerge foo` beats a
                // `package.use foo` entry. (The global `config.use_flags`
                // above is already env-last for a package with no
                // `package.use` entry.)
                config.env_use_tokens.push(value.clone());
            }
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            "USE_EXPAND" => apply_incremental(&value, &mut config.use_expand),
            "USE_EXPAND_UNPREFIXED" => apply_incremental(&value, &mut config.use_expand_unprefixed),
            "USE_EXPAND_IMPLICIT" => apply_incremental(&value, &mut config.use_expand_implicit),
            "USE_EXPAND_HIDDEN" => apply_incremental(&value, &mut config.use_expand_hidden),
            "IUSE_IMPLICIT" => apply_incremental(&value, &mut config.iuse_implicit),
            _ => {}
        }
        note_higher(config, name);
        scalars.insert(name.to_string(), value);
    }
    for &name in ENV_SCALAR_VARS {
        if let Some(value) = config_env_var(name) {
            // `FEATURES` is in real `const.INCREMENTALS`: a process-env
            // value stacks incrementally on the profile + `make.conf`
            // fold (`-tok` removes, bare adds), it does not replace it.
            // `config_env_var` returns it here as a plain scalar for
            // `other_vars`, but `resolved_incremental` must also see it
            // as the final (highest-priority) source. Other
            // `ENV_SCALAR_VARS` entries are genuine last-wins scalars.
            if TRACKED_INCREMENTALS.contains(&name) {
                note_incremental(config, name, &value);
            }
            note_higher(config, name);
            scalars.insert(name.to_string(), value);
        }
    }
    // `CONFIG_PROTECT` / `CONFIG_PROTECT_MASK` / `ENV_UNSET`: also in
    // real `const.INCREMENTALS` (`const.py:125-138`), so real
    // `config.regenerate()` stacks the calling env as the final source
    // of their fold like `FEATURES` (`mydbs = configlist[:-1] +
    // [backupenv]`, `config.py:2735-2736`) -- `emerge --info`'s
    // `settings.get(k)` then prints the stacked, sorted union (backlog
    // #219 fix round 1). Same treatment as `FEATURES` above: the env
    // value is both the final incremental source and the scalar.
    for name in ["CONFIG_PROTECT", "CONFIG_PROTECT_MASK", "ENV_UNSET"] {
        if let Some(value) = config_env_var(name) {
            note_incremental(config, name, &value);
            note_higher(config, name);
            scalars.insert(name.to_string(), value);
        }
    }
}

/// The calling-env-over-config value of a plain last-wins scalar: the
/// process environment first (real `config.regenerate()`'s `env`
/// `USE_ORDER` layer, the highest-priority source), then the
/// already-folded `make.globals` + profile-chain + `make.conf` value in
/// `other_vars`. For variables outside the `ENV_SCALAR_VARS` allowlist
/// (e.g. `BINPKG_FORMAT`, backlog #173) `resolve_config` never folds
/// the env value into `other_vars` itself, so callers that need the
/// full chain read it here instead of `std::env::var` alone (which
/// would miss `make.conf`/profile) or `other_vars` alone (which would
/// miss the calling env). Reads the env through `config_env_var`, so
/// `cfg(test)` threads can drive it via `with_test_env` without
/// touching the real process environment.
pub fn env_over_config_scalar(config: &Config, key: &str) -> Option<String> {
    config_env_var(key).or_else(|| config.other_vars.get(key).cloned())
}
