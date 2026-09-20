use super::*;

use portage_util::TempDir;

/// The `portuale` binary built next to this test binary.
fn portuale_exe() -> PathBuf {
    crate::self_exe::built_portuale_bin()
}

mod commands;
mod environment;
mod package_env;
mod phase_env;
mod phase_runs;
mod phase_table;
mod post_install_meta;
mod runtime_paths;
mod sandbox;
mod single_phase;
mod standalone_config;
