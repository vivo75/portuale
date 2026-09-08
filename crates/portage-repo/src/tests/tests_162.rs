/// Direct unit tests for the eight pure selection predicates: each
/// test observes the predicate's own return value.
use crate::*;
use portage_util::TempDir;

/// `bare_cp` rejects each non-bare form individually.
#[test]
fn bare_cp_rejects_every_non_bare_form() {
    for rejected in [
        "!dev-libs/foo",
        "!!dev-libs/foo",
        ">=dev-libs/foo-1.0",
        "=dev-libs/foo-1.0",
        "dev-libs/foo-1.0",
        "dev-libs/foo:0",
        "dev-libs/foo:=",
        "dev-libs/foo[bar]",
        "dev-libs/foo::gentoo",
        "not an atom at all !!!",
    ] {
        assert_eq!(bare_cp(rejected), None, "{rejected}");
    }
}

/// Fresh unique scratch dir per test (vdb + repo live under it, so
/// the per-root memo caches never see a reused path).
fn dir_162(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-162-{tag}")).keep()
}

/// One installed instance under `<root>/var/db/pkg` with the given
/// `SLOT` plus caller-supplied field files (`USE`, `IUSE`, ...).
fn install_162(root: &Path, cat: &str, pf: &str, slot: &str, files: &[(&str, &[u8])]) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    for (name, bytes) in files {
        std::fs::write(d.join(name), bytes).unwrap();
    }
}

fn atoms_162(atoms: &[&str]) -> Vec<String> {
    atoms.iter().map(|a| a.to_string()).collect()
}

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

// ---- `complete_graph_auto_enable` (the `--complete` re-walk
// trigger: an `Upgrade`/`Downgrade`, a new-slot `New`, or a
// `Reinstall` with USE/slot churn enables it). ----

/// Any `Upgrade`/`Downgrade` enables when `if_new_ver` is set, and
/// neither does when it is not.
#[test]
fn complete_graph_auto_enable_upgrade_and_downgrade_follow_if_new_ver() {
    let entries = vec![
        entry_162(
            "dev-libs",
            "up",
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            Some("0"),
            false,
        ),
        entry_162(
            "dev-libs",
            "down",
            PretendOutcome::Downgrade {
                from: "2.0".to_string(),
                to: "1.0".to_string(),
            },
            Some("0"),
            false,
        ),
    ];
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &entries,
        if_new_use: true,
        if_new_ver: true,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &entries,
        if_new_use: false,
        if_new_ver: true,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &entries,
        if_new_use: true,
        if_new_ver: false,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &entries,
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
}

/// A new-slot `New` enables on `if_new_ver` or `if_new_slot`
/// alone; a same-slot `New` never does even with every flag set.
#[test]
fn complete_graph_auto_enable_new_only_for_a_new_slot() {
    let new_slot = vec![entry_162(
        "dev-libs",
        "fresh",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("1"),
        true,
    )];
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &new_slot,
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &new_slot,
        if_new_use: false,
        if_new_ver: true,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &new_slot,
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    let same_slot = vec![entry_162(
        "dev-libs",
        "fresh",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &same_slot,
        if_new_use: true,
        if_new_ver: true,
        if_new_slot: true,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
}

/// A `Reinstall` with USE churn enables on `if_new_use`, one with
/// a slot change on `if_new_ver`; a churn-free one never does.
#[test]
fn complete_graph_auto_enable_reinstall_needs_churn() {
    let use_churn = |changed: Vec<String>| {
        entry_162(
            "dev-libs",
            "re",
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: changed,
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
            Some("0"),
            false,
        )
    };
    assert!(complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &[use_churn(vec!["flip".to_string()])],
        if_new_use: true,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &[use_churn(vec!["flip".to_string()])],
        if_new_use: false,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
    assert!(!complete_graph_auto_enable(CompleteGraphAutoEnableArgs {
        entries: &[use_churn(Vec::new())],
        if_new_use: true,
        if_new_ver: false,
        if_new_slot: false,
        root: std::path::Path::new(""),
        repos: &[],
        config: &portage_profile::Config::default()
    }));
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

/// One queued-but-unresolved atom for the drain-state legs
/// (`atoms_all_in_graph`'s `queued_cps` shortcut).
fn qatom_162(atom: &str) -> QueueItem {
    QueueItem {
        atom: atom.to_string(),
        depth: 0,
        owner: None,
        owner_targets_running_root: false,
        unevaluated: None,
        targets_running_root: false,
        buildtime_hard: false,
        from_disjunction: false,
    }
}

fn self_162(cat: &str, pkg: &str) -> RootCp {
    (false, cat.to_string(), pkg.to_string())
}

// ---- `disjunction_preference` (Portage `dep_zapdeps` choice bins). ----
/// Arguments of [`disj_162`].
struct Disj162Args<'a> {
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    self_cp: &'a RootCp,
    atoms: &'a [&'a str],
    queued: &'a [QueueItem],
}

/// Arguments of [`disj_162_circ`].
struct Disj162CircArgs<'a> {
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    self_cp: &'a RootCp,
    atoms: &'a [&'a str],
    queued: &'a [QueueItem],
    circular_dependency: &'a HashMap<RootCp, Vec<CircularDepChild>>,
}

/// `disj_162` with an explicit `circular_dependency` map: owner cp
/// to circular-child records, like the backtrack loop carries.
fn disj_162_circ(args: Disj162CircArgs<'_>) -> portage_use_reduce::AltPreference {
    let Disj162CircArgs {
        repos,
        config,
        root,
        entries,
        self_cp,
        atoms,
        queued,
        circular_dependency,
    } = args;
    disjunction_preference(DisjunctionPreferenceArgs {
        repos,
        config,
        root,
        entries,
        self_cp,
        constraints: &HashMap::new(),
        circular_dependency,
        root_deps_running_root: None,
        atoms: &atoms_162(atoms),
        queued,
        update: false,
        gate_entries: entries,
        gate_queued: queued,
    })
}

// ---- S5b: `disjunction_preference`'s `circular_atom` demotion. ---- (Portage `dep_zapdeps`) /// Arguments of [`disj_circ_162`].
struct DisjCirc162Args<'a> {
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    self_cp: &'a RootCp,
    atoms: &'a [&'a str],
    queued: &'a [QueueItem],
    circular: &'a HashMap<RootCp, Vec<CircularDepChild>>,
}

fn disj_circ_162(args: DisjCirc162Args<'_>) -> portage_use_reduce::AltPreference {
    let DisjCirc162Args {
        repos,
        config,
        root,
        entries,
        self_cp,
        atoms,
        queued,
        circular,
    } = args;
    disjunction_preference(DisjunctionPreferenceArgs {
        repos,
        config,
        root,
        entries,
        self_cp,
        constraints: &HashMap::new(),
        circular_dependency: circular,
        root_deps_running_root: None,
        atoms: &atoms_162(atoms),
        queued,
        update: false,
        gate_entries: entries,
        gate_queued: queued,
    })
}

fn circ_map_162(
    parent: &RootCp,
    children: Vec<CircularDepChild>,
) -> HashMap<RootCp, Vec<CircularDepChild>> {
    let mut map = HashMap::new();
    map.insert(parent.clone(), children);
    map
}
/// Arguments of [`promote_162`].
struct Promote162Args<'a> {
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    alts: &'a [&'a [&'a str]],
    queued: &'a [QueueItem],
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
