/// Backlog #166 (circular/instance-use solution residue): direct
/// predicate-result legs in the #161/#162 scratch style. Each test
/// observes the predicate's own return value -- no end-to-end contract
/// reproduction -- so every in-body operator flip changes an asserted
/// outcome. Kept self-contained (own scratch repo/vdb/entry helpers)
/// so the other Track U branches' blocks rebase mechanically. Do not
/// edit other items' `mod tests_16x` blocks.
use crate::*;
use portage_util::TempDir;

// ---- S1: `strip_revision` (`--rebuild-if-new-ver` compares the
// merged version against installed ones with `-r<n>` stripped on
// both sides -- see `rebuild_if_entries`). ----

/// Only a numeric `-r<n>` suffix strips, and the split is at the
/// last `-r`: kills the whole-body `""`/`"xyzzy"` rows, both guard
/// replacements, the `&&` -> `||` widening (empty rev strips) and
/// the `!` deletion (numeric rev kept).
#[test]
fn strip_revision_166_strips_numeric_revisions_only() {
    assert_eq!(strip_revision("1.0-r1"), "1.0");
    assert_eq!(strip_revision("1.0"), "1.0");
    // Non-numeric tail: the guard stays shut (guard-`true` would
    // strip to "1.0").
    assert_eq!(strip_revision("1.0-rx"), "1.0-rx");
    assert_eq!(strip_revision("1.0-r1x"), "1.0-r1x");
    // Empty rev: `!rev.is_empty()` is false while `all(digits)` is
    // vacuously true, so `&&` -> `||` would strip to "1.0".
    assert_eq!(strip_revision("1.0-r"), "1.0-r");
    assert_eq!(strip_revision("1.0-r0"), "1.0");
    // Last `-r` wins: only the trailing numeric revision strips.
    assert_eq!(strip_revision("1.0-r1-r2"), "1.0-r1");
}

// ---- S2: `filter_usepkg_exclude_include` (real
// `create_depgraph_params.py` `--usepkg-exclude`/`--usepkg-include`
// handling, applied to the binary pool in `depgraph.py`'s own
// break-out-of-the-loop rejection -- see the call site's doc
// comment). ----

/// Hand-built binary candidate in the #162 `cand_162` shape (same
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

/// Exclude-only: the pinned version drops, the rest stays. Kills
/// the either-empty gate widening (`&&` -> `||` keeps everything),
/// the `||` -> `&&` narrowing in `is_included` (drops everything)
/// and the `!` deletion (keeps the excluded row).
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
/// excludes it. Kills the `!excluded && included` -> `||`
/// widening (keeps the row through `!excluded` alone).
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
    // Pin: a matching include keeps the row (no mutant target;
    // documents the `is_empty() ||` arm).
    let out = filter_usepkg_exclude_include(
        vec![cand_166("1.0")],
        "dev-libs",
        "foo",
        &[],
        &["dev-libs/foo".to_string()],
    );
    assert_eq!(out.len(), 1);
}

// ---- S3: `rebuilt_binary_changed` (real `depgraph.py`'s two
// independent `rebuilt_binary` triggers folded into one: the
// `--rebuilt-binaries` BUILD_TIME comparison and the
// no-ebuild-visible rejection of an installed built instance --
// see the function's own doc comment). ----

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
/// newer past the floor reports, anything else does not. Kills the
/// `&&` -> `||` widening (older binary reports through the floor
/// arm), the three `>` rewrites and the `>=` -> `<` flip.
#[test]
fn rebuilt_binary_166_timestamped_comparison() {
    let root = dir_166("rebuilt-ts");
    install_166(&root, "dev-libs", "rebuilt-1.0", "100\n");
    let changed = |built: Option<i64>, ts: Option<u64>| {
        rebuilt_binary_changed(&root, built, "dev-libs", "rebuilt", "1.0", ts)
    };
    // No binary candidate (or no BUILD_TIME): no verdict either way
    // (pins both `let-else return false` arms; no mutant targets).
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

/// Untimestamped comparison: any difference reports. Kills the `!=`
/// -> `==` flip in both directions (newer reports, equal does not).
#[test]
fn rebuilt_binary_166_untimestamped_comparison() {
    let root = dir_166("rebuilt-plain");
    install_166(&root, "dev-libs", "rebuilt-1.0", "100\n");
    let changed = |built: Option<i64>| {
        rebuilt_binary_changed(&root, built, "dev-libs", "rebuilt", "1.0", None)
    };
    assert!(changed(Some(200)));
    assert!(!changed(Some(100)));
    // No installed BUILD_TIME at all: the second `let-else` fires
    // (no mutant target; documents the missing-record tolerance).
    let bare = dir_166("rebuilt-bare");
    std::fs::create_dir_all(bare.join("var/db/pkg/dev-libs/rebuilt-1.0")).unwrap();
    assert!(!rebuilt_binary_changed(
        &bare,
        Some(200),
        "dev-libs",
        "rebuilt",
        "1.0",
        None
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&bare);
}

// ---- S4: `direct_solve_arg_mode` (real `_want_installed_pkg`,
// `depgraph.py:7280-7302`, folded to what the direct solve needs:
// the installed instance is "argued" exactly when the user asked
// for that cp without `--exclude` and outside selective mode). ----

/// Minimal visibility config in the #161 `test_config` shape
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

/// Eight-arg input builder (same shape as #161's `ds_input_161`).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
fn arg_input_166<'a>(
    conflicts: &'a [SlotConflict],
    top_level: &'a HashSet<(String, String)>,
    excluded: &'a [String],
    selective: bool,
    replace_cps: &'a BTreeSet<(String, String)>,
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
) -> DirectSolveInput<'a> {
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
/// exclude, not selective) is the only `true`: kills the
/// whole-body `-> false` row.
#[test]
fn direct_solve_arg_mode_166_argues_the_requested_installed() {
    let root = dir_166("arg-true");
    let repos = Vec::new();
    let config = cfg_166();
    let conflicts = [conflict_166(vec![("2.0", false), ("1.0", true)])];
    let top = arg_top_166();
    let replace = BTreeSet::new();
    assert!(direct_solve_arg_mode(
        &arg_input_166(
            &conflicts,
            &top,
            &[],
            false,
            &replace,
            &root,
            &repos,
            &config
        ),
        &conflicts[0]
    ));
    let _ = std::fs::remove_dir_all(&root);
}

/// Every missing precondition reports `false`: no installed
/// instance (kills the `!any(installed)` deletion and the `||` ->
/// `&&` narrowing, both of which proceed to `true`), selective
/// mode, an unrequested cp, and a matching `--exclude` (kills the
/// `!excluded.any` deletion, which reports `true`).
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
            &arg_input_166(conflicts, top, excl, sel, &replace, &root, &repos, &config),
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

// ---- S5: `direct_solve_instance_use` (real
// `pkg.with_use(self._pkg_use_enabled(pkg))`: a merge instance with
// an entry reads its resolved display flags, a merge instance
// without one reads the tree candidate effectively, an installed
// instance reads its vdb record -- see the function's own doc
// comment). ----

/// One scratch ebuild plus its real md5-cache entry (the md5-cache
/// validation guard fails otherwise), in the #162
/// `write_pkg_162` shape extended with `DEPEND`/`REQUIRED_USE`
/// lines for the circular legs below.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
fn write_pkg_166(
    repo: &Path,
    cp: &str,
    pv: &str,
    slot: &str,
    iuse: &str,
    depend: &str,
    required_use: &str,
) {
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
        write_pkg_166(&repo, cp, pv, slot, iuse, depend, required_use);
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
/// `IUSE` (declared): kills the whole-body rows and the `==` ->
/// `!=` version-find flip (which falls back to `enabled.clone()`).
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

// ---- S6: `synthesize_surviving_conflict_entries` (files each
// surviving conflict instance beside its merge-bound siblings, so
// the final merge list prints conflicting instances adjacently --
// see the function's own doc comment). ----

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
/// sibling (not at the end, not before): kills both `+` -> `-`/`*`
/// index shifts, both `&&` -> `||` widenings (which file behind the
/// trailing decoy instead) and both `==` -> `!=` flips (which file
/// behind the other-category same-package row, or at the end).
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
    synthesize_surviving_conflict_entries(
        &mut entries,
        &[synth_conflict_166()],
        &synth_owners_166(),
        &repos,
        &root,
        &config,
    );
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

/// `new_slot` reports whether the cp is already installed: kills
/// the `!is_empty()` deletion in both directions (installed reads
/// `true`, fresh root reads `false`).
#[test]
fn synthesize_166_new_slot_tracks_the_installed_set() {
    let dir = dir_166("synth-slot-repo");
    let repos = repo_pkgs_166(&dir, &[("dev-libs/foo", "2.0", "0", "fa", "", "")]);
    let config = cfg_166();
    let owners = synth_owners_166();
    let filed_new_slot = |root: &Path| {
        let mut entries = Vec::new();
        synthesize_surviving_conflict_entries(
            &mut entries,
            &[synth_conflict_166()],
            &owners,
            &repos,
            root,
            &config,
        );
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

// ---- S7: `circular_dep_solutions` (real `_find_suggestions`,
// `3rdparty/portage/lib/_emerge/resolver/circular_dependency.py:114`:
// per-cycle-position USE assignments whose reduced dep drops the
// pull atom while REQUIRED_USE still holds, minimised and checked
// against grandparent use-deps). Cycles are handed in directly as
// cpv strings over scratch repos -- no graph walk needed. ----

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
/// "enable y" removes the atom while REQUIRED_USE holds. Kills the
/// cur-construction `<<` -> `>>` (bit 1 never sets, no solution),
/// the sol-diff `<<` -> `>>` plus both `&&` flips and both `!`
/// deletions around the diff (all emit `[{}}` or a bloated pair),
/// the REQUIRED_USE `!empty` deletion and the `!ok` deletion (both
/// admit the violating empty assignment), and the `>` -> `<`
/// pair on the MAX_AFFECTING_USE gates (retain/drop to nothing).
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
/// fi" pair is a minimal solution (nine suggestions). Kills the
/// REQUIRED_USE `!untouchable` deletion (collapses to nothing), the
/// `<=` -> `>` expansion flip (stays unexpanded, nothing holds),
/// and the `>` -> `==`/`>=` pair on both MAX_AFFECTING_USE gates
/// (retain-to-one / give-up, both empty).
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
/// solution. Kills the `&&` -> `||` widening on the REQUIRED_USE
/// gate (skips the only solution).
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
/// the ten enabled ones and the second lets exactly ten through.
/// Every retained flag gates its own occurrence of the pull atom,
/// so the only solution disables all ten. Kills the second gate's
/// `==` -> continue and `>=` -> continue (both give up at exactly
/// ten where real proceeds).
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
/// solution disables all nine. Kills the second gate's `<` ->
/// continue (which gives up below ten where real proceeds).
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
/// reports its own parent. Kills the parent-index `-` -> `+`
/// rotation (identical on two-rings, wrong parent on three).
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
/// flags, and decoys (wrong category, wrong package) never match:
/// kills the whole-body rows, the category/package `==` -> `!=`
/// flips (which match the first decoy instead) and the first `&&`
/// -> `||` widening (which matches the same-category decoy).
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
