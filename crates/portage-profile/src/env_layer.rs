//! The process-environment layer over make.conf and the profile.

use crate::*;

/// Config variables honoured from the process environment: the `env`
/// `USE_ORDER` layer, the highest-priority config source. A curated
/// allowlist: folding `PATH`/`HOME`/… into `other_vars` would pollute
/// `emerge --info`.
// Portage: config.py, regenerate (env layer)
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
    /// When `Some`, `config_env_var` reads from this map instead of the
    /// process environment, so tests need no `set_var` (which would race).
    pub(crate) static TEST_ENV_OVERRIDE: std::cell::RefCell<Option<HashMap<String, String>>> =
        const { std::cell::RefCell::new(None) };
}

/// The process environment as the `env` layer sees it (honouring the
/// test override under `cfg(test)`).
///
/// `pub` so `BINPKG_FORMAT` construction sites read the calling env
/// through the same choke point.
pub fn config_env_var(name: &str) -> Option<String> {
    #[cfg(test)]
    {
        if TEST_ENV_OVERRIDE.with(|o| o.borrow().is_some()) {
            return TEST_ENV_OVERRIDE.with(|o| o.borrow().as_ref().unwrap().get(name).cloned());
        }
    }
    std::env::var(name).ok()
}

/// The whole process environment through the same test override, for
/// `phase_environ` (Portage's `env` layer is all of the calling
/// environment). Non-UTF-8 entries are dropped.
///
/// `pub` so the `package.env` layering compares against the same source.
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

/// Apply the `env` layer: allowlisted vars override (scalars) or stack
/// (incrementals) on top of the profile chain + `make.conf`. Called once,
/// right after `make.conf`.
pub(crate) fn apply_env_layer(scalars: &mut HashMap<String, String>, config: &mut Config) {
    for &name in ENV_INCREMENTAL_VARS {
        let Some(value) = config_env_var(name) else {
            continue;
        };
        match name {
            "USE" => {
                apply_use_incremental(&value, &mut config.use_flags);
                // The `env` tier sits above user `package.use`; replay it
                // last so `USE="-X" emerge foo` beats a `package.use` entry.
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
            // `FEATURES` is incremental: the env value stacks, not replaces.
            if TRACKED_INCREMENTALS.contains(&name) {
                note_incremental(config, name, &value);
            }
            note_higher(config, name);
            scalars.insert(name.to_string(), value);
        }
    }
    // `CONFIG_PROTECT` / `CONFIG_PROTECT_MASK` / `ENV_UNSET` are incremental
    // too: the calling env is their final fold source.
    for name in ["CONFIG_PROTECT", "CONFIG_PROTECT_MASK", "ENV_UNSET"] {
        if let Some(value) = config_env_var(name) {
            note_incremental(config, name, &value);
            note_higher(config, name);
            scalars.insert(name.to_string(), value);
        }
    }
}

/// The calling-env-over-config value of a plain scalar: the process
/// environment first, then the folded `other_vars` value.
pub fn env_over_config_scalar(config: &Config, key: &str) -> Option<String> {
    config_env_var(key).or_else(|| config.other_vars.get(key).cloned())
}
