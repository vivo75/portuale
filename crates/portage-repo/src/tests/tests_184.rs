/// Shadowed-arm order legs for `direct_solve_instance_use` /
/// `direct_solve_atom_matches` (self-contained scratch helpers).
use crate::*;
use portage_util::TempDir;

/// Minimal visibility config (amd64-only, no overrides).
fn cfg_184() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}
