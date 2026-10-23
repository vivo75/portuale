/// Direct predicate-result legs in the scratch style of `tests_161`/`tests_162`: each
/// test observes the predicate's own return value. Do not edit other
/// items' `mod tests_16x` blocks.
use crate::*;
use portage_util::TempDir;

// ---- `strip_revision` (`--rebuild-if-new-ver` compares the
// merged version against installed ones with `-r<n>` stripped on
// both sides -- see `rebuild_if_entries`). ----

/// Only a numeric `-r<n>` suffix strips, and the split is at the
/// last `-r`.
#[test]
fn strip_revision_166_strips_numeric_revisions_only() {
    assert_eq!(strip_revision("1.0-r1"), "1.0");
    assert_eq!(strip_revision("1.0"), "1.0");
    // Non-numeric tail: the guard stays shut (guard-`true` would
    // strip to "1.0").
    assert_eq!(strip_revision("1.0-rx"), "1.0-rx");
    assert_eq!(strip_revision("1.0-r1x"), "1.0-r1x");
    // Empty rev: `&&` -> `||` would strip to "1.0" (`all` is
    // vacuously true on the empty rev).
    assert_eq!(strip_revision("1.0-r"), "1.0-r");
    assert_eq!(strip_revision("1.0-r0"), "1.0");
    // Last `-r` wins: only the trailing numeric revision strips.
    assert_eq!(strip_revision("1.0-r1-r2"), "1.0-r1");
}

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
        eapi: "8".to_string(),
        binary_deps: HashMap::new(),
        user_patches: String::new(),
    }
}

/// Exclude-only: the pinned version drops, the rest stays.
#[test]
fn filter_usepkg_166_exclude_drops_only_the_match() {
    let out = filter_usepkg_exclude_include(
        vec![cand_166("1.0"), cand_166("2.0")],
        "dev-libs",
        "foo",
        &["=dev-libs/foo-1.0".to_string()],
        &[],
    );
    let versions: Vec<&str> = out.iter().map(|c| c.version.as_str()).collect();
    assert_eq!(versions, vec!["2.0"]);
}

/// Include-only: a non-matching candidate drops even though nothing
/// excludes it.
#[test]
fn filter_usepkg_166_include_drops_a_non_match() {
    let out = filter_usepkg_exclude_include(
        vec![cand_166("1.0")],
        "dev-libs",
        "foo",
        &[],
        &["dev-libs/bar".to_string()],
    );
    assert!(out.is_empty(), "{out:?}");
    // A matching include keeps the row.
    let out = filter_usepkg_exclude_include(
        vec![cand_166("1.0")],
        "dev-libs",
        "foo",
        &[],
        &["dev-libs/foo".to_string()],
    );
    assert_eq!(out.len(), 1);
}

/// A binary dropped by the filters is a reason-less masked row (backlog #416); the
/// surviving version is not listed, and nothing dropped means no report.
#[test]
fn usepkg_dropped_report_lists_only_the_dropped_matches_without_a_reason() {
    let binaries = vec![cand_166("1.0"), cand_166("2.0")];
    let exclude = vec!["=dev-libs/foo-1.0".to_string()];
    let report = usepkg_dropped_binary_report(
        &binaries,
        "dev-libs/foo",
        "\"dev-libs/foo\"",
        (&exclude, &[]),
        false,
    )
    .expect("a dropped match");
    assert!(
        report.contains("- dev-libs/foo-1.0::testrepo (masked by: )\n"),
        "{report}"
    );
    assert!(!report.contains("foo-2.0"), "{report}");
    assert!(
        usepkg_dropped_binary_report(&binaries, "dev-libs/foo", "x", (&[], &[]), false).is_none()
    );
}

// ---- `rebuilt_binary_changed` (Portage's two rebuilt_binary triggers). ----

/// Fresh unique scratch dir per test (vdb lives under it, so the
/// per-root aux caches never see a reused path).
fn dir_166(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-166-{tag}")).keep()
}

/// One installed instance with the given `BUILD_TIME` (plus `SLOT`,
/// which the vdb scan expects beside every instance).
fn install_166(root: &Path, cat: &str, pf: &str, build_time: &str) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), "0\n").unwrap();
    std::fs::write(d.join("BUILD_TIME"), build_time).unwrap();
}

/// Timestamped (`--rebuilt-binaries-timestamp`) comparison: strictly
/// newer past the floor reports, anything else does not.
#[test]
fn rebuilt_binary_166_timestamped_comparison() {
    let root = dir_166("rebuilt-ts");
    install_166(&root, "dev-libs", "rebuilt-1.0", "100\n");
    let changed = |built: Option<i64>, ts: Option<u64>| {
        rebuilt_binary_changed(RebuiltBinaryChangedArgs {
            root: &root,
            binary_build_time: built,
            category: "dev-libs",
            package: "rebuilt",
            version: "1.0",
            rebuilt_binaries_timestamp: ts,
        })
    };
    // No binary candidate (or no BUILD_TIME): no verdict either way.
    assert!(!changed(None, Some(50)));
    // Strictly newer past the floor: the only `true` shape here.
    assert!(changed(Some(200), Some(50)));
    // Older binary: `false && true` is false, `false || true` is
    // true.
    assert!(!changed(Some(50), Some(10)));
    // Equal stamps: `>` is false, `>=` is true.
    assert!(!changed(Some(100), Some(50)));
    let _ = std::fs::remove_dir_all(&root);
}

/// Untimestamped comparison: any difference reports.
#[test]
fn rebuilt_binary_166_untimestamped_comparison() {
    let root = dir_166("rebuilt-plain");
    install_166(&root, "dev-libs", "rebuilt-1.0", "100\n");
    let changed = |built: Option<i64>| {
        rebuilt_binary_changed(RebuiltBinaryChangedArgs {
            root: &root,
            binary_build_time: built,
            category: "dev-libs",
            package: "rebuilt",
            version: "1.0",
            rebuilt_binaries_timestamp: None,
        })
    };
    assert!(changed(Some(200)));
    assert!(!changed(Some(100)));
    // No installed BUILD_TIME at all: the second `let-else` fires.
    let bare = dir_166("rebuilt-bare");
    std::fs::create_dir_all(bare.join("var/db/pkg/dev-libs/rebuilt-1.0")).unwrap();
    assert!(!rebuilt_binary_changed(RebuiltBinaryChangedArgs {
        root: &bare,
        binary_build_time: Some(200),
        category: "dev-libs",
        package: "rebuilt",
        version: "1.0",
        rebuilt_binaries_timestamp: None
    }));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&bare);
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

/// Arguments of [`arg_input_166`].
struct ArgInput166Args<'a> {
    conflicts: &'a [SlotConflict],
    top_level: &'a HashSet<(String, String)>,
    excluded: &'a [String],
    selective: bool,
    replace_cps: &'a BTreeSet<(String, String)>,
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
}

/// Eight-arg input builder (same shape as `ds_input_161`).
fn arg_input_166<'a>(args: ArgInput166Args<'a>) -> DirectSolveInput<'a> {
    let ArgInput166Args {
        conflicts,
        top_level,
        excluded,
        selective,
        replace_cps,
        root,
        repos,
        config,
    } = args;
    DirectSolveInput {
        conflicts,
        entries: &[],
        top_level,
        excluded,
        selective,
        replace_cps,
        root,
        repos,
        config,
    }
}

fn arg_top_166() -> HashSet<(String, String)> {
    HashSet::from([("dev-libs".to_string(), "argpkg".to_string())])
}

/// The argued shape (installed instance, top-level names the cp, no
/// exclude, not selective) is the only `true`.
#[test]
fn direct_solve_arg_mode_166_argues_the_requested_installed() {
    let root = dir_166("arg-true");
    let repos = Vec::new();
    let config = cfg_166();
    let conflicts = [conflict_166(vec![("2.0", false), ("1.0", true)])];
    let top = arg_top_166();
    let replace = BTreeSet::new();
    assert!(direct_solve_arg_mode(
        &arg_input_166(ArgInput166Args {
            conflicts: &conflicts,
            top_level: &top,
            excluded: &[],
            selective: false,
            replace_cps: &replace,
            root: &root,
            repos: &repos,
            config: &config
        }),
        &conflicts[0]
    ));
    let _ = std::fs::remove_dir_all(&root);
}

/// Every missing precondition reports `false`: no installed
/// instance, selective mode, an unrequested cp, a matching exclude.
#[test]
fn direct_solve_arg_mode_166_rejects_every_non_arg_shape() {
    let root = dir_166("arg-false");
    let repos = Vec::new();
    let config = cfg_166();
    let top = arg_top_166();
    let empty_top: HashSet<(String, String)> = HashSet::new();
    let replace = BTreeSet::new();
    let mk = |conflicts: &[SlotConflict],
              top: &HashSet<(String, String)>,
              excl: &[String],
              sel: bool| {
        direct_solve_arg_mode(
            &arg_input_166(ArgInput166Args {
                conflicts,
                top_level: top,
                excluded: excl,
                selective: sel,
                replace_cps: &replace,
                root: &root,
                repos: &repos,
                config: &config,
            }),
            &conflicts[0],
        )
    };
    // No installed instance: not arguable even when requested.
    let merge_only = [conflict_166(vec![("2.0", false), ("1.0", false)])];
    assert!(!mk(&merge_only, &top, &[], false));
    // Selective mode wants the installed package.
    let with_inst = [conflict_166(vec![("2.0", false), ("1.0", true)])];
    assert!(!mk(&with_inst, &top, &[], true));
    // Nobody asked for this cp.
    assert!(!mk(&with_inst, &empty_top, &[], false));
    // `--exclude` names the installed cpv.
    assert!(!mk(
        &with_inst,
        &top,
        &["dev-libs/argpkg".to_string()],
        false
    ));
    let _ = std::fs::remove_dir_all(&root);
}

// ---- `direct_solve_instance_use` (Portage per-instance USE). ----

/// Arguments of [`write_pkg_166`].
struct WritePkg166Args<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    iuse: &'a str,
    depend: &'a str,
    required_use: &'a str,
}

/// One scratch ebuild plus its md5-cache entry, in the
/// `write_pkg_162` shape extended with `DEPEND`/`REQUIRED_USE` lines.
fn write_pkg_166(args: WritePkg166Args<'_>) {
    let WritePkg166Args {
        repo,
        cp,
        pv,
        slot,
        iuse,
        depend,
        required_use,
    } = args;
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = format!(
        "EAPI=8\nDESCRIPTION=\"166 circular/instance-use\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n"
    );
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    if !depend.is_empty() {
        writeln!(body, "DEPEND=\"{depend}\"").unwrap();
    }
    if !required_use.is_empty() {
        writeln!(body, "REQUIRED_USE=\"{required_use}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=166 circular/instance-use\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    if !depend.is_empty() {
        writeln!(entry, "DEPEND={depend}").unwrap();
    }
    if !required_use.is_empty() {
        writeln!(entry, "REQUIRED_USE={required_use}").unwrap();
    }
    writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

/// Single scratch repo (`testrepo`, priority 0, main) holding
/// `(cp, version, slot, IUSE, DEPEND, REQUIRED_USE)` ebuilds with
/// `KEYWORDS="amd64"`.
fn repo_pkgs_166(dir: &Path, pkgs: &[(&str, &str, &str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse, depend, required_use) in pkgs {
        write_pkg_166(WritePkg166Args {
            repo: &repo,
            cp,
            pv,
            slot,
            iuse,
            depend,
            required_use,
        });
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

/// One installed instance with the given `USE` record.
fn use_install_166(root: &Path, cat: &str, pf: &str, use_flags: &str) {
    let d = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("SLOT"), "0\n").unwrap();
    std::fs::write(d.join("USE"), use_flags).unwrap();
}

/// Merge-bound `GraphEntry` carrying resolved display flags (the
/// shape `direct_solve_instance_use` reads for a merge instance).
fn entry_166(cat: &str, pkg: &str, version: &str, display: Vec<(String, bool)>) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: cat.to_string(),
        package: pkg.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: Some("0".to_string()),
        sub_slot: Some("0".to_string()),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: display,
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
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
        build_time: None,
        deps: Vec::new(),
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

fn iu_input_166<'a>(
    entries: &'a [GraphEntry],
    replace_cps: &'a BTreeSet<(String, String)>,
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
) -> DirectSolveInput<'a> {
    let conflicts: &'a [SlotConflict] = &[];
    let top: &'a HashSet<(String, String)> = &EMPTY_TOP_166;
    DirectSolveInput {
        conflicts,
        entries,
        top_level: top,
        excluded: &[],
        selective: false,
        replace_cps,
        root,
        repos,
        config,
    }
}

/// Empty top-level set for inputs whose verdict never consults it.
static EMPTY_TOP_166: std::sync::LazyLock<HashSet<(String, String)>> =
    std::sync::LazyLock::new(HashSet::new);

/// An installed instance reads vdb `USE` (enabled) over the tree
/// `IUSE` (declared).
#[test]
fn direct_solve_instance_use_166_installed_reads_vdb_over_tree_iuse() {
    let root = dir_166("iu-inst");
    use_install_166(&root, "dev-libs", "iupkg-1.0", "fa\n");
    let dir = dir_166("iu-inst-repo");
    let repos = repo_pkgs_166(&dir, &[("dev-libs/iupkg", "1.0", "0", "fa fb", "", "")]);
    let config = cfg_166();
    let replace = BTreeSet::new();
    let input = iu_input_166(&[], &replace, &root, &repos, &config);
    let (enabled, declared) =
        direct_solve_instance_use(&input, "dev-libs", "iupkg", &inst_166("1.0", true));
    assert_eq!(enabled, HashSet::from(["fa".to_string()]));
    assert_eq!(
        declared,
        HashSet::from(["fa".to_string(), "fb".to_string()])
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&dir);
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

fn synth_owners_166() -> HashMap<(String, String), HashSet<(String, String)>> {
    HashMap::from([(
        ("dev-libs".to_string(), "foo".to_string()),
        HashSet::from([("dev-libs".to_string(), "parent".to_string())]),
    )])
}

/// The surviving instance files directly after its last merge-bound
/// sibling (not at the end, not before).
#[test]
fn synthesize_166_files_the_instance_beside_its_siblings() {
    let root = dir_166("synth-pos");
    let dir = dir_166("synth-pos-repo");
    let repos = repo_pkgs_166(
        &dir,
        &[
            ("dev-libs/foo", "1.0", "0", "fa", "", ""),
            ("dev-libs/foo", "2.0", "0", "fa", "", ""),
        ],
    );
    let config = cfg_166();
    let mut entries = vec![
        entry_166("other-libs", "foo", "1.0", Vec::new()),
        entry_166("dev-libs", "foo", "1.0", Vec::new()),
        entry_166("dev-libs", "decoy", "9.0", Vec::new()),
    ];
    synthesize_surviving_conflict_entries(SynthesizeSurvivingConflictEntriesArgs {
        entries: &mut entries,
        conflicts: &[synth_conflict_166()],
        required_by_map: &synth_owners_166(),
        repos: &repos,
        root: &root,
        config: &config,
    });
    let order: Vec<(&str, &str, &str)> = entries
        .iter()
        .map(|e| {
            (
                e.category.as_str(),
                e.package.as_str(),
                merge_bound_version(&e.outcome)
                    .map(String::as_str)
                    .unwrap_or("?"),
            )
        })
        .collect();
    assert_eq!(
        order,
        vec![
            ("other-libs", "foo", "1.0"),
            ("dev-libs", "foo", "1.0"),
            ("dev-libs", "foo", "2.0"),
            ("dev-libs", "decoy", "9.0"),
        ]
    );
    // The filed entry carries the conflict's owners as parents.
    assert_eq!(
        entries[2].required_by,
        vec![("dev-libs".to_string(), "parent".to_string())]
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `new_slot` reports whether the cp is already installed.
#[test]
fn synthesize_166_new_slot_tracks_the_installed_set() {
    let dir = dir_166("synth-slot-repo");
    let repos = repo_pkgs_166(&dir, &[("dev-libs/foo", "2.0", "0", "fa", "", "")]);
    let config = cfg_166();
    let owners = synth_owners_166();
    let filed_new_slot = |root: &Path| {
        let mut entries = Vec::new();
        synthesize_surviving_conflict_entries(SynthesizeSurvivingConflictEntriesArgs {
            entries: &mut entries,
            conflicts: &[synth_conflict_166()],
            required_by_map: &owners,
            repos: &repos,
            root,
            config: &config,
        });
        assert_eq!(entries.len(), 1);
        entries.pop().expect("filed").new_slot
    };
    let bare = dir_166("synth-slot-bare");
    assert!(!filed_new_slot(&bare));
    let installed = dir_166("synth-slot-inst");
    install_166(&installed, "dev-libs", "foo-1.0", "100\n");
    assert!(filed_new_slot(&installed));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&installed);
}

// ---- `circular_dep_solutions` (Portage `_find_suggestions`). ----

/// The suggestion set rendered order-free (the trailing
/// sort/dedup carries no mutants; the positions, flags and
/// directions below are what the operator flips move).
fn sols_166(sols: &[CircularSuggestion]) -> std::collections::BTreeSet<String> {
    sols.iter()
        .map(|s| {
            let changes: Vec<String> = s
                .changes
                .iter()
                .map(|(f, on)| format!("{f}={on}"))
                .collect();
            format!(
                "{} [{}] followup={}",
                s.parent_cpv,
                changes.join(","),
                s.followup
            )
        })
        .collect()
}

/// Two entangled flags under `|| ( x y )` with both off: only
/// "enable y" removes the atom while REQUIRED_USE holds.
#[test]
fn circular_166_two_flag_or_requirement_suggests_enabling_y() {
    let dir = dir_166("circ-a");
    let repos = repo_pkgs_166(
        &dir,
        &[(
            "dev-libs/apkg",
            "1.0",
            "0",
            "x y",
            "x? ( dev-libs/apkg )",
            "|| ( x y )",
        )],
    );
    let config = cfg_166();
    let sols = circular_dep_solutions(
        &["dev-libs/apkg-1.0".to_string()],
        &repos,
        &config,
        &[],
        &[],
    );
    assert_eq!(
        sols_166(&sols),
        std::collections::BTreeSet::from(["dev-libs/apkg-1.0 [y=true] followup=false".to_string()])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Ten entangled flags with only `f1` on: each "disable f1, enable
/// fi" pair is a minimal solution (nine suggestions).
#[test]
fn circular_166_ten_flags_yield_nine_minimal_pairs() {
    let dir = dir_166("circ-b");
    let iuse = "+f1 f2 f3 f4 f5 f6 f7 f8 f9 f10";
    let ru = "|| ( f1 f2 f3 f4 f5 f6 f7 f8 f9 f10 )";
    let repos = repo_pkgs_166(
        &dir,
        &[(
            "dev-libs/bpkg",
            "1.0",
            "0",
            iuse,
            "f1? ( dev-libs/bpkg )",
            ru,
        )],
    );
    let config = cfg_166();
    let sols = circular_dep_solutions(
        &["dev-libs/bpkg-1.0".to_string()],
        &repos,
        &config,
        &[],
        &[],
    );
    let mut expected = std::collections::BTreeSet::new();
    for f in ["f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10"] {
        expected.insert(format!(
            "dev-libs/bpkg-1.0 [f1=false,{f}=true] followup=false"
        ));
    }
    assert_eq!(sols_166(&sols), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

/// No REQUIRED_USE at all with `x` on: "disable x" is the only
/// solution.
#[test]
fn circular_166_plain_gated_edge_suggests_disabling_x() {
    let dir = dir_166("circ-c");
    let repos = repo_pkgs_166(
        &dir,
        &[(
            "dev-libs/cpkg",
            "1.0",
            "0",
            "+x",
            "x? ( dev-libs/cpkg )",
            "",
        )],
    );
    let config = cfg_166();
    let sols = circular_dep_solutions(
        &["dev-libs/cpkg-1.0".to_string()],
        &repos,
        &config,
        &[],
        &[],
    );
    assert_eq!(
        sols_166(&sols),
        std::collections::BTreeSet::from(
            ["dev-libs/cpkg-1.0 [x=false] followup=false".to_string()]
        )
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Eleven gated flags with one disabled: the first MAX gate keeps
/// the ten enabled ones and the second lets exactly ten through, so
/// the only solution disables all ten.
#[test]
fn circular_166_eleven_flags_retain_to_ten_and_proceed() {
    let dir = dir_166("circ-e");
    let flags: Vec<String> = (1..=11).map(|i| format!("g{i}")).collect();
    let depend = flags
        .iter()
        .map(|f| format!("{f}? ( dev-libs/hpkg )"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut iuse = flags
        .iter()
        .map(|f| format!("+{f}"))
        .collect::<Vec<_>>()
        .join(" ");
    // g11 off: the retain drops exactly one.
    iuse = iuse.replacen("+g11", "g11", 1);
    let repos = repo_pkgs_166(&dir, &[("dev-libs/hpkg", "1.0", "0", &iuse, &depend, "")]);
    let config = cfg_166();
    let sols = circular_dep_solutions(
        &["dev-libs/hpkg-1.0".to_string()],
        &repos,
        &config,
        &[],
        &[],
    );
    assert_eq!(
        sols_166(&sols),
        std::collections::BTreeSet::from(["dev-libs/hpkg-1.0 [g1=false,g10=false,g2=false,g3=false,g4=false,g5=false,g6=false,g7=false,g8=false,g9=false] followup=false".to_string()])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Eleven gated flags with two disabled: the retain drops to nine
/// and the second gate lets fewer-than-ten through, so the only
/// solution disables all nine.
#[test]
fn circular_166_eleven_flags_retain_to_nine_and_proceed() {
    let dir = dir_166("circ-f");
    let flags: Vec<String> = (1..=11).map(|i| format!("g{i}")).collect();
    let depend = flags
        .iter()
        .map(|f| format!("{f}? ( dev-libs/hpkg )"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut iuse = flags
        .iter()
        .map(|f| format!("+{f}"))
        .collect::<Vec<_>>()
        .join(" ");
    // g10 and g11 off: the retain drops exactly two.
    iuse = iuse.replacen("+g10", "g10", 1);
    iuse = iuse.replacen("+g11", "g11", 1);
    let repos = repo_pkgs_166(&dir, &[("dev-libs/hpkg", "1.0", "0", &iuse, &depend, "")]);
    let config = cfg_166();
    let sols = circular_dep_solutions(
        &["dev-libs/hpkg-1.0".to_string()],
        &repos,
        &config,
        &[],
        &[],
    );
    assert_eq!(
        sols_166(&sols),
        std::collections::BTreeSet::from(["dev-libs/hpkg-1.0 [g1=false,g2=false,g3=false,g4=false,g5=false,g6=false,g7=false,g8=false,g9=false] followup=false".to_string()])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A three-ring with one gated edge per position: every position
/// reports its own parent.
#[test]
fn circular_166_three_ring_reports_each_parent() {
    let dir = dir_166("circ-d");
    let repos = repo_pkgs_166(
        &dir,
        &[
            ("dev-libs/ca", "1.0", "0", "+y", "y? ( dev-libs/cb )", ""),
            ("dev-libs/cb", "1.0", "0", "+z", "z? ( dev-libs/cc )", ""),
            ("dev-libs/cc", "1.0", "0", "+x", "x? ( dev-libs/ca )", ""),
        ],
    );
    let config = cfg_166();
    let sols = circular_dep_solutions(
        &[
            "dev-libs/ca-1.0".to_string(),
            "dev-libs/cb-1.0".to_string(),
            "dev-libs/cc-1.0".to_string(),
        ],
        &repos,
        &config,
        &[],
        &[],
    );
    assert_eq!(
        sols_166(&sols),
        std::collections::BTreeSet::from([
            "dev-libs/ca-1.0 [y=false] followup=false".to_string(),
            "dev-libs/cb-1.0 [z=false] followup=false".to_string(),
            "dev-libs/cc-1.0 [x=false] followup=false".to_string(),
        ])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A merge instance with an entry reads its resolved display
/// flags, and decoys (wrong category, wrong package) never match.
#[test]
fn direct_solve_instance_use_166_merge_reads_its_own_entry() {
    let root = dir_166("iu-entry");
    let repos = Vec::new();
    let config = cfg_166();
    let replace = BTreeSet::new();
    let entries = vec![
        entry_166("other-libs", "iupkg", "1.0", vec![("dx".to_string(), true)]),
        entry_166(
            "dev-libs",
            "otherpkg",
            "1.0",
            vec![("dy".to_string(), true)],
        ),
        entry_166(
            "dev-libs",
            "iupkg",
            "1.0",
            vec![("fa".to_string(), true), ("fb".to_string(), false)],
        ),
    ];
    let input = iu_input_166(&entries, &replace, &root, &repos, &config);
    let (enabled, declared) =
        direct_solve_instance_use(&input, "dev-libs", "iupkg", &inst_166("1.0", false));
    assert_eq!(enabled, HashSet::from(["fa".to_string()]));
    assert_eq!(
        declared,
        HashSet::from(["fa".to_string(), "fb".to_string()])
    );
    let _ = std::fs::remove_dir_all(&root);
}
