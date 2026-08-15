/// Direct predicate-result legs in the scratch style of `tests_161`/`tests_162`: each
/// test observes the predicate's own return value. Do not edit other
/// items' `mod tests_16x` blocks.
use crate::*;

// ---- `direct_solve_arg_mode` (Portage `_want_installed_pkg`). ----

/// Minimal visibility config in the `test_config` shape
/// (amd64-only, no overrides).
fn cfg_166() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

fn inst_166(version: &str, installed: bool) -> SlotConflictInstance {
    SlotConflictInstance {
        version: version.to_string(),
        sub_slot: "0".to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: Vec::new(),
        installed,
    }
}

// ---- `synthesize_surviving_conflict_entries` (files instances adjacently). ----

/// One conflict with a single merge-bound (non-installed) instance.
fn synth_conflict_166() -> SlotConflict {
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "foo".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: "dev-libs/foo".to_string(),
        instances: vec![inst_166("2.0", false)],
    }
}
