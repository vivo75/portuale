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

/// `--root-deps`' running-root default: `/`, since ESYSROOT resolves to the
/// build machine's own `/` when SYSROOT is unset. `PORTAGE_RUNNING_ROOT` is
/// not a Portage variable -- a portuale-only test override for fixture vdb
/// trees, mirroring `MergeOptions::config_root`.
pub fn running_root_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_RUNNING_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
