/// Backlog #184: the shadowed-arm order legs for
/// `direct_solve_instance_use`/`direct_solve_atom_matches`, in the
/// #166 scratch style. Kept self-contained (own scratch repo helpers
/// copied from `tests_166`) so the other Track U branches' blocks
/// rebase mechanically.
use crate::*;
use portage_util::TempDir;

// ---- #184: the shadowed (no-entry) arm of
// `direct_solve_instance_use` returned `slot_conflict_flag_sets`'s
// `(iuse, use_flags)` = `(declared, enabled)` pair as-is against the
// documented `(enabled, declared)` contract (the installed and entry
// arms both return `(enabled, declared)`, and the caller
// `direct_solve_atom_matches` destructures `let (enabled, declared) =
// ...`). Real evaluates the atom's USE deps against the package's
// enabled USE with its IUSE as the valid set: `Package.with_use`
// (`3rdparty/portage/lib/_emerge/Package.py:870`) applied to
// `depgraph._pkg_use_enabled`
// (`3rdparty/portage/lib/_emerge/depgraph.py:7669`). The swap
// evaluated them against the IUSE as if every declared flag were
// enabled. ----

/// Fresh unique scratch dir per test (vdb lives under it, so the
/// per-root aux caches never see a reused path).
fn dir_184(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-184-{tag}")).keep()
}

/// One scratch ebuild plus its real md5-cache entry (the md5-cache
/// validation guard fails otherwise).
fn write_pkg_184(repo: &Path, cp: &str, pv: &str, slot: &str, iuse: &str) {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = format!(
        "EAPI=8\nDESCRIPTION=\"184 shadowed instance-use\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n"
    );
    if !iuse.is_empty() {
        writeln!(body, "IUSE=\"{iuse}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=184 shadowed instance-use\nEAPI=8\n".to_string();
    if !iuse.is_empty() {
        writeln!(entry, "IUSE={iuse}").unwrap();
    }
    writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

/// Single scratch repo (`testrepo`, priority 0, main) holding
/// `(cp, version, slot, IUSE)` ebuilds with `KEYWORDS="amd64"`.
fn repo_pkgs_184(dir: &Path, pkgs: &[(&str, &str, &str, &str)]) -> Vec<RepoConfig> {
    let repo = dir.join("repo");
    for (cp, pv, slot, iuse) in pkgs {
        write_pkg_184(&repo, cp, pv, slot, iuse);
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

/// Empty top-level set for inputs whose verdict never consults it.
static EMPTY_TOP_184: std::sync::LazyLock<HashSet<(String, String)>> =
    std::sync::LazyLock::new(HashSet::new);

fn iu_input_184<'a>(
    entries: &'a [GraphEntry],
    replace_cps: &'a BTreeSet<(String, String)>,
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
) -> DirectSolveInput<'a> {
    let conflicts: &'a [SlotConflict] = &[];
    let top: &'a HashSet<(String, String)> = &EMPTY_TOP_184;
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

/// A shadowed merge instance (no entry, not installed) reads the
/// tree candidate effectively in `(enabled, declared)` order: IUSE
/// `+fa fb` enables only `fa` (the `+` default) while declaring
/// both. Before the fix the arm returned
/// `slot_conflict_flag_sets` as-is, i.e. `(enabled, declared) =
/// ({fa, fb}, {fa})`.
#[test]
fn direct_solve_instance_use_184_shadowed_reads_tree_effectively() {
    let root = dir_184("iu-shadowed");
    let dir = dir_184("iu-shadowed-repo");
    let repos = repo_pkgs_184(&dir, &[("dev-libs/iupkg", "1.0", "0", "+fa fb")]);
    let config = cfg_184();
    let replace = BTreeSet::new();
    // No entries: the instance is shadowed (a second instance with
    // no merge-bound entry of its own).
    let input = iu_input_184(&[], &replace, &root, &repos, &config);
    let (enabled, declared) =
        direct_solve_instance_use(&input, "dev-libs", "iupkg", &inst_184("1.0", false));
    assert_eq!(enabled, HashSet::from(["fa".to_string()]));
    assert_eq!(
        declared,
        HashSet::from(["fa".to_string(), "fb".to_string()])
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `[fb(+)]` use-dep is not satisfied by the shadowed instance:
/// `fb` is declared but not enabled. Before the fix the swapped arm
/// put the whole IUSE in the enabled position, so the dep wrongly
/// matched. (Plain `[fb]` cannot discriminate: the
/// `use_deps_satisfied` valid-set gate rejects a default-less flag
/// outside IUSE either way.)
#[test]
fn direct_solve_atom_matches_184_shadowed_rejects_disabled_flag() {
    let root = dir_184("am-shadowed");
    let dir = dir_184("am-shadowed-repo");
    let repos = repo_pkgs_184(&dir, &[("dev-libs/iupkg", "1.0", "0", "+fa fb")]);
    let config = cfg_184();
    let replace = BTreeSet::new();
    let input = iu_input_184(&[], &replace, &root, &repos, &config);
    let conflict = conflict_184();
    assert!(!direct_solve_atom_matches(
        &input,
        &conflict,
        0,
        "dev-libs/iupkg[fb(+)]",
        false
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&dir);
}
