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
