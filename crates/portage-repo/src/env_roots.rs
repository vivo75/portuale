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

/// Real `cnf/make.globals:35`'s own `PORTAGE_TMPDIR="/var/tmp"` -- the
/// fallback when neither the calling environment nor the resolved config
/// sets one. Real `doebuild_environment()` builds the per-package
/// `${PORTAGE_TMPDIR}/portage/${CATEGORY}/${PF}` on top of this value
/// (`doebuild.py:504-524`) -- the double `portage/portage/` builddir bug
/// `TEST/findings/l2.md` "#38 recon" (`l2-gpkg-dostrip-splitdebug` S0)
/// traced to this boundary defaulting to `/var/tmp/portage` instead.
pub const PORTAGE_TMPDIR_DEFAULT: &str = "/var/tmp";

/// `PORTAGE_TMPDIR` for a CLI boundary with no resolved config
/// (`ebuild <file>`, `execute_unmerge`, `emerge --regen`): the calling
/// environment (real's highest config layer) if set, else real's own
/// `make.globals` default.
pub fn portage_tmpdir_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PORTAGE_TMPDIR_DEFAULT))
}

/// The resolved config's own `PORTAGE_TMPDIR` scalar (`make.globals` +
/// profile chain + `make.conf`, folded into `Config::other_vars`), else
/// real's default. Real's calling environment is the highest config
/// layer, so the CLI boundaries apply [`portage_tmpdir_from_env`]'s
/// precedence on top of this.
pub fn portage_tmpdir_from_config(config: &portage_profile::Config) -> PathBuf {
    config
        .other_vars
        .get("PORTAGE_TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PORTAGE_TMPDIR_DEFAULT))
}

/// `--root-deps`'s own real running-root default: real `ESYSROOT`
/// resolves to the real *build machine's* own `/` whenever `SYSROOT` is
/// left unset (see `running_root_satisfies_atom`'s own doc comment for
/// the full grounding) -- `/` is the correct real default here too, for
/// the same reason `config_root_from_env`/`root_from_env` default to
/// `/`. `PORTAGE_RUNNING_ROOT` itself is NOT a real portage environment
/// variable (real portage has no way to override this at all -- it's
/// always genuinely `/`, full stop, outside of Prefix's own `BROOT`
/// concept) -- a portuale-specific override purely so a test can point this
/// at a fixture's own fake vdb tree instead of the real host, the same
/// "explicit override for tests, real default at the CLI boundary"
/// pattern `MergeOptions::config_root` already established.
pub fn running_root_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_RUNNING_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
