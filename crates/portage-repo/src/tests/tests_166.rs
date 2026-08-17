/// Direct predicate-result legs in the scratch style of `tests_161`/`tests_162`: each
/// test observes the predicate's own return value. Do not edit other
/// items' `mod tests_16x` blocks.
use crate::*;
use portage_util::TempDir;

// ---- `filter_usepkg_exclude_include` (Portage usepkg-exclude/include). ----

/// Hand-built binary candidate in the `cand_162` shape (same
/// fields; `candidate_str` renders `cat/pkg-ver:slot/sub::repo`).
fn cand_166(version: &str) -> Candidate {
    Candidate {
        version: version.to_string(),
        keywords: vec!["amd64".to_string()],
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: 0,
        repo_name: "testrepo".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Binary,
        binary_use: None,
        remote: false,
        build_id: None,
        build_time: None,
        binary_deps: HashMap::new(),
    }
}

// ---- `direct_solve_arg_mode` (Portage `_want_installed_pkg`). ----

/// Minimal visibility config in the `test_config` shape
/// (amd64-only, no overrides).
fn cfg_166() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

/// One conflict over `(version, installed)` instances (parents are
/// irrelevant to the arg-mode verdict).
fn conflict_166(versions: Vec<(&str, bool)>) -> SlotConflict {
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "argpkg".to_string(),
        slot: "0".to_string(),
        resolved_version: versions[0].0.to_string(),
        conflicting_atom: "dev-libs/argpkg".to_string(),
        instances: versions
            .into_iter()
            .map(|(v, installed)| SlotConflictInstance {
                version: v.to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed,
            })
            .collect(),
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
