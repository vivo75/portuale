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

/// One conflict with a single merge-bound (non-installed) instance.
fn conflict_184() -> SlotConflict {
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "iupkg".to_string(),
        slot: "0".to_string(),
        resolved_version: "1.0".to_string(),
        conflicting_atom: "dev-libs/iupkg".to_string(),
        instances: vec![inst_184("1.0", false)],
    }
}
