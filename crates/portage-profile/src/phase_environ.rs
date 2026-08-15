//! Portage `config.environ()` for build-phase environments: the export
//! set a source build's every phase sees, built from the resolved
//! [`Config`]. Expected values verified live against Portage.
//!
//! Rules mirrored (`ebuild/config.py`, `_config/special_env_vars.py`):
//! the `env` layer is the whole calling environment (highest priority);
//! `env_blacklist` keys never enter the config; `environ()` skips
//! `environ_filter`, forces `PORTAGE_FEATURES = FEATURES` and
//! `USE = PORTAGE_USE`, and pops `AA`; incrementals export folded and
//! sorted; every `USE_EXPAND` variable derives from the package's
//! effective USE; `IUSE_EFFECTIVE` is declared IUSE plus the implicit set.
//!
//! Deliberately not set here: keys the phase runner computes from the
//! build directory / graph entry ([`PORTUALE_COMPUTED`]) and the dynamic
//! `doebuild_environment()` values the per-entry caller adds on top.
//! `phase_env_vars` appends the caller's `extra_env` last, so anything
//! returned here can still be overridden downstream.
// Portage: ebuild/config.py, _config/special_env_vars.py

use std::collections::{BTreeMap, BTreeSet};

use crate::Config;

/// `env_blacklist` keys: never enter the config from the environment or
/// config files; set programmatically by the phase runner instead. The
/// keys this module computes (`USE`, `IUSE_EFFECTIVE`) insert after the
/// gate on purpose.
// Portage: special_env_vars.py, env_blacklist
pub const ENV_BLACKLIST: &[&str] = &[
    "A",
    "AA",
    "BDEPEND",
    "BROOT",
    "CATEGORY",
    "DEPEND",
    "DESCRIPTION",
    "DOCS",
    "EAPI",
    "EBUILD_FORCE_TEST",
    "EBUILD_PHASE",
    "EBUILD_PHASE_FUNC",
    "EBUILD_SKIP_MANIFEST",
    "ED",
    "EMERGE_FROM",
    "EPREFIX",
    "EROOT",
    "GREP_OPTIONS",
    "HOMEPAGE",
    "IDEPEND",
    "INHERITED",
    "IUSE",
    "IUSE_EFFECTIVE",
    "KEYWORDS",
    "LICENSE",
    "MERGE_TYPE",
    "PDEPEND",
    "PF",
    "PKGUSE",
    "PORTAGE_BACKGROUND",
    "PORTAGE_BACKGROUND_UNMERGE",
    "PORTAGE_BUILDDIR_LOCKED",
    "PORTAGE_BUILT_USE",
    "PORTAGE_CONFIGROOT",
    "PORTAGE_EXPLICIT_INHERIT",
    "PORTAGE_INTERNAL_CALLER",
    "PORTAGE_IUSE",
    "PORTAGE_NONFATAL",
    "PORTAGE_PIPE_FD",
    "PORTAGE_REPO_NAME",
    "PORTAGE_USE",
    "PROPERTIES",
    "RDEPEND",
    "REPOSITORY",
    "REQUIRED_USE",
    "RESTRICT",
    "ROOT",
    "SANDBOX_LOG",
    "SLOT",
    "SRC_URI",
    "_",
];

fn iuse_bare(flag: &str) -> &str {
    flag.trim_start_matches(['+', '-'])
}
