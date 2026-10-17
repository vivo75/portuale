//! ROOT, PORTAGE_CONFIGROOT and PORTAGE_TMPDIR from the environment.

use crate::*;

pub fn config_root_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_CONFIGROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn root_from_env() -> PathBuf {
    std::env::var_os("ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Fallback when neither the environment nor the config sets one.
/// Portage: cnf/make.globals `PORTAGE_TMPDIR="/var/tmp"`
pub const PORTAGE_TMPDIR_DEFAULT: &str = "/var/tmp";

/// Fallback when neither the environment nor the config sets one.
/// Portage: cnf/make.globals:30 `DISTDIR="/var/cache/distfiles"`
pub const DISTDIR_DEFAULT: &str = "/var/cache/distfiles";

/// `PORTAGE_TMPDIR` for a CLI boundary with no resolved config: the calling
/// environment (Portage's highest config layer) if set, else the default.
pub fn portage_tmpdir_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PORTAGE_TMPDIR_DEFAULT))
}

/// The resolved config's own `PORTAGE_TMPDIR` scalar, else the default.
/// Callers layer the calling environment on top (highest config layer).
pub fn portage_tmpdir_from_config(config: &portage_profile::Config) -> PathBuf {
    config
        .other_vars
        .get("PORTAGE_TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PORTAGE_TMPDIR_DEFAULT))
}

/// `PORTAGE_TMPDIR` for a CLI boundary with a resolved config: the calling
/// environment (Portage's highest config layer) if set, else the config.
pub fn portage_tmpdir_from_env_or_config(config: &portage_profile::Config) -> PathBuf {
    std::env::var_os("PORTAGE_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| portage_tmpdir_from_config(config))
}

/// The resolved config's own `DISTDIR` scalar, else the default.
/// Callers layer the calling environment on top (highest config layer).
pub fn distdir_from_config(config: &portage_profile::Config) -> PathBuf {
    config
        .other_vars
        .get("DISTDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DISTDIR_DEFAULT))
}

/// `DISTDIR` for a CLI boundary with a resolved config: the calling
/// environment (Portage's highest config layer) if set, else the config.
pub fn distdir_from_env_or_config(config: &portage_profile::Config) -> PathBuf {
    std::env::var_os("DISTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| distdir_from_config(config))
}

/// `--root-deps`' running-root default: `/`, since ESYSROOT resolves to the
/// build machine's own `/` when SYSROOT is unset. `PORTAGE_RUNNING_ROOT` is
/// not a Portage variable -- a portuale-only test override for fixture vdb
/// trees, mirroring `MergeOptions::config_root`.
pub fn running_root_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_RUNNING_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(vars: &[(&str, &str)]) -> portage_profile::Config {
        let mut config = portage_profile::Config::default();
        for (k, v) in vars {
            config.other_vars.insert(k.to_string(), v.to_string());
        }
        config
    }

    /// Guard a process-env key around a closure: `set_var`/`remove_var`
    /// are process-global, so every env-touching test saves and restores.
    /// Portage 3.0.82.2 reads the calling env over `make.conf` for both
    /// keys (`config.py`, the `env` configdict layer).
    fn with_env(key: &str, value: Option<&str>, f: impl FnOnce()) {
        let saved = std::env::var_os(key);
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        f();
        unsafe {
            match saved {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn distdir_prefers_the_calling_env_then_the_config_then_the_default() {
        // Portage 3.0.82.2, S0 cell `e01-full`: make.conf `DISTDIR`
        // becomes `PORTAGE_ACTUAL_DISTDIR`; `e02-nodistdir` falls back
        // to the per-package fake under `make.globals`' default dir.
        with_env("DISTDIR", None, || {
            assert_eq!(
                distdir_from_env_or_config(&config_with(&[("DISTDIR", "/conf/dist")])),
                PathBuf::from("/conf/dist")
            );
            assert_eq!(
                distdir_from_config(&config_with(&[("DISTDIR", "")])),
                PathBuf::from(DISTDIR_DEFAULT)
            );
            assert_eq!(
                distdir_from_env_or_config(&config_with(&[])),
                PathBuf::from(DISTDIR_DEFAULT)
            );
        });
        with_env("DISTDIR", Some("/env/dist"), || {
            assert_eq!(
                distdir_from_env_or_config(&config_with(&[("DISTDIR", "/conf/dist")])),
                PathBuf::from("/env/dist")
            );
        });
    }

    #[test]
    fn portage_tmpdir_prefers_the_calling_env_then_the_config_then_the_default() {
        // Portage 3.0.82.2, S0 cells `e02-nodistdir`/`e03-notmpdir`:
        // make.conf `PORTAGE_TMPDIR` is honored; unset everywhere it
        // falls back to `make.globals`' `/var/tmp`.
        with_env("PORTAGE_TMPDIR", None, || {
            assert_eq!(
                portage_tmpdir_from_env_or_config(&config_with(&[("PORTAGE_TMPDIR", "/conf/tmp")])),
                PathBuf::from("/conf/tmp")
            );
            assert_eq!(
                portage_tmpdir_from_env_or_config(&config_with(&[])),
                PathBuf::from(PORTAGE_TMPDIR_DEFAULT)
            );
        });
        with_env("PORTAGE_TMPDIR", Some("/env/tmp"), || {
            assert_eq!(
                portage_tmpdir_from_env_or_config(&config_with(&[("PORTAGE_TMPDIR", "/conf/tmp")])),
                PathBuf::from("/env/tmp")
            );
        });
    }
}
