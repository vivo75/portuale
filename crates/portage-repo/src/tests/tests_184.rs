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

fn inst_184(version: &str, installed: bool) -> SlotConflictInstance {
    SlotConflictInstance {
        version: version.to_string(),
        sub_slot: "0".to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: Vec::new(),
        installed,
    }
}
