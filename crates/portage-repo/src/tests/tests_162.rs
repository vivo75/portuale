/// Direct unit tests for the eight pure selection predicates: each
/// test observes the predicate's own return value.
use crate::*;
use portage_util::TempDir;

// ---- `bare_cp` (Portage `update.py parse_updates`). ----

/// `bare_cp` keeps the `(category, package)` of a plain `cat/pkg`.
#[test]
fn bare_cp_accepts_a_plain_cat_pkg() {
    assert_eq!(
        bare_cp("dev-libs/foo"),
        Some(("dev-libs".to_string(), "foo".to_string()))
    );
    assert_eq!(
        bare_cp("sys-apps/portage"),
        Some(("sys-apps".to_string(), "portage".to_string()))
    );
}

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

// ---- `clean_selection` (Portage `emerge --prune --nodeps` selection). ----

/// Scratch vdb for the `clean_selection` legs: `multi` spans two
/// versions in one slot, `single` has one (skipped), `spread`
/// spans two versions in each of two slots.
fn clean_root_162() -> std::path::PathBuf {
    let root = dir_162("clean");
    install_162(&root, "dev-libs", "multi-1.0", "0", &[]);
    install_162(&root, "dev-libs", "multi-2.0", "0", &[]);
    install_162(&root, "dev-libs", "single-1.0", "0", &[]);
    install_162(&root, "dev-libs", "spread-1.0", "0", &[]);
    install_162(&root, "dev-libs", "spread-2.0", "0", &[]);
    install_162(&root, "dev-libs", "spread-3.0", "1", &[]);
    install_162(&root, "dev-libs", "spread-4.0", "1", &[]);
    root
}

/// Empty args walk every installed cp: `multi` keeps 2.0, `spread`
/// keeps 4.0 across both slots, the single-version cp is skipped.
#[test]
fn clean_selection_reports_every_multi_version_cp() {
    let root = clean_root_162();
    let out = clean_selection(&root, &[]);
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0].category, "dev-libs");
    assert_eq!(out[0].package, "multi");
    assert_eq!(out[0].best_version, "2.0");
    assert_eq!(out[0].other_versions, vec!["1.0".to_string()]);
    assert_eq!(out[1].package, "spread");
    assert_eq!(out[1].best_version, "4.0");
    assert_eq!(
        out[1].other_versions,
        vec!["1.0".to_string(), "3.0".to_string()]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A version-range atom restricts the best/rest split to its own
/// match set, and a cp mismatch filters everything out.
#[test]
fn clean_selection_version_pinned_atom_selects_only_its_set() {
    let root = clean_root_162();
    let out = clean_selection(&root, &atoms_162(&[">=dev-libs/multi-1.0"]));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].best_version, "2.0");
    assert_eq!(out[0].other_versions, vec!["1.0".to_string()]);
    let out = clean_selection(&root, &atoms_162(&["dev-libs/nonexistent"]));
    assert!(out.is_empty(), "{out:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// An atom matching nothing installed selects nothing.
#[test]
fn clean_selection_impossible_version_matches_nothing() {
    let root = clean_root_162();
    let out = clean_selection(&root, &atoms_162(&["=dev-libs/multi-3.0"]));
    assert!(out.is_empty(), "{out:?}");
    let _ = std::fs::remove_dir_all(&root);
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
        eapi: "8".to_string(),
        binary_deps: HashMap::new(),
    }
}

// ---- `check_if_latest_atom_form` (the autounmask atom
// wording: `>=cpv` when nothing is higher, `>=cpv:slot` when only
// other slots are, `=cpv` when the same slot has more). ----

/// Nothing higher anywhere renders the bare `>=cpv`.
#[test]
fn check_if_latest_atom_form_renders_gte_when_nothing_is_higher() {
    let config = cfg_162();
    let resolved = cand_162("2.0", &["amd64"], "0");
    let all = vec![
        cand_162("1.0", &["amd64"], "0"),
        cand_162("2.0", &["amd64"], "0"),
    ];
    assert_eq!(
        check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
            resolved: &resolved,
            all_candidates: &all,
            category: "test",
            package: "pkg",
            config: &config,
            check_visibility: false
        }),
        ">=test/pkg-2.0"
    );
}

/// A higher version in another slot only renders `>=cpv:slot`.
#[test]
fn check_if_latest_atom_form_renders_slot_when_only_other_slots_are_higher() {
    let config = cfg_162();
    let resolved = cand_162("2.0", &["amd64"], "0");
    let all = vec![
        cand_162("2.0", &["amd64"], "0"),
        cand_162("3.0", &["amd64"], "1"),
    ];
    assert_eq!(
        check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
            resolved: &resolved,
            all_candidates: &all,
            category: "test",
            package: "pkg",
            config: &config,
            check_visibility: false
        }),
        ">=test/pkg-2.0:0"
    );
}

/// A higher version in the same slot renders `=cpv`.
#[test]
fn check_if_latest_atom_form_renders_equals_when_its_slot_is_higher() {
    let config = cfg_162();
    let resolved = cand_162("2.0", &["amd64"], "0");
    let all = vec![
        cand_162("2.0", &["amd64"], "0"),
        cand_162("2.5", &["amd64"], "0"),
    ];
    assert_eq!(
        check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
            resolved: &resolved,
            all_candidates: &all,
            category: "test",
            package: "pkg",
            config: &config,
            check_visibility: false
        }),
        "=test/pkg-2.0"
    );
}

/// With visibility checking on, a keyword-masked higher candidate
/// does not count.
#[test]
fn check_if_latest_atom_form_ignores_a_masked_higher_candidate() {
    let config = cfg_162();
    let resolved = cand_162("2.0", &["amd64"], "0");
    let all = vec![
        cand_162("2.0", &["amd64"], "0"),
        cand_162("3.0", &["~amd64"], "1"),
    ];
    assert_eq!(
        check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
            resolved: &resolved,
            all_candidates: &all,
            category: "test",
            package: "pkg",
            config: &config,
            check_visibility: true
        }),
        ">=test/pkg-2.0"
    );
}

/// With visibility checking off, that same masked candidate counts
/// again.
#[test]
fn check_if_latest_atom_form_counts_a_masked_candidate_without_visibility() {
    let config = cfg_162();
    let resolved = cand_162("2.0", &["amd64"], "0");
    let all = vec![
        cand_162("2.0", &["amd64"], "0"),
        cand_162("3.0", &["~amd64"], "1"),
    ];
    assert_eq!(
        check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
            resolved: &resolved,
            all_candidates: &all,
            category: "test",
            package: "pkg",
            config: &config,
            check_visibility: false
        }),
        ">=test/pkg-2.0:0"
    );
}

/// Single scratch repo in the `blocker_161` shape (name
/// `testrepo`, priority 0, main) holding `(cp, version, slot, iuse)`
/// ebuilds with `KEYWORDS="amd64"`.
fn repo_pkgs_162(dir: &Path, pkgs: &[(&str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse) in pkgs {
        write_pkg_162(&repo, cp, pv, slot, iuse);
    }
    vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo,
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }]
}

/// One scratch ebuild plus its md5-cache entry, in the
/// `blocker_161_write_pkg_full` shape with an `IUSE` line.
fn write_pkg_162(repo: &Path, cp: &str, pv: &str, slot: &str, iuse: &str) {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = format!(
        "EAPI=8\nDESCRIPTION=\"162 selection predicates\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n"
    );
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=162 selection predicates\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
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

fn disj_162(args: Disj162Args<'_>) -> portage_use_reduce::AltPreference {
    let Disj162Args {
        repos,
        config,
        root,
        entries,
        self_cp,
        atoms,
        queued,
    } = args;
    disj_162_circ(Disj162CircArgs {
        repos,
        config,
        root,
        entries,
        self_cp,
        atoms,
        queued,
        circular_dependency: &HashMap::new(),
    })
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

/// A resolvable but uninstalled alternative ranks `Available`.
#[test]
fn disjunction_preference_ranks_a_fresh_alternative_available() {
    let dir = dir_162("disj-avail");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/pkgA"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Available
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An installed alternative ranks `Installed`.
#[test]
fn disjunction_preference_ranks_an_installed_alternative_installed() {
    let dir = dir_162("disj-inst");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    install_162(&dir, "test", "pkgA-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/pkgA"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An alternative on the resolving package itself, not installed and
/// matching nothing in-graph, is circular and ranks `Unsatisfiable`.
/// Portage: pass-1 preferred_in_graph pick
#[test]
fn disjunction_preference_rejects_a_circular_self_alternative() {
    let dir = dir_162("disj-circ");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "pkgA"),
            atoms: &["test/pkgA"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Unsatisfiable
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same self alternative with the package installed is no
/// circle and ranks `Installed`.
#[test]
fn disjunction_preference_keeps_an_installed_self_alternative() {
    let dir = dir_162("disj-circ-inst");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    install_162(&dir, "test", "pkgA-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "pkgA"),
            atoms: &["test/pkgA"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same self alternative with the package already merge-bound
/// in-graph is no circle and ranks `Installed`.
/// Portage: pass-1 preferred_in_graph pick
#[test]
fn disjunction_preference_keeps_an_in_graph_self_alternative() {
    let dir = dir_162("disj-circ-graph");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgA",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &self_162("test", "pkgA"),
            atoms: &[">=test/pkgA-1.0"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `:slot` atom naming a different slot than the graphed instance
/// is NOT in-graph: the older-slot branch ranks `Available`.
/// Portage: graph_db.match_pkgs checks the slot
#[test]
fn disjunction_preference_cross_slot_atom_is_not_in_graph() {
    let dir = dir_162("disj-crossslot");
    let repos = repo_pkgs_162(
        &dir,
        &[("test/pkgB", "1.0", "0", ""), ("test/pkgB", "2.0", "1", "")],
    );
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgB",
        PretendOutcome::New {
            version: "2.0".to_string(),
        },
        Some("1"),
        false,
    )];
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/pkgB:0"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Available
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same-slot `:slot` atom still matches the graphed instance --
/// the g221c precision must not over-narrow to `Available`.
#[test]
fn disjunction_preference_same_slot_atom_stays_in_graph() {
    let dir = dir_162("disj-sameslot");
    let repos = repo_pkgs_162(
        &dir,
        &[("test/pkgB", "1.0", "0", ""), ("test/pkgB", "2.0", "1", "")],
    );
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgB",
        PretendOutcome::New {
            version: "2.0".to_string(),
        },
        Some("1"),
        false,
    )];
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/pkgB:1"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The circular-self gate still lets a cross-slot self atom through:
/// the gate asks cp-presence, not instance identity (Portage merges
/// the second instance as an ordinary dependency).
#[test]
fn disjunction_preference_cross_slot_self_atom_still_proceeds() {
    let dir = dir_162("disj-crossself");
    let repos = repo_pkgs_162(
        &dir,
        &[("test/pkgB", "1.0", "0", ""), ("test/pkgB", "2.0", "1", "")],
    );
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgB",
        PretendOutcome::New {
            version: "2.0".to_string(),
        },
        Some("1"),
        false,
    )];
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &self_162("test", "pkgB"),
            atoms: &["test/pkgB:0"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Available
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// With the self edge recorded in the `circular_dependency` map, the
/// in-graph self alternative demotes to `Other`.
/// Portage: dep_check.py, dep_zapdeps circular_atom
#[test]
fn disjunction_preference_demotes_a_recorded_circular_self_alternative() {
    let dir = dir_162("disj-circ-demote");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgA",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    let mut circular: HashMap<RootCp, Vec<CircularDepChild>> = HashMap::new();
    circular.insert(
        self_162("test", "pkgA"),
        vec![CircularDepChild {
            category: "test".to_string(),
            package: "pkgA".to_string(),
            version: "1.0".to_string(),
            slot: Some("0".to_string()),
            sub_slot: None,
        }],
    );
    assert_eq!(
        disj_162_circ(Disj162CircArgs {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &self_162("test", "pkgA"),
            atoms: &[">=test/pkgA-1.0"],
            queued: &[],
            circular_dependency: &circular
        }),
        portage_use_reduce::AltPreference::Other
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The demotion spares an atom an installed instance satisfies (a
/// "cycle" through an installed package is no deadlock).
/// Portage: vardb.match
#[test]
fn disjunction_preference_circular_demotion_spares_an_installed_atom() {
    let dir = dir_162("disj-circ-exempt");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    install_162(&dir, "test", "pkgA-1.0", "0", &[]);
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgA",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    let mut circular: HashMap<RootCp, Vec<CircularDepChild>> = HashMap::new();
    circular.insert(
        self_162("test", "pkgA"),
        vec![CircularDepChild {
            category: "test".to_string(),
            package: "pkgA".to_string(),
            version: "1.0".to_string(),
            slot: Some("0".to_string()),
            sub_slot: None,
        }],
    );
    assert_eq!(
        disj_162_circ(Disj162CircArgs {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &self_162("test", "pkgA"),
            atoms: &["test/pkgA"],
            queued: &[],
            circular_dependency: &circular
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An uninstalled alternative on another package is not circular.
#[test]
fn disjunction_preference_keeps_an_uninstalled_other_alternative() {
    let dir = dir_162("disj-other");
    let repos = repo_pkgs_162(&dir, &[("test/pkgB", "1.0", "0", "")]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "pkgA"),
            atoms: &["test/pkgB"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Available
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A queued-but-unresolved atom counts as in-graph, promoting the
/// alternative to `Installed`.
#[test]
fn disjunction_preference_counts_a_queued_atom_as_in_graph() {
    let dir = dir_162("disj-queued");
    let repos = repo_pkgs_162(&dir, &[("test/pkgA", "1.0", "0", "")]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/pkgA"],
            queued: &[qatom_162("test/pkgA")]
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An alternative whose USE-dep the tree cannot satisfy (and no
/// installed instance helps) ranks `UnsatUseNonInstalled`.
#[test]
fn disjunction_preference_ranks_a_use_unsatisfiable_alternative() {
    let dir = dir_162("disj-unsat");
    let repos = repo_pkgs_162(&dir, &[("test/useflag", "1.0", "0", "flip")]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/useflag[flip]"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::UnsatUseNonInstalled
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same alternative with the violated flag in `use.mask`
/// demotes to plain `Other` (Portage soft 705).
#[test]
fn disjunction_preference_demotes_a_masked_use_violation_to_other() {
    let dir = dir_162("disj-masked");
    let repos = repo_pkgs_162(&dir, &[("test/useflag", "1.0", "0", "flip")]);
    let mut config = cfg_162();
    config.use_mask_force_levels = vec![portage_profile::UseMaskForceLevel {
        use_mask: vec!["flip".to_string()],
        use_force: Vec::new(),
        ..Default::default()
    }];
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/useflag[flip]"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Other
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An alternative satisfied by an installed instance's recorded vdb
/// USE keeps bin 0 even when the profile leaves the flag off.
/// Portage: _dep_check_composite_db / dbapi._match_use
#[test]
fn disjunction_preference_matches_an_installed_alternatives_vdb_use_direct() {
    let dir = dir_162("disj-vdbuse");
    let repos = repo_pkgs_162(&dir, &[("test/vdbuse", "1.0", "0", "flip")]);
    install_162(
        &dir,
        "test",
        "vdbuse-1.0",
        "0",
        &[("USE", b"flip\n"), ("IUSE", b"flip\n")],
    );
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/vdbuse[flip]"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An alternative satisfiable only in an uninstalled slot ranks
/// `UnsatUseNonInstalled`, not `UnsatUseInstalled` (Portage soft
/// 599-607).
#[test]
fn disjunction_preference_needs_the_target_slot_installed() {
    let dir = dir_162("disj-slot");
    let repos = repo_pkgs_162(
        &dir,
        &[
            ("test/slotpkg", "1.0", "0", "flip"),
            ("test/slotpkg", "2.0", "1", "flip"),
        ],
    );
    install_162(&dir, "test", "slotpkg-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/slotpkg:1[flip]"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::UnsatUseNonInstalled
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// No tree candidate but an installed cp behind a blocker-shaped
/// group ranks `Other`: blockers stay out of the installed fold.
/// Portage: dep_check.py (if not atom.blocker, soft 715-724)
#[test]
fn disjunction_preference_ranks_an_unavailable_group_other() {
    let dir = dir_162("disj-other2");
    let repos = repo_pkgs_162(&dir, &[("test/bystander", "1.0", "0", "")]);
    install_162(&dir, "test", "victim-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["!test/victim", "test/nonexistent"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::Other
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// No tree candidate but a fully matching installed instance
/// ranks `OtherInstalled`.
#[test]
fn disjunction_preference_ranks_a_fully_installed_group_other_installed() {
    let dir = dir_162("disj-oinst");
    let repos = repo_pkgs_162(&dir, &[("test/bystander", "1.0", "0", "")]);
    install_162(&dir, "test", "victim-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        disj_162(Disj162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &self_162("test", "consumer"),
            atoms: &["test/victim"],
            queued: &[]
        }),
        portage_use_reduce::AltPreference::OtherInstalled
    );
    let _ = std::fs::remove_dir_all(&dir);
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

fn circ_child_162(cat: &str, pkg: &str, ver: &str, slot: &str) -> CircularDepChild {
    CircularDepChild {
        category: cat.to_string(),
        package: pkg.to_string(),
        version: ver.to_string(),
        slot: Some(slot.to_string()),
        sub_slot: Some(slot.to_string()),
    }
}

fn circ_map_162(
    parent: &RootCp,
    children: Vec<CircularDepChild>,
) -> HashMap<RootCp, Vec<CircularDepChild>> {
    let mut map = HashMap::new();
    map.insert(parent.clone(), children);
    map
}

/// An alternative matching the parent's recorded in-cycle child
/// ranks `Other` (the bug-703440 `|| ( bootstrap cmake )` re-pick).
#[test]
fn disjunction_preference_demotes_a_recorded_circular_child_to_other() {
    let dir = dir_162("disj-circ-hit");
    let repos = repo_pkgs_162(&dir, &[("test/pkgB", "1.0", "0", "")]);
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "pkgB",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    let parent = self_162("test", "consumer");
    assert_eq!(
        disj_circ_162(DisjCirc162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &parent,
            atoms: &["test/pkgB"],
            queued: &[],
            circular: &HashMap::new()
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let recorded = circ_map_162(&parent, vec![circ_child_162("test", "pkgB", "1.0", "0")]);
    assert_eq!(
        disj_circ_162(DisjCirc162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            self_cp: &parent,
            atoms: &["test/pkgB"],
            queued: &[],
            circular: &recorded
        }),
        portage_use_reduce::AltPreference::Other
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A recorded child for another parent, or for a version the atom
/// does not match, leaves the rank alone.
#[test]
fn disjunction_preference_ignores_a_non_matching_circular_record() {
    let dir = dir_162("disj-circ-miss");
    let repos = repo_pkgs_162(
        &dir,
        &[("test/pkgB", "1.0", "0", ""), ("test/pkgB", "2.0", "1", "")],
    );
    let config = cfg_162();
    let parent = self_162("test", "consumer");
    let other_parent = self_162("test", "stranger");
    let wrong_parent = circ_map_162(
        &other_parent,
        vec![circ_child_162("test", "pkgB", "1.0", "0")],
    );
    assert_eq!(
        disj_circ_162(DisjCirc162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &parent,
            atoms: &["test/pkgB:0"],
            queued: &[],
            circular: &wrong_parent
        }),
        portage_use_reduce::AltPreference::Available
    );
    let wrong_slot = circ_map_162(&parent, vec![circ_child_162("test", "pkgB", "2.0", "1")]);
    assert_eq!(
        disj_circ_162(DisjCirc162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &parent,
            atoms: &["test/pkgB:0"],
            queued: &[],
            circular: &wrong_slot
        }),
        portage_use_reduce::AltPreference::Available
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An atom satisfied by an installed version is not a circular dep:
/// the exemption holds `Installed` even with a matching record.
/// Portage: vardb.match
#[test]
fn disjunction_preference_exempts_an_installed_satisfying_circular_atom() {
    let dir = dir_162("disj-circ-inst");
    let repos = repo_pkgs_162(&dir, &[("test/pkgB", "1.0", "0", "")]);
    install_162(&dir, "test", "pkgB-1.0", "0", &[]);
    let config = cfg_162();
    let parent = self_162("test", "consumer");
    let recorded = circ_map_162(&parent, vec![circ_child_162("test", "pkgB", "1.0", "0")]);
    assert_eq!(
        disj_circ_162(DisjCirc162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            self_cp: &parent,
            atoms: &["test/pkgB"],
            queued: &[],
            circular: &recorded
        }),
        portage_use_reduce::AltPreference::Installed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- `promote_tied_alternative` (Portage `dep_zapdeps` upgrade pass). ----

/// Shared tree for the promotion legs: `up` spans two versions,
/// `plain` spans two, `plainb` has one, `slota` spans two slots.
fn promote_repo_162(dir: &Path) -> Vec<RepoConfig> {
    repo_pkgs_162(
        dir,
        &[
            ("test/up", "1.0", "0", ""),
            ("test/up", "2.0", "0", ""),
            ("test/plain", "1.0", "0", ""),
            ("test/plain", "2.0", "0", ""),
            ("test/plainb", "1.0", "0", ""),
            ("test/slota", "1.0", "0", ""),
            ("test/slota", "2.0", "1", ""),
        ],
    )
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

fn promote_162(args: Promote162Args<'_>) -> usize {
    let Promote162Args {
        repos,
        config,
        root,
        entries,
        alts,
        queued,
    } = args;
    let owned: Vec<Vec<String>> = alts
        .iter()
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .collect();
    promote_tied_alternative(PromoteTiedAlternativeArgs {
        repos,
        config,
        root,
        entries,
        constraints: &HashMap::new(),
        alts: &owned,
        queued,
    })
}

/// A strict upgrade on the shared cp promotes the newer alternative.
#[test]
fn promote_tied_alternative_prefers_the_strict_upgrade() {
    let dir = dir_162("prom-up");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["=test/up-1.0"], &["test/up"]],
            queued: &[]
        }),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Disjoint cps never promote.
#[test]
fn promote_tied_alternative_keeps_disjoint_order() {
    let dir = dir_162("prom-disj");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["test/up"], &["test/plain"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Three alternatives promote the newest past both older ones.
#[test]
fn promote_tied_alternative_promotes_through_a_longer_list() {
    let dir = dir_162("prom-long");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["=test/up-1.0"], &["test/plain"], &["test/up"]],
            queued: &[]
        }),
        2
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An upgrade on one shared cp plus a downgrade on another is no
/// promotion.
#[test]
fn promote_tied_alternative_rejects_a_mixed_upgrade_and_downgrade() {
    let dir = dir_162("prom-mixed");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[
                &["=test/plain-1.0", "test/up"],
                &["test/plain", "=test/up-1.0"]
            ],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// With equal versions the in-graph alternative promotes.
#[test]
fn promote_tied_alternative_prefers_the_in_graph_alternative() {
    let dir = dir_162("prom-graph");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "plainb",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            alts: &[&["test/plain"], &["test/plainb"]],
            queued: &[]
        }),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// With both alternatives in-graph nothing promotes.
#[test]
fn promote_tied_alternative_keeps_order_when_both_are_in_graph() {
    let dir = dir_162("prom-bothgraph");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    let entries = vec![
        entry_162(
            "test",
            "plain",
            PretendOutcome::New {
                version: "2.0".to_string(),
            },
            Some("0"),
            false,
        ),
        entry_162(
            "test",
            "plainb",
            PretendOutcome::New {
                version: "1.0".to_string(),
            },
            Some("0"),
            false,
        ),
    ];
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            alts: &[&["test/plain"], &["test/plainb"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A downgrade-only alternative stays put even when it is the one
/// in-graph.
#[test]
fn promote_tied_alternative_rejects_an_in_graph_downgrade() {
    let dir = dir_162("prom-downgraph");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    let entries = vec![entry_162(
        "test",
        "up",
        PretendOutcome::New {
            version: "1.0".to_string(),
        },
        Some("0"),
        false,
    )];
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &entries,
            alts: &[&["test/up", "test/plain"], &["=test/up-1.0"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An alternative installed in every needed slot promotes over
/// one that is not.
#[test]
fn promote_tied_alternative_prefers_the_installed_slots_alternative() {
    let dir = dir_162("prom-slots");
    let repos = promote_repo_162(&dir);
    install_162(&dir, "test", "slota-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["test/plain"], &["=test/slota-1.0"]],
            queued: &[]
        }),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An uninstalled alternative does not take the installed-slots
/// promotion even against an installed one.
#[test]
fn promote_tied_alternative_rejects_an_uninstalled_alternative() {
    let dir = dir_162("prom-noslots");
    let repos = promote_repo_162(&dir);
    install_162(&dir, "test", "slota-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["=test/slota-1.0"], &["test/plain"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Cp-installed but in the wrong slot is not `all_installed_slots`
/// (Portage soft 599-607).
#[test]
fn promote_tied_alternative_needs_the_candidate_slot_installed() {
    let dir = dir_162("prom-wrongslot");
    let repos = promote_repo_162(&dir);
    install_162(&dir, "test", "slota-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["test/plain"], &["test/slota"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Blockers never enter a `cp_map`: the blocker alternative keeps
/// no shared cp and loses.
#[test]
fn promote_tied_alternative_ignores_blockers_in_cp_maps() {
    let dir = dir_162("prom-blocker");
    let repos = promote_repo_162(&dir);
    install_162(&dir, "test", "up-1.0", "0", &[]);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["=test/up-1.0"], &["!test/up"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A single alternative always wins index 0.
#[test]
fn promote_tied_alternative_returns_zero_for_a_single_alternative() {
    let dir = dir_162("prom-single");
    let repos = promote_repo_162(&dir);
    let config = cfg_162();
    assert_eq!(
        promote_162(Promote162Args {
            repos: &repos,
            config: &config,
            root: &dir,
            entries: &[],
            alts: &[&["test/up"]],
            queued: &[]
        }),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- `alternative_downgrade_demoted` (Portage bug-531656 downgrade). ----

/// Shared tree for the demotion legs: `pkg` spans two versions in
/// slot 0.
fn demote_repo_162(dir: &Path) -> Vec<RepoConfig> {
    repo_pkgs_162(
        dir,
        &[("test/pkg", "1.0", "0", ""), ("test/pkg", "2.0", "0", "")],
    )
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

fn demote_162(args: Demote162Args<'_>) -> bool {
    let Demote162Args {
        repos,
        config,
        root,
        entries,
        self_cp,
        atoms,
        queued,
        update,
    } = args;
    alternative_downgrade_demoted(AlternativeDowngradeDemotedArgs {
        repos,
        root,
        config,
        entries,
        self_cp,
        constraints: &HashMap::new(),
        atoms: &atoms_162(atoms),
        queued,
        update,
    })
}

fn demote_entries_162() -> Vec<GraphEntry> {
    vec![entry_162(
        "test",
        "pkg",
        PretendOutcome::New {
            version: "2.0".to_string(),
        },
        Some("0"),
        false,
    )]
}

/// An alternative resolving below the graphed version demotes.
#[test]
fn alternative_downgrade_demoted_flags_an_older_alternative() {
    let dir = dir_162("demote-older");
    let repos = demote_repo_162(&dir);
    let config = cfg_162();
    assert!(demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &demote_entries_162(),
        self_cp: &self_162("test", "consumer"),
        atoms: &["=test/pkg-1.0"],
        queued: &[],
        update: false
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An alternative at the graphed version does not demote.
#[test]
fn alternative_downgrade_demoted_keeps_the_graphed_version() {
    let dir = dir_162("demote-same");
    let repos = demote_repo_162(&dir);
    let config = cfg_162();
    let mut entries = demote_entries_162();
    entries.push(entry_162(
        "test",
        "pkg",
        PretendOutcome::New {
            version: "9.9".to_string(),
        },
        Some("9"),
        false,
    ));
    assert!(!demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &entries,
        self_cp: &self_162("test", "consumer"),
        atoms: &["test/pkg"],
        queued: &[],
        update: false
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The resolving package's own alternative never demotes.
#[test]
fn alternative_downgrade_demoted_skips_the_resolving_package() {
    let dir = dir_162("demote-self");
    let repos = demote_repo_162(&dir);
    let config = cfg_162();
    assert!(!demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &demote_entries_162(),
        self_cp: &self_162("test", "pkg"),
        atoms: &["=test/pkg-1.0"],
        queued: &[],
        update: false
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A blocker alternative never demotes.
#[test]
fn alternative_downgrade_demoted_skips_blockers() {
    let dir = dir_162("demote-blocker");
    let repos = demote_repo_162(&dir);
    let config = cfg_162();
    assert!(!demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &demote_entries_162(),
        self_cp: &self_162("test", "consumer"),
        atoms: &["!<=test/pkg-1.0"],
        queued: &[],
        update: false
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// With an empty graph a queued same-cp atom stands in for the
/// not-yet-graphed selection: the installed best is above, so it
/// demotes.
#[test]
fn alternative_downgrade_demoted_uses_the_queued_hypothetical() {
    let dir = dir_162("demote-queued");
    let repos = demote_repo_162(&dir);
    install_162(&dir, "test", "pkg-2.0", "0", &[]);
    let config = cfg_162();
    assert!(demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[],
        self_cp: &self_162("test", "consumer"),
        atoms: &["=test/pkg-1.0"],
        queued: &[qatom_162("test/pkg")],
        update: false
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A queued atom on another cp is not a hypothetical for this one.
#[test]
fn alternative_downgrade_demoted_ignores_other_cp_queue() {
    let dir = dir_162("demote-queue-other");
    let repos = demote_repo_162(&dir);
    install_162(&dir, "test", "other-3.0", "0", &[]);
    let config = cfg_162();
    assert!(!demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[],
        self_cp: &self_162("test", "consumer"),
        atoms: &["=test/pkg-1.0"],
        queued: &[qatom_162("test/other")],
        update: false
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Under `--update` the hypothetical is the tree best, no vdb needed.
#[test]
fn alternative_downgrade_demoted_uses_the_tree_hypothetical_under_update() {
    let dir = dir_162("demote-update");
    let repos = demote_repo_162(&dir);
    let config = cfg_162();
    assert!(demote_162(Demote162Args {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[],
        self_cp: &self_162("test", "consumer"),
        atoms: &["=test/pkg-1.0"],
        queued: &[qatom_162("test/pkg")],
        update: true
    }));
    let _ = std::fs::remove_dir_all(&dir);
}
