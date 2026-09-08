use super::*;

use portage_util::TempDir;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .expect("fixtures/ does not resolve: it is a symlink to ../pmtest/fixtures, so the sibling pmtest repo has to be checked out next to this one")
}

/// These unit tests exercise portage-repo's own resolution logic in isolation
/// from profile/make.conf/package.* parsing (portage-profile's job, tested
/// separately): visibility is pinned to a fixed "amd64"-only, no-overrides
/// config, constructed directly (not via `portage_profile::resolve_config`).
fn test_config() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

fn masters_test_root(name: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portage-repo-masters-test-{name}")).keep()
}

fn fixture_binhost() -> portage_profile::BinRepo {
    portage_profile::BinRepo {
        name: "testbinhost".to_string(),
        sync_uri: fixtures_root()
            .join("binhost")
            .to_string_lossy()
            .into_owned(),
        priority: 1,
        location: None,
        verify_signature: true,
        frozen: false,
    }
}

/// A binrepo whose `Packages` index lives only in the in-memory override
/// table, under a pid+nanos-unique `sync_uri` so parallel tests never collide.
fn in_memory_binhost_1010(
    tag: &str,
    entries: Vec<HashMap<String, String>>,
) -> portage_profile::BinRepo {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let binrepo = portage_profile::BinRepo {
        name: format!("multiinst-{tag}"),
        sync_uri: format!(
            "https://binhost.invalid/{tag}-{}-{nanos}",
            std::process::id()
        ),
        priority: 1,
        location: None,
        verify_signature: false,
        frozen: false,
    };
    set_remote_binary_index_override(
        &binrepo.packages_dir(Path::new("/")),
        Some(std::sync::Arc::new(BinaryIndex::from_entries(entries))),
    );
    binrepo
}

fn resolve(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions {
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve`, but with `--emptytree`/`-e` enabled.
fn resolve_empty(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions {
            is_top_level: true,
            empty: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// A minimal ad-hoc vdb entry: `var/db/pkg/<cat>/<pf>/` with a
/// `SLOT` file plus whatever extra files the test writes.
fn tmp_vdb(cat: &str, pf: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let root = TempDir::new("portuale-vdb").keep();
    let dir = root.join("var/db/pkg").join(cat).join(pf);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SLOT"), "0\n").unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    root
}

/// Like `resolve`, but with `--update` enabled.
fn resolve_update(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions {
            update: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve`, but `is_top_level=false`: exercises the avoid_update
/// shortcut's DEPENDENCY-atom code path, distinct from the top-level one.
fn resolve_as_dependency(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions::default(),
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve`, but with `selective=false` (Portage's own default for a
/// bare top-level atom).
fn resolve_not_selective(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions {
            selective: false,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve_update`, but with `excluded` too (`--exclude` beats `--update`).
fn resolve_update_excluded(category: &str, package: &str, excluded: &[String]) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions {
            update: true,
            excluded,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Unlike `resolve()`, uses the fixture's fully resolved config
/// (package.mask/.unmask/.accept_keywords included), not the
/// synthetic `test_config()`.
fn resolve_real(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve_real`, but with `--newuse` enabled -- for the
/// `PretendOutcome::Reinstall` tests below.
fn resolve_real_newuse(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            newuse: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve_real`, but with `--changed-use` enabled.
fn resolve_real_changed_use(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            changed_use: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

fn resolve_real_changed_deps(category: &str, package: &str, with_bdeps: bool) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            changed_deps: true,
            with_bdeps,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

fn resolve_real_changed_slot(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            changed_slot: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

fn resolve_real_newrepo(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            is_top_level: true,
            newrepo: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Shared builders: a synthetic `SlotConflict` over
/// `dev-libs/slotconflicttarget:0` from `(version, installed,
/// [(parent_cpv, atom, parent_installed)])` rows, plus merge `New`
/// entries in walk order for the `or_tuple` first-pulled rule.
type S2Row<'x> = (&'x str, bool, Vec<(&'x str, &'x str, bool)>);

fn s2_conflict(rows: Vec<S2Row<'_>>) -> SlotConflict {
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "slotconflicttarget".to_string(),
        slot: "0".to_string(),
        resolved_version: rows[0].0.to_string(),
        conflicting_atom: rows
            .get(1)
            .map(|r| {
                r.2.first()
                    .map(|(_, a, _)| a.to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default(),
        instances: rows
            .into_iter()
            .map(|(v, installed, parents)| SlotConflictInstance {
                version: v.to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: parents
                    .into_iter()
                    .map(|(cpv, atom, pinst)| SlotConflictParent {
                        parent_cpv: cpv.to_string(),
                        atom: atom.to_string(),
                        use_display: Vec::new(),
                        installed: pinst,
                    })
                    .collect(),
                installed,
            })
            .collect(),
    }
}

fn s2_entries(versions: Vec<&str>) -> Vec<GraphEntry> {
    versions
        .into_iter()
        .map(|v| {
            let mut e = graph_entry("dev-libs", "slotconflicttarget", v);
            e.outcome = PretendOutcome::New {
                version: v.to_string(),
            };
            e
        })
        .collect()
}

/// Arguments of [`s2_input`].
struct S2InputArgs<'a> {
    conflicts: &'a [SlotConflict],
    entries: &'a [GraphEntry],
    top_level: &'a HashSet<(String, String)>,
    replace_cps: &'a BTreeSet<(String, String)>,
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
}

fn s2_input<'a>(args: S2InputArgs<'a>) -> DirectSolveInput<'a> {
    let S2InputArgs {
        conflicts,
        entries,
        top_level,
        replace_cps,
        root,
        repos,
        config,
    } = args;
    DirectSolveInput {
        conflicts,
        entries,
        top_level,
        excluded: &[],
        selective: false,
        replace_cps,
        root,
        repos,
        config,
    }
}

fn mk_at(root: &std::path::Path, name: &str, key: &str, dep: &str) {
    let d = root.join("var/db/pkg/dev-libs").join(format!("{name}-1.0"));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    std::fs::write(d.join("SLOT"), "0\n").unwrap();
    std::fs::write(d.join(key), format!("{dep}\n")).unwrap();
}

/// Arguments of [`graph_rebuild_if`].
struct GraphRebuildIfArgs<'a> {
    atom_str: &'a str,
    unbuilt: bool,
    new_rev: bool,
    new_ver: bool,
    rebuild_exclude: &'a [&'a str],
    rebuild_ignore: &'a [&'a str],
}

/// Arguments of [`assert_one_conflict`].
struct AssertOneConflictArgs<'a> {
    result: &'a GraphResult,
    category: &'a str,
    package: &'a str,
    slot: &'a str,
    resolved_version: &'a str,
    conflicting_atom: &'a str,
}

#[track_caller]
fn assert_one_conflict(args: AssertOneConflictArgs<'_>) {
    let AssertOneConflictArgs {
        result,
        category,
        package,
        slot,
        resolved_version,
        conflicting_atom,
    } = args;
    assert_eq!(
        result.slot_conflicts.len(),
        1,
        "expected exactly one slot conflict"
    );
    let c = &result.slot_conflicts[0];
    assert_eq!(
        (
            c.category.as_str(),
            c.package.as_str(),
            c.slot.as_str(),
            c.resolved_version.as_str(),
            c.conflicting_atom.as_str()
        ),
        (category, package, slot, resolved_version, conflicting_atom)
    );
}

/// ABI probe harness: installed `abicons-1` recording `app-misc/abiprov:0/1=`,
/// tree `abiprov-1`/`abiprov-2` + consumer (live `:=`) + forcer. `with_consumer`
/// controls whether the consumer's same-version ebuild is visible in the tree.
fn abi_probe_harness(with_consumer: bool) -> (std::path::PathBuf, Vec<RepoConfig>) {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-abi-probe").keep();
    for (name, slot, rdepend) in [
        ("abiprov-1", "0/1", ""),
        ("abicons-1", "0", "app-misc/abiprov:0/1="),
    ] {
        let d = base.join("var/db/pkg/app-misc").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "app-misc\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str, rdepend: &str| {
        let dir = repo.join("app-misc").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let mut body =
            format!("EAPI=8\nDESCRIPTION=\"214 abi\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        if !rdepend.is_empty() {
            writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
        }
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=214 abi\nEAPI=8\n".to_string();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/app-misc");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("abiprov", "1", "0/1", "");
    write_pkg("abiprov", "2", "0/2", "");
    if with_consumer {
        write_pkg("abicons", "1", "0", "app-misc/abiprov:=");
    }
    write_pkg("abiforce", "1", "0", ">=app-misc/abiprov-2");
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
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
    }];
    (base, repos)
}

fn abi_probe_entries(consumer_outcome: PretendOutcome) -> Vec<GraphEntry> {
    let provider = |version: &str, sub: &str| GraphEntry {
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        slot: Some("0".into()),
        sub_slot: Some(sub.into()),
        ..graph_entry("app-misc", "abiprov", version)
    };
    vec![
        provider("2", "2"),
        provider("1", "1"),
        GraphEntry {
            outcome: consumer_outcome,
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            ..graph_entry("app-misc", "abicons", "1")
        },
        GraphEntry {
            outcome: PretendOutcome::New {
                version: "1".to_string(),
            },
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            ..graph_entry("app-misc", "abiforce", "1")
        },
    ]
}

/// Residual-report helpers over the `souprov` fixtures (1.0 at 0/1, 2.0 at
/// 0/2). A `:=`-rebuild-style dropped pin whose atom matches the installed
/// instance but not the merged one yields one record: merge instance first,
/// installed second, with the consumer folded in at its vdb slot and
/// repository.
fn residual_161_setup(dir: &Path) -> (Vec<RepoConfig>, portage_profile::Config, GraphEntry) {
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let config = test_config();
    let d = dir.join("var/db/pkg/dev-libs/souprov-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    // The dropped-pin consumer: slot and repo flow verbatim into
    // the folded parent line.
    let c = dir.join("var/db/pkg/dev-libs/conpkg-1.0");
    fs::create_dir_all(&c).unwrap();
    fs::write(c.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(c.join("SLOT"), "3/0\n").unwrap();
    fs::write(c.join("repository"), "testrepo\n").unwrap();
    let upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "souprov".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "souprov", "2.0")
    };
    (repos, config, upgrade)
}

/// Shared harness for the `collect_feedback` legs: a minimal `ResolveCtx` plus
/// an empty `PassResult` each leg fills in.
static NO_STRINGS_161: Vec<String> = Vec::new();

#[derive(Default)]
struct CtxOpts161 {
    reachable: HashSet<(String, String)>,
    backtrack_max: u32,
    excluded: Vec<String>,
    atoms: Vec<String>,
    update: bool,
    newuse: bool,
    usepkg: bool,
    binpkg_respect_use: bool,
    usepkgonly: bool,
    nodeps: bool,
    selective: bool,
    with_test_deps: bool,
    deep: Deep,
    with_bdeps: bool,
    // `--buildpkgonly` blanks the run-time keys for a non-binary candidate;
    // default off like every other option flag.
    buildpkgonly: bool,
    complete: bool,
    locked: HashSet<(String, String)>,
    blocker_closure: HashSet<(String, String)>,
    /// The request's top-level atom texts (empty here unless a test needs the
    /// fatal top-level miss or the set-member drop).
    top_level: Vec<String>,
    /// Request atoms expanded from `@world`/`@selected`
    /// (see [`ResolveRequest::set_args`]).
    set_args: HashSet<String>,
}

fn ctx_161<'a>(
    root: &'a Path,
    config: &'a portage_profile::Config,
    repos: Vec<RepoConfig>,
    o: &'a CtxOpts161,
) -> ResolveCtx<'a> {
    ResolveCtx {
        root,
        atoms: &o.atoms,
        update: o.update,
        config,
        newuse: o.newuse,
        changed_use: false,
        nodeps: o.nodeps,
        onlydeps: false,
        onlydeps_with_rdeps: true,
        onlydeps_with_ideps: false,
        deep: o.deep,
        excluded: &o.excluded,
        with_bdeps: o.with_bdeps,
        changed_deps: false,
        changed_slot: false,
        with_test_deps: o.with_test_deps,
        changed_deps_report: false,
        selective: o.selective,
        autounmask_backtrack_enabled: false,
        usepkg: o.usepkg,
        usepkgonly: o.usepkgonly,
        binpkg_respect_use: o.binpkg_respect_use,
        usepkg_exclude: &NO_STRINGS_161,
        usepkg_include: &NO_STRINGS_161,
        rebuilt_binaries: false,
        rebuilt_binaries_timestamp: None,
        newrepo: false,
        buildpkgonly: o.buildpkgonly,
        root_deps_running_root: None,
        distdir: root,
        empty: false,
        getbinpkg: false,
        ignore_built_slot_operator_deps: false,
        backtrack_max: o.backtrack_max,
        reinstall_atoms: &NO_STRINGS_161,
        rebuild_if_new_slot: true,
        rebuild_if_unbuilt: false,
        rebuild_if_new_rev: false,
        rebuild_if_new_ver: false,
        rebuild_exclude: &NO_STRINGS_161,
        rebuild_ignore: &NO_STRINGS_161,
        dynamic_deps: true,
        implicit_system_deps: false,
        complete: o.complete,
        set_args: o.set_args.clone(),
        repos,
        slot_op_reachable: o.reachable.clone(),
        world_reachable: o.reachable.clone(),
        blocker_retry_closure: o.blocker_closure.clone(),
        complete_locked_merges: o.locked.clone(),
        top_level: o.top_level.iter().map(|s| s.as_str()).collect(),
        top_level_cps: HashSet::new(),
        // Like production's `ResolveCtx::new`: the binary pool
        // derives from the config's scanned binpkgs (empty here
        // unless a leg stages some).
        local_binpkg: build_local_binpkg_index(config),
    }
}

fn pass_161() -> PassResult {
    PassResult {
        entries: Vec::new(),
        slot_conflicts: Vec::new(),
        skipped_updates: Vec::new(),
        orphan_blockers: Vec::new(),
        slot_want: HashMap::new(),
        slot_pullers: HashMap::new(),
        masked_deps: Vec::new(),
        use_unsat_deps: Vec::new(),
        plain_miss_deps: Vec::new(),
        nvc_dep_atoms: HashMap::new(),
        missing_dep_trigger: None,
        missing_args: Vec::new(),
        autounmask_grew: false,
        parent_flip_rescued: false,
        edge_kind_map: HashMap::new(),
        changed_deps_report_entries: Vec::new(),
        pprovided_atoms: Vec::new(),
        autounmask_keyword_changes: Vec::new(),
        autounmask_use_changes: Vec::new(),
        autounmask_license_changes: Vec::new(),
        autounmask_mask_changes: Vec::new(),
        use_overlay: HashMap::new(),
        use_change_overlay: Vec::new(),
        use_broke: false,
        abi_rebuilds: None,
        suppressed_nvc: false,
        parent_atoms: Vec::new(),
    }
}

/// The `souprov` slot conflict for the solvable legs: 1.0 and 2.0
/// pulled, 2.0 resolved.
fn souprov_conflict_161() -> SlotConflict {
    let inst = |version: &str, sub: &str| SlotConflictInstance {
        version: version.to_string(),
        sub_slot: sub.to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: Vec::new(),
        installed: false,
    };
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "souprov".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: ">=dev-libs/souprov-2.0".to_string(),
        instances: vec![inst("2.0", "2"), inst("1.0", "1")],
    }
}

/// Arguments of [`ds_input_161`].
struct DsInput161Args<'a> {
    conflicts: &'a [SlotConflict],
    entries: &'a [GraphEntry],
    top_level: &'a HashSet<(String, String)>,
    excluded: &'a [String],
    selective: bool,
    replace_cps: &'a BTreeSet<(String, String)>,
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
}

/// Arguments of [`run_161`].
struct Run161Args<'a> {
    dir: &'a Path,
    atoms: &'a [String],
    update: bool,
    installed: &'a [(&'a str, &'a str, &'a str)],
    newuse: bool,
    usepkgonly: bool,
    nodeps: bool,
    selective: bool,
    usepkg: bool,
    deep: Deep,
    with_bdeps: bool,
}

/// One full BFS walk per leg over the fixture tree.
fn run_161(args: Run161Args<'_>) -> PassResult {
    let Run161Args {
        dir,
        atoms,
        update,
        installed,
        newuse,
        usepkgonly,
        nodeps,
        selective,
        usepkg,
        deep,
        with_bdeps,
    } = args;
    for (name, version, slot) in installed {
        let d = dir
            .join("var/db/pkg/dev-libs")
            .join(format!("{name}-{version}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
    }
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.to_vec(),
        update,
        newuse,
        usepkg,
        usepkgonly,
        nodeps,
        selective,
        deep,
        with_bdeps,
        ..Default::default()
    };
    let ctx = ctx_161(dir, &config, repos, &opts);
    run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles")
}

/// Arguments of [`blocker_161_write_pkg_full`].
struct Blocker161WritePkgFullArgs<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    depend: &'a str,
    rdepend: &'a str,
    bdepend: &'a str,
}

/// The `_full` variant also writes `BDEPEND`.
fn blocker_161_write_pkg_full(args: Blocker161WritePkgFullArgs<'_>) {
    let Blocker161WritePkgFullArgs {
        repo,
        cp,
        pv,
        slot,
        depend,
        rdepend,
        bdepend,
    } = args;
    use md5::Digest as _;
    use std::fmt::Write as _;
    let (cat, pkg) = cp.split_once('/').expect("category/package");
    let dir = repo.join(cat).join(pkg);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body =
        format!("EAPI=8\nDESCRIPTION=\"161 blocker\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
    if !depend.is_empty() {
        writeln!(body, "DEPEND=\"{depend}\"").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
    }
    if !bdepend.is_empty() {
        writeln!(body, "BDEPEND=\"{bdepend}\"").unwrap();
    }
    std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=161 blocker\nEAPI=8\n".to_string();
    if !depend.is_empty() {
        writeln!(entry, "DEPEND={depend}").unwrap();
    }
    if !rdepend.is_empty() {
        writeln!(entry, "RDEPEND={rdepend}").unwrap();
    }
    if !bdepend.is_empty() {
        writeln!(entry, "BDEPEND={bdepend}").unwrap();
    }
    writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
    let cachedir = repo.join("metadata/md5-cache").join(cat);
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
}

/// Arguments of [`blocker_161_scratch_repo`].
struct Blocker161ScratchRepoArgs<'a> {
    base: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    depend: &'a str,
    rdepend: &'a str,
}

/// Arguments of [`slotundo_vdb`].
struct SlotundoVdbArgs<'a> {
    dir: &'a Path,
    name: &'a str,
    version: &'a str,
    slot: &'a str,
    rdepend: &'a str,
    installed_use: &'a str,
}

/// recordered `USE`.
fn slotundo_vdb(args: SlotundoVdbArgs<'_>) {
    let SlotundoVdbArgs {
        dir,
        name,
        version,
        slot,
        rdepend,
        installed_use,
    } = args;
    let d = dir
        .join("var/db/pkg/dev-libs")
        .join(format!("{name}-{version}"));
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    if !rdepend.is_empty() {
        fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
    }
    if !installed_use.is_empty() {
        fs::write(d.join("USE"), format!("{installed_use}\n")).unwrap();
    }
}

/// A walked rebuild entry: `Reinstall { slot_operator_rebuild: true }`
/// at the installed version, with the given resolved USE display.
fn slotundo_rebuild_entry(
    name: &str,
    version: &str,
    slot: &str,
    sub_slot: &str,
    use_display: &[(&str, bool)],
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: name.into(),
        outcome: PretendOutcome::Reinstall {
            version: version.into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: true,
        },
        slot: Some(slot.into()),
        sub_slot: Some(sub_slot.into()),
        use_flags_display: use_display
            .iter()
            .map(|(f, on)| ((*f).to_string(), *on))
            .collect(),
        ..graph_entry("dev-libs", name, version)
    }
}

/// The `souprov` upgrade entry every undo test walks the consumer
/// against: 1.0 (0/1) -> 2.0 (0/2), the shape
/// `fixtures/repo/dev-libs/souprov` + its md5-cache provide.
fn slotundo_provider_entry() -> GraphEntry {
    GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "souprov", "2.0")
    }
}

fn slotundo_temp_dir(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portage-repo-{tag}")).keep()
}

/// Arguments of [`euf_flat`].
struct EufFlatArgs<'a> {
    iuse: &'a str,
    use_tokens: &'a [String],
    package_use: &'a [(String, Vec<String>)],
    package_use_force: &'a [(String, Vec<String>)],
    package_use_mask: &'a [(String, Vec<String>)],
    use_force: &'a HashSet<String>,
    use_mask: &'a HashSet<String>,
    use_stable_force: &'a HashSet<String>,
    use_stable_mask: &'a HashSet<String>,
    package_use_stable_force: &'a [(String, Vec<String>)],
    package_use_stable_mask: &'a [(String, Vec<String>)],
    keywords: &'a [String],
    accept_keywords: &'a HashSet<String>,
    package_accept_keywords: &'a [(String, Vec<String>)],
    candidate_str: &'a str,
    category: &'a str,
    package: &'a str,
}

/// Test shim for the pre-"Config depth" positional `effective_use_flags`
/// signature (single flat `package_use` list). Tests that exercise the split
/// call `effective_use_flags` directly.
fn euf_flat(args: EufFlatArgs<'_>) -> HashSet<String> {
    let EufFlatArgs {
        iuse,
        use_tokens,
        package_use,
        package_use_force,
        package_use_mask,
        use_force,
        use_mask,
        use_stable_force,
        use_stable_mask,
        package_use_stable_force,
        package_use_stable_mask,
        keywords,
        accept_keywords,
        package_accept_keywords,
        candidate_str,
        category,
        package,
    } = args;
    let config = portage_profile::Config {
        use_tokens: use_tokens.to_vec(),
        package_use: package_use.to_vec(),
        package_use_force: package_use_force.to_vec(),
        package_use_mask: package_use_mask.to_vec(),
        use_force: use_force.clone(),
        use_mask: use_mask.clone(),
        use_stable_force: use_stable_force.clone(),
        use_stable_mask: use_stable_mask.clone(),
        package_use_stable_force: package_use_stable_force.to_vec(),
        package_use_stable_mask: package_use_stable_mask.to_vec(),
        accept_keywords: accept_keywords.clone(),
        package_accept_keywords: package_accept_keywords.to_vec(),
        // `effective_use_flags`/`forced_or_masked_flags` now read the
        // per-level view; collapse every flat input into one
        // synthetic level (global-then-package, same as the old flat
        // application).
        use_mask_force_levels: vec![portage_profile::UseMaskForceLevel {
            use_mask: use_mask.iter().cloned().collect(),
            use_force: use_force.iter().cloned().collect(),
            use_stable_mask: use_stable_mask.iter().cloned().collect(),
            use_stable_force: use_stable_force.iter().cloned().collect(),
            package_use_mask: package_use_mask.to_vec(),
            package_use_force: package_use_force.to_vec(),
            package_use_stable_mask: package_use_stable_mask.to_vec(),
            package_use_stable_force: package_use_stable_force.to_vec(),
        }],
        ..Default::default()
    };
    // Test helper keeps the owned return; the `Rc` is unwrapped here.
    effective_use_flags(EffectiveUseFlagsArgs {
        config: &config,
        iuse,
        keywords,
        candidate_str,
        category,
        package,
    })
    .as_ref()
    .clone()
}

fn candidate(version: &str, keywords: &[&str]) -> Candidate {
    Candidate {
        version: version.to_string(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
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

fn license_tokens(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

/// Direct unit legs for the masking/visibility stack: each leg observes its
/// predicate's own result, no end-to-end contract reproduction.
mod tests_164 {
    use super::*;

    /// Hand-built binary candidate: `binary_use` is the baked USE set
    /// (Portage's `pkg.use.enabled`), `iuse` its domain.
    fn binary_164(version: &str, iuse: &str, use_flags: &[&str]) -> Candidate {
        let mut c = candidate(version, &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = iuse.to_string();
        c.binary_use = Some(use_flags.iter().map(|s| s.to_string()).collect());
        c
    }

    #[test]
    fn keyword_masked_only_164_mask_unmask_interplay() {
        let rescued = candidate("1.0", &["~amd64"]);
        // In `package.mask` but rescued by `package.unmask`: the mask
        // gate stays shut, so a keyword-masked candidate still
        // counts as keyword-masked only ...
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/kwunmask".to_string()],
            package_unmask: vec!["dev-libs/kwunmask".to_string()],
            ..Default::default()
        };
        assert!(keyword_masked_only(
            &rescued, "dev-libs", "kwunmask", &config
        ));
        // ... while a mask with no unmask masks outright, whatever
        // the keywords say.
        let config_masked = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/kwmasked".to_string()],
            ..Default::default()
        };
        assert!(!keyword_masked_only(
            &rescued,
            "dev-libs",
            "kwmasked",
            &config_masked
        ));
    }

    /// A candidate masked by `package.mask` alone (Portage's first
    /// `_getmaskingstatus` reason).
    #[test]
    fn mask_masked_only_164_needs_a_mask_and_nothing_else() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/maskonly".to_string()],
            ..Default::default()
        };
        let masked = candidate("1.0", &["amd64"]);
        assert!(mask_masked_only(&masked, "dev-libs", "maskonly", &config));
        // No mask entry at all: not mask-masked, however visible.
        assert!(!mask_masked_only(
            &masked,
            "dev-libs",
            "maskabsent",
            &config
        ));
    }

    #[test]
    fn mask_masked_only_164_false_when_another_reason_blocks() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            package_mask: vec![
                "dev-libs/maskkw".to_string(),
                "dev-libs/masklic".to_string(),
                "dev-libs/maskprops".to_string(),
                "dev-libs/maskrestrict".to_string(),
            ],
            ..Default::default()
        };
        // Masked AND keyword-rejected.
        let kw = candidate("1.0", &["~amd64"]);
        assert!(!mask_masked_only(&kw, "dev-libs", "maskkw", &config));
        // Masked AND license-unaccepted.
        let lic = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!mask_masked_only(&lic, "dev-libs", "masklic", &config));
        // Masked AND PROPERTIES-unaccepted.
        let props = Candidate {
            properties: "interactive".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!mask_masked_only(&props, "dev-libs", "maskprops", &config));
        // Masked AND RESTRICT-unaccepted.
        let restrict = Candidate {
            restrict: "fetch".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!mask_masked_only(
            &restrict,
            "dev-libs",
            "maskrestrict",
            &config
        ));
    }

    #[test]
    fn mask_masked_only_164_unmask_cancels_the_mask() {
        // Masked AND unmasked: `package.unmask` cancels the mask (Portage's
        // unmask half), so the candidate is not mask-masked at all.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/maskrescue".to_string()],
            package_unmask: vec!["dev-libs/maskrescue".to_string()],
            ..Default::default()
        };
        assert!(!mask_masked_only(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "maskrescue",
            &config
        ));
    }

    /// A candidate masked by LICENSE alone (Portage's `LicenseManager`).
    #[test]
    fn license_masked_only_164_needs_a_license_and_only_a_license() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        // GPL-2 unaccepted, keywords fine: license-masked only.
        let lic_only = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(license_masked_only(
            &lic_only, "dev-libs", "liconly", &config
        ));
        // License accepted: not license-masked at all.
        let accepted = Candidate {
            license: "MIT".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!license_masked_only(
            &accepted, "dev-libs", "licok", &config
        ));
    }

    #[test]
    fn license_masked_only_164_mask_unmask_leaves_the_license_verdict() {
        // In `package.mask` but rescued by `package.unmask`: the
        // mask gate stays shut, so the license verdict still shows.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            package_mask: vec!["dev-libs/licunmask".to_string()],
            package_unmask: vec!["dev-libs/licunmask".to_string()],
            ..Default::default()
        };
        let lic = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(license_masked_only(&lic, "dev-libs", "licunmask", &config));
    }

    #[test]
    fn license_masked_only_164_use_conditional_follows_the_flag() {
        // Portage `use_reduce`'s own "if '?' in license_str" path: the
        // `foo? ( ... )` group only constrains once `foo` is enabled.
        // `+foo` in IUSE is enabled by default, no profile needed.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        // MIT accepted but the active conditional pulls in an
        // unaccepted GPL-2: license-masked only.
        let cond = Candidate {
            license: "MIT foo? ( GPL-2 )".to_string(),
            iuse: "+foo".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(license_masked_only(&cond, "dev-libs", "liccond", &config));
        // Same shape with the flag default-off: the group drops out,
        // MIT alone is accepted -- not license-masked.
        let cond_off = Candidate {
            license: "MIT foo? ( GPL-2 )".to_string(),
            iuse: "-foo".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!license_masked_only(
            &cond_off,
            "dev-libs",
            "liccondoff",
            &config
        ));
    }

    #[test]
    fn license_masked_only_164_invalid_is_never_license_only() {
        // `invalid` disqualifies the claim even with a genuinely unaccepted
        // LICENSE.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        let mut invalid = binary_164("1.0", "", &[]);
        invalid.license = "GPL-2".to_string();
        invalid.binary_deps = HashMap::from([(
            "RDEPEND".to_string(),
            "bogusflag? ( dev-libs/foo )".to_string(),
        )]);
        assert!(!license_masked_only(
            &invalid,
            "dev-libs",
            "licinvalid",
            &config
        ));
    }

    /// Arguments of [`entry_164`].
    struct Entry164Args<'a> {
        category: &'a str,
        package: &'a str,
        outcome: PretendOutcome,
        required_by: Vec<(String, String)>,
        repo: Option<&'a str>,
        source: CandidateSource,
    }

    /// Minimal `GraphEntry` for the dep-chain legs (merge-bound unless stated
    /// otherwise).
    fn entry_164(args: Entry164Args<'_>) -> GraphEntry {
        let Entry164Args {
            category,
            package,
            outcome,
            required_by,
            repo,
            source,
        } = args;
        GraphEntry {
            discovery: 0,
            category: category.to_string(),
            package: package.to_string(),
            outcome,
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: repo.map(str::to_string),
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by,
            source,
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

    fn new_164(package: &str, version: &str, parent: &str) -> GraphEntry {
        entry_164(Entry164Args {
            category: "dev-libs",
            package,
            outcome: PretendOutcome::New {
                version: version.to_string(),
            },
            required_by: vec![("dev-libs".to_string(), parent.to_string())],
            repo: Some("testrepo"),
            source: CandidateSource::Ebuild,
        })
    }

    /// `required_use_dep_chain` reports the failing package's direct parent
    /// plus its command-line argument (Portage's `_show_unsatisfied_dep` tail).
    #[test]
    fn required_use_dep_chain_164_reports_parent_then_argument() {
        let entries = vec![
            // Decoy: same category, wrong package -- must not win
            // the parent lookup.
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "rudecoy",
                outcome: PretendOutcome::New {
                    version: "9.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "ruparent",
                outcome: PretendOutcome::New {
                    version: "1.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let owner = Some(("dev-libs".to_string(), "ruparent".to_string()));
        let top: HashSet<&str> = ["=dev-libs/ruparent-1.0"].into_iter().collect();
        let got = required_use_dep_chain(&owner, &top, &entries);
        assert_eq!(
            got,
            vec![
                (
                    "dev-libs/ruparent-1.0::testrepo".to_string(),
                    "ebuild".to_string()
                ),
                ("=dev-libs/ruparent-1.0".to_string(), "argument".to_string()),
            ]
        );
    }

    #[test]
    fn required_use_dep_chain_164_upgrade_parent_reports_the_to_version() {
        let entries = vec![entry_164(Entry164Args {
            category: "dev-libs",
            package: "ruup",
            outcome: PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            required_by: Vec::new(),
            repo: Some("testrepo"),
            source: CandidateSource::Ebuild,
        })];
        let owner = Some(("dev-libs".to_string(), "ruup".to_string()));
        let top: HashSet<&str> = ["sys-apps/other"].into_iter().collect();
        let got = required_use_dep_chain(&owner, &top, &entries);
        assert_eq!(
            got,
            vec![(
                "dev-libs/ruup-2.0::testrepo".to_string(),
                "ebuild".to_string()
            )]
        );
    }

    #[test]
    fn required_use_dep_chain_164_argument_needs_the_same_package() {
        // A same-category other-package atom is not this parent's
        // argument line.
        let entries = vec![entry_164(Entry164Args {
            category: "dev-libs",
            package: "rusib",
            outcome: PretendOutcome::New {
                version: "1.0".to_string(),
            },
            required_by: Vec::new(),
            repo: Some("testrepo"),
            source: CandidateSource::Ebuild,
        })];
        let owner = Some(("dev-libs".to_string(), "rusib".to_string()));
        let top: HashSet<&str> = ["dev-libs/rusibling"].into_iter().collect();
        let got = required_use_dep_chain(&owner, &top, &entries);
        assert_eq!(
            got,
            vec![(
                "dev-libs/rusib-1.0::testrepo".to_string(),
                "ebuild".to_string()
            )]
        );
    }

    #[test]
    fn required_use_dep_chain_164_no_owner_and_installed_parent_yield_nothing() {
        // No owner: the failing package IS the top-level atom, and Portage
        // skips the block.
        let entries = vec![entry_164(Entry164Args {
            category: "dev-libs",
            package: "rux",
            outcome: PretendOutcome::New {
                version: "1.0".to_string(),
            },
            required_by: Vec::new(),
            repo: Some("testrepo"),
            source: CandidateSource::Ebuild,
        })];
        let top: HashSet<&str> = ["dev-libs/rux"].into_iter().collect();
        assert_eq!(required_use_dep_chain(&None, &top, &entries), Vec::new());
        // An `AlreadyInstalled` parent has no merge version to name.
        let installed = vec![entry_164(Entry164Args {
            category: "dev-libs",
            package: "ruinst",
            outcome: PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string(),
            },
            required_by: Vec::new(),
            repo: None,
            source: CandidateSource::Ebuild,
        })];
        let owner = Some(("dev-libs".to_string(), "ruinst".to_string()));
        let empty: HashSet<&str> = HashSet::new();
        assert_eq!(
            required_use_dep_chain(&owner, &empty, &installed),
            Vec::new()
        );
    }
}

/// Arguments of [`euf_layers`].
struct EufLayersArgs<'a> {
    iuse: &'a str,
    use_tokens: &'a [String],
    conf_use_tokens: &'a [String],
    package_use_repo: &'a [(String, Vec<String>)],
    package_use: &'a [(String, Vec<String>)],
    package_use_user: &'a [(String, Vec<String>)],
}

/// "Config depth" slice: `package.use` from each of the three config sources
/// lands at its own `USE_ORDER` position.
fn euf_layers(args: EufLayersArgs<'_>) -> HashSet<String> {
    let EufLayersArgs {
        iuse,
        use_tokens,
        conf_use_tokens,
        package_use_repo,
        package_use,
        package_use_user,
    } = args;
    let cfg = portage_profile::Config {
        use_tokens: use_tokens.to_vec(),
        conf_use_tokens: conf_use_tokens.to_vec(),
        package_use_repo: package_use_repo.to_vec(),
        package_use: package_use.to_vec(),
        package_use_user: package_use_user.to_vec(),
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    };
    effective_use_flags(EffectiveUseFlagsArgs {
        config: &cfg,
        iuse,
        keywords: &["amd64".to_string()],
        candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
        category: "dev-libs",
        package: "pkg",
    })
    .as_ref()
    .clone()
}

fn pu(flag: &str) -> Vec<(String, Vec<String>)> {
    vec![("dev-libs/pkg".to_string(), vec![flag.to_string()])]
}

fn graph_entry(category: &str, package: &str, version: &str) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: category.to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: Some("0".to_string()),
        sub_slot: Some("0".to_string()),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
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

mod tests_162;

mod tests_163;

mod tests_165;

mod tests_166;

mod tests_184;

mod abi_probe;
mod binary_index;
mod bind;
mod binpkg_use;
mod blockers;
mod candidates;
mod complete_graph;
mod config_match;
mod conflict_use_display;
mod cycles;
mod depclean;
mod direct_solve;
mod eliminate;
mod entry;
mod force_scan;
mod graph;
mod info_pkgs;
mod installed;
mod license;
mod masked_deps;
mod metadata_cache;
mod pass;
mod pretend;
mod queue;
mod rebuild_scan;
mod reinstall;
mod repo_config;
mod resolver_trace;
mod rev_dep;
mod slot_undo;
mod updates;
mod use_display;
mod use_flags;
mod visibility;
