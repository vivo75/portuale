/// Direct unit tests for the eight pure selection predicates: each
/// test observes the predicate's own return value.
use crate::*;
use portage_util::TempDir;

/// Minimal merge-bound `GraphEntry` for the predicate legs (slot
/// and `new_slot` as the leg needs; everything display-side left
/// at its empty default).
fn entry_162(
    cat: &str,
    pkg: &str,
    outcome: PretendOutcome,
    slot: Option<&str>,
    new_slot: bool,
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: cat.to_string(),
        package: pkg.to_string(),
        outcome,
        blockers: Vec::new(),
        slot: slot.map(str::to_string),
        sub_slot: slot.map(str::to_string),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

/// A `Reinstall` with only a slot change enables on `if_new_ver`
/// alone; the crossed shape stays off.
#[test]
fn complete_graph_auto_enable_reinstall_slot_change_needs_if_new_ver() {
    let slot_churn = entry_162(
        "dev-libs",
        "re",
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: true,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        Some("0"),
        false,
    );
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: std::slice::from_ref(&slot_churn),
        if_new_use: true,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: std::slice::from_ref(&slot_churn),
        if_new_use: false,
        if_new_ver: true,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &[slot_churn],
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    let crossed = entry_162(
        "dev-libs",
        "re",
        PretendOutcome::Reinstall {
            version: "1.0".to_string(),
            changed_flags: vec!["flip".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        Some("0"),
        false,
    );
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &[crossed],
        if_new_use: false,
        if_new_ver: true,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
}

/// Settled entries never enable, whatever the flags.
#[test]
fn complete_graph_auto_enable_settled_entries_stay_off() {
    let entries = vec![
        entry_162(
            "dev-libs",
            "kept",
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string(),
            },
            None,
            false,
        ),
        entry_162(
            "dev-libs",
            "gone",
            PretendOutcome::NoVisibleCandidate,
            None,
            false,
        ),
    ];
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &entries,
        if_new_use: true,
        if_new_ver: true,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
}

/// Fixed "amd64"-only, no-overrides config, mirroring `tests`'
/// own `test_config` (visibility is pinned the same way so a
/// `KEYWORDS="amd64"` scratch ebuild is always visible).
fn cfg_162() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

/// Direct `Candidate` literal for the visibility-free legs (mirrors
/// the existing `candidate()` helper's full field set).
fn cand_162(version: &str, keywords: &[&str], slot: &str) -> Candidate {
    Candidate {
        version: version.to_string(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        slot: slot.to_string(),
        sub_slot: slot.to_string(),
        repo_location: PathBuf::new(),
        repo_priority: 0,
        repo_name: "test".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Ebuild,
        binary_use: None,
        remote: false,
        build_id: None,
        build_time: None,
        binary_deps: HashMap::new(),
    }
}
/// Arguments of [`demote_162`].
struct Demote162Args<'a> {
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    self_cp: &'a RootCp,
    atoms: &'a [&'a str],
    queued: &'a [QueueItem],
    update: bool,
}
