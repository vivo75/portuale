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
        fetchcommand: None,
        resumecommand: None,
        getbinpkg_exclude: Vec::new(),
        getbinpkg_include: Vec::new(),
    }
}

/// Binpkg-multi-instance builds of one `cpv` are told apart only by `BUILD_ID`.
/// Shared fixture for the three tests below, oldest first like a live index.
fn multi_instance_entry_1010(
    build_id: &str,
    build_time: i64,
    rdepend: &str,
) -> HashMap<String, String> {
    HashMap::from([
        ("CPV".to_string(), "dev-libs/spectre-0.2.12".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "gentoo".to_string()),
        ("BUILD_ID".to_string(), build_id.to_string()),
        ("BUILD_TIME".to_string(), build_time.to_string()),
        ("RDEPEND".to_string(), rdepend.to_string()),
        (
            "PATH".to_string(),
            format!("dev-libs/spectre/spectre-0.2.12-{build_id}.gpkg.tar"),
        ),
    ])
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
        fetchcommand: None,
        resumecommand: None,
        getbinpkg_exclude: Vec::new(),
        getbinpkg_include: Vec::new(),
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

/// Like `resolve_as_dependency`, but with `--update` at depth 1 and no
/// `--deep`: the `_want_update_pkg` shape (a non-argument dep without
/// `--deep` is never wanted for update).
fn resolve_dep_update(atom_str: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        atom_str,
        &SelectOptions {
            update: true,
            depth: 1,
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

/// Write a consolidated `metadata` snapshot body into `root`'s entry for
/// `<cat>/<pf>` and, when `stamp`, append the `#dir_mtime=` stamp after the
/// body write (Portage's write-then-stamp order). Returns the `metadata` path.
fn write_snapshot(root: &Path, cat: &str, pf: &str, body: &str, stamp: bool) -> PathBuf {
    let dir = root.join("var/db/pkg").join(cat).join(pf);
    let path = dir.join("metadata");
    std::fs::write(&path, body).unwrap();
    if stamp {
        let ns = dir_mtime_ns(&dir);
        append_line(&path, &format!("#dir_mtime={ns}"));
    }
    path
}

fn append_line(path: &Path, line: &str) {
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(f, "{line}").unwrap();
}

/// The directory's own `st_mtime_ns`, for stamping a runtime snapshot.
fn dir_mtime_ns(dir: &Path) -> i128 {
    use std::os::unix::fs::MetadataExt as _;
    let st = std::fs::metadata(dir).unwrap();
    st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128
}

/// Bump a package dir's mtime without changing what it holds (create
/// then remove a scratch entry) -- the memo's invalidation signal.
fn bump_dir_mtime(dir: &Path) {
    std::fs::write(dir.join("scratch"), b"").unwrap();
    std::fs::remove_file(dir.join("scratch")).unwrap();
}

fn bz2(bytes: &[u8]) -> Vec<u8> {
    use bzip2::write::BzEncoder;
    use std::io::Write as _;
    let mut enc = BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(bytes).unwrap();
    enc.finish().unwrap()
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

fn graph_changed_deps_report(
    atom_str: &str,
    changed_deps: bool,
    changed_deps_report: bool,
) -> GraphResult {
    let root = fixtures_root();
    ResolveRequest {
        changed_deps,
        changed_deps_report,
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
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

fn graph_with(
    atom_str: &str,
    tweak: impl FnOnce(&mut ResolveRequest),
) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    let mut request = ResolveRequest::new(
        &root,
        &root,
        &[atom_str.to_string()],
        &test_config(),
        &fixtures_root().join("distfiles"),
    );
    tweak(&mut request);
    request
        .resolve()
        .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
}

fn graph(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_with(atom_str, |_| {})
}

/// Resolve one atom as an `--onlydeps` root with the given `--onlydeps-with-*`
/// values, over the shared fixture tree (the unit-level counterpart of the
/// `odw0*` contract cells).
fn graph_onlydeps(
    atom_str: &str,
    with_rdeps: bool,
    with_ideps: bool,
) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    let req = ResolveRequest {
        config_root: root.clone(),
        root: root.clone(),
        argument_sets: HashMap::new(),
        atoms: vec![atom_str.to_string()],
        config: test_config(),
        newuse: false,
        changed_use: false,
        nodeps: false,
        onlydeps: true,
        onlydeps_with_rdeps: with_rdeps,
        onlydeps_with_ideps: with_ideps,
        update: false,
        deep: Deep::NotRequested,
        excluded: Vec::new(),
        with_bdeps: true,
        changed_deps: false,
        changed_slot: false,
        with_test_deps: false,
        changed_deps_report: false,
        selective: false,
        autounmask_suggest_keywords: false,
        autounmask_suggest_use: false,
        autounmask_suggest_license: false,
        autounmask_suggest_masks: false,
        usepkg: false,
        usepkgonly: false,
        binpkg_respect_use: false,
        usepkg_exclude: Vec::new(),
        usepkg_include: Vec::new(),
        rebuilt_binaries: false,
        rebuilt_binaries_timestamp: None,
        newrepo: false,
        buildpkgonly: false,
        root_deps_running_root: None,
        distdir: root.join("distfiles"),
        empty: false,
        getbinpkg: false,
        ignore_built_slot_operator_deps: false,
        backtrack_max: 10,
        reinstall_atoms: Vec::new(),
        rebuild_if_new_slot: true,
        rebuild_if_unbuilt: false,
        rebuild_if_new_rev: false,
        rebuild_if_new_ver: false,
        rebuild_exclude: Vec::new(),
        rebuild_ignore: Vec::new(),
        dynamic_deps: true,
        implicit_system_deps: true,
        complete: false,
        complete_reuse: HashMap::new(),
        set_args: std::collections::HashSet::new(),
        solver: SolverKind::Portage,
    };
    active_resolver()
        .resolve(&req)
        .unwrap_or_else(|e| panic!("resolve({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
}

/// Resolve a multi-atom fixture graph with `--update`, returning full entries
/// (the `graph*` helpers above map to name/outcome pairs only).
fn graph_update_entries(atoms: &[&str], config: &portage_profile::Config) -> Vec<GraphEntry> {
    let root = fixtures_root();
    let owned: Vec<String> = atoms.iter().map(|a| a.to_string()).collect();
    ResolveRequest {
        update: true,
        ..ResolveRequest::new(
            &root,
            &root,
            &owned,
            config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {owned:?} failed: {e}"))
    .entries
}

/// Like `graph`, but with `--nodeps` enabled.
fn graph_nodeps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_with(atom_str, |r| r.nodeps = true)
}

/// Like `graph`, but with `--update` enabled.
fn graph_update(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_with(atom_str, |r| r.update = true)
}

/// Like `graph`, but with a specific `Deep` value.
fn graph_deep(atom_str: &str, deep: Deep) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    ResolveRequest {
        deep,
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect()
}

/// Like `graph_deep(atom, Deep::Unlimited)`, but with a
/// `--reinstall-atoms` set applied.
fn graph_deep_reinstall_atoms(
    atom_str: &str,
    reinstall_atoms: &[&str],
) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    let ra: Vec<String> = reinstall_atoms.iter().map(|s| s.to_string()).collect();
    ResolveRequest {
        deep: Deep::Unlimited,
        reinstall_atoms: ra.to_vec(),
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect()
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

/// `resolve_pretend_graph` for one atom with the `--rebuild-if-*`
/// flags set (and `--update` on, so `rebuildtrigger` upgrades).
fn graph_rebuild_if(args: GraphRebuildIfArgs<'_>) -> Vec<(String, PretendOutcome)> {
    let GraphRebuildIfArgs {
        atom_str,
        unbuilt,
        new_rev,
        new_ver,
        rebuild_exclude,
        rebuild_ignore,
    } = args;
    let root = fixtures_root();
    let ex: Vec<String> = rebuild_exclude.iter().map(|s| s.to_string()).collect();
    let ig: Vec<String> = rebuild_ignore.iter().map(|s| s.to_string()).collect();
    ResolveRequest {
        update: true,
        rebuild_if_unbuilt: unbuilt,
        rebuild_if_new_rev: new_rev,
        rebuild_if_new_ver: new_ver,
        rebuild_exclude: ex.to_vec(),
        rebuild_ignore: ig.to_vec(),
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect()
}

/// Resolve one consumer atom through the `--deep` walk and return
/// `(cat/pkg, outcome-variant, targets_running_root)` per graph entry.
/// `root_deps` passes the fixture root itself as the running root.
fn resolve_deep_consumer(atom: &str, root_deps: bool) -> Vec<(String, String, bool)> {
    let root = fixtures_root();
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
    let running_root = root.clone();
    ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: (if root_deps {
            Some(running_root.as_path())
        } else {
            None
        })
        .map(std::path::Path::to_path_buf),
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom.to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap()
    .entries
    .into_iter()
    .map(|e| {
        let outcome = match &e.outcome {
            PretendOutcome::NoVisibleCandidate => "NoVisibleCandidate",
            PretendOutcome::New { .. } => "New",
            PretendOutcome::Upgrade { .. } => "Upgrade",
            PretendOutcome::Downgrade { .. } => "Downgrade",
            PretendOutcome::AlreadyInstalled { .. } => "AlreadyInstalled",
            PretendOutcome::Reinstall { .. } => "Reinstall",
            PretendOutcome::Uninstall { .. } => "Uninstall",
        };
        (
            format!("{}/{}", e.category, e.package),
            outcome.to_string(),
            e.targets_running_root,
        )
    })
    .collect()
}

/// Like `graph_deep`, but driven by `--emptytree`/`-e` instead
/// (`deep` forced on, every installed atom -> a bare Reinstall).
fn graph_empty(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_with(atom_str, |r| r.empty = true)
}

fn graph_deep_with_bdeps(atom_str: &str, with_bdeps: bool) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    ResolveRequest {
        deep: Deep::Unlimited,
        with_bdeps,
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect()
}

fn graph_root_deps(
    atom_str: &str,
    root_deps_running_root: Option<&Path>,
) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
    ResolveRequest {
        deep: Deep::Unlimited,
        root_deps_running_root: root_deps_running_root.map(std::path::Path::to_path_buf),
        ..ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
    .into_iter()
    .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
    .collect()
}

fn full_graph(atom_str: &str) -> Vec<GraphEntry> {
    let root = fixtures_root();
    ResolveRequest::new(
        &root,
        &root,
        &[atom_str.to_string()],
        &test_config(),
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
}

fn graph_real_with(
    atom_str: &str,
    tweak: impl FnOnce(&mut ResolveRequest),
) -> Vec<(String, PretendOutcome)> {
    let root = fixtures_root();
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
    let mut request = ResolveRequest::new(
        &root,
        &root,
        &[atom_str.to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    );
    tweak(&mut request);
    request
        .resolve()
        .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
}

fn graph_real(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_real_with(atom_str, |_| {})
}

/// Like `graph_real`, but with `--with-test-deps` enabled.
fn graph_real_with_test_deps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_real_with(atom_str, |r| r.with_test_deps = true)
}

/// Like `graph_real`, but for a call expected to fail outright --
/// returns the error message instead of panicking on one.
fn graph_real_err(atom_str: &str) -> String {
    let root = fixtures_root();
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
    ResolveRequest::new(
        &root,
        &root,
        &[atom_str.to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect_err(&format!(
        "resolve_pretend_graph({atom_str}) should have failed"
    ))
    .to_string()
}

/// Like `graph_real`, but with `--newuse` enabled.
fn graph_real_newuse(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_real_with(atom_str, |r| r.newuse = true)
}

/// Like `graph_real`, but with `--changed-use` enabled.
fn graph_real_changed_use(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_real_with(atom_str, |r| r.changed_use = true)
}

/// Like `graph_real`, but with `--nodeps` enabled.
fn graph_real_nodeps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
    graph_real_with(atom_str, |r| r.nodeps = true)
}

fn graph_result_real(atom_str: &str) -> GraphResult {
    graph_result_real_backtrack(atom_str, 10)
}

/// Resolves `atom_str` over the fixture tree's own resolved config,
/// after `tweak` adjusts the request.
fn graph_result_with(atom_str: &str, tweak: impl FnOnce(&mut ResolveRequest)) -> GraphResult {
    let root = fixtures_root();
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
    let mut request = ResolveRequest::new(
        &root,
        &root,
        &[atom_str.to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    );
    tweak(&mut request);
    request
        .resolve()
        .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
}

fn graph_result_empty(atom_str: &str) -> GraphResult {
    graph_result_with(atom_str, |r| r.empty = true)
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

fn graph_result_real_backtrack(atom_str: &str, backtrack_max: u32) -> GraphResult {
    graph_result_real_atoms(&[atom_str.to_string()], backtrack_max)
}

/// `graph_result_real_backtrack` for more than one top-level atom.
fn graph_result_real_atoms(atoms: &[String], backtrack_max: u32) -> GraphResult {
    let atom_str = atoms.join(" ");
    let root = fixtures_root();
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
    ResolveRequest {
        backtrack_max,
        ..ResolveRequest::new(
            &root,
            &root,
            atoms,
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
}

fn graph_entries_real(atom_str: &str) -> Vec<GraphEntry> {
    graph_result_real(atom_str).entries
}

/// Like `graph_result_real`, but resolved with the pubgrub engine: the S9
/// (#363/#364/#365) parity probes assert these match the default solver's
/// abort outcome and notice vectors. Same request, only the resolver differs.
fn graph_result_pubgrub(atom_str: &str) -> GraphResult {
    let root = fixtures_root();
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
    active_resolver_for(SolverKind::PubGrub)
        .resolve(&ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        ))
        .unwrap_or_else(|e| panic!("pubgrub resolving {atom_str} failed: {e}"))
}

/// Like `graph_real_err`, but resolved with the pubgrub engine.
fn graph_pubgrub_err(atom_str: &str) -> String {
    let root = fixtures_root();
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
    active_resolver_for(SolverKind::PubGrub)
        .resolve(&ResolveRequest::new(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        ))
        .expect_err(&format!("pubgrub resolving {atom_str} should have failed"))
        .to_string()
}

/// Like `graph_result_autounmask`, but resolved with the pubgrub engine.
fn graph_result_autounmask_pubgrub(atom_str: &str) -> GraphResult {
    let root = fixtures_root();
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
    graph_result_autounmask_cfg_pubgrub(&config, atom_str)
}

/// `graph_result_autounmask_pubgrub` with an explicit config.
fn graph_result_autounmask_cfg_pubgrub(
    config: &portage_profile::Config,
    atom_str: &str,
) -> GraphResult {
    let root = fixtures_root();
    let atoms = vec![atom_str.to_string()];
    active_resolver_for(SolverKind::PubGrub)
        .resolve(&ResolveRequest {
            autounmask_suggest_keywords: true,
            autounmask_suggest_use: true,
            autounmask_suggest_license: true,
            autounmask_suggest_masks: true,
            ..ResolveRequest::new(
                &root,
                &root,
                &atoms,
                config,
                &fixtures_root().join("distfiles"),
            )
        })
        .unwrap_or_else(|e| panic!("pubgrub resolving {atom_str} failed: {e}"))
}

/// Like `graph_result_real`, but with `blocker_retry_seed_atoms` set (the
/// `@world ∪ @selected ∪ @system` list of Portage's complete-mode retry).
fn graph_entries_with_retry_seeds(atom_str: &str, seeds: &[&str]) -> Vec<GraphEntry> {
    let root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    config.blocker_retry_seed_atoms = seeds.iter().map(|s| s.to_string()).collect();
    ResolveRequest::new(
        &root,
        &root,
        &[atom_str.to_string()],
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
    .entries
}

/// Like `graph_result_real`, but with `--autounmask` keyword
/// resolution enabled (`autounmask_suggest_keywords = true`).
fn graph_result_autounmask(atom_str: &str) -> GraphResult {
    let root = fixtures_root();
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
    graph_result_autounmask_cfg(&config, atom_str)
}

/// `graph_result_autounmask` with `--autounmask-backtrack=y`: the
/// `'backtrack` loop re-drives the whole walk on every accumulator
/// growth (Slices 1/4's full cascade), rather than the default
/// collect-and-display-only behaviour.
fn graph_result_autounmask_backtrack(atom_str: &str) -> GraphResult {
    let root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    config.autounmask_backtrack = true;
    graph_result_autounmask_cfg(&config, atom_str)
}

fn graph_result_autounmask_cfg(config: &portage_profile::Config, atom_str: &str) -> GraphResult {
    graph_result_autounmask_cfg_atoms(config, &[atom_str.to_string()])
}

/// `graph_result_autounmask_cfg` for several top-level atoms, in argv order.
fn graph_result_autounmask_cfg_atoms(
    config: &portage_profile::Config,
    atoms: &[String],
) -> GraphResult {
    let atom_str = atoms.join(" ");
    let root = fixtures_root();
    ResolveRequest {
        autounmask_suggest_keywords: true,
        autounmask_suggest_use: true,
        autounmask_suggest_license: true,
        autounmask_suggest_masks: true,
        ..ResolveRequest::new(
            &root,
            &root,
            atoms,
            config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving {atom_str} failed: {e}"))
}

/// `graph_result_autounmask` with `autounmask_suggest_use` forced off
/// (an explicit `--autounmask-use=n`).
fn graph_result_autounmask_use_n(atom_str: &str) -> GraphResult {
    graph_result_with(atom_str, |r| r.autounmask_suggest_keywords = true)
}

/// The six `aub0` argument orders (upstream `test_autounmask_use_breakage`).
/// Expected values from Portage 3.0.82.2.
fn aub0_result(order: &str, backtrack_y: bool) -> GraphResult {
    let root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    config.autounmask_backtrack = backtrack_y;
    let atoms: Vec<String> = order
        .split_whitespace()
        .map(|p| format!("dev-libs/aub0{p}"))
        .collect();
    graph_result_autounmask_cfg_atoms(&config, &atoms)
}

fn aub0_conflict_versions(result: &GraphResult) -> Vec<Vec<String>> {
    result
        .slot_conflicts
        .iter()
        .map(|c| c.instances.iter().map(|i| i.version.clone()).collect())
        .collect()
}

fn assert_aub0_fails_on_a(result: &GraphResult, order: &str) {
    assert!(
        matches!(
            &result.outcome,
            ResolveOutcome::Aborted {
                reason: AbortReason::UnsatisfiedAtom { atom, parent_cpv },
                ..
            } if atom == "dev-libs/aub0d[-foo]" && parent_cpv == "dev-libs/aub0a-0"
        ),
        "{order}: {:?}",
        result.outcome
    );
    assert!(result.autounmask_use_changes.is_empty(), "{order}");
    assert_eq!(result.use_unsat_deps.len(), 1, "{order}");
    assert_eq!(
        result.use_unsat_deps[0].chain,
        vec![
            (
                "dev-libs/aub0a-0::testrepo".to_string(),
                "ebuild".to_string()
            ),
            ("dev-libs/aub0a".to_string(), "argument".to_string()),
        ],
        "{order}: the failing dependency's own chain only"
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

fn abi_conflict_record(
    inst0_parents: Vec<(&str, &str)>,
    inst1_parents: Vec<(&str, &str)>,
) -> SlotConflict {
    let edge = |cpv: &str, atom: &str| SlotConflictParent {
        parent_cpv: cpv.to_string(),
        atom: atom.to_string(),
        use_display: Vec::new(),
        installed: false,
    };
    let inst = |version: &str, sub: &str, parents: Vec<(&str, &str)>| SlotConflictInstance {
        version: version.to_string(),
        sub_slot: sub.to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: parents.into_iter().map(|(c, a)| edge(c, a)).collect(),
        installed: false,
    };
    SlotConflict {
        category: "app-misc".to_string(),
        package: "abiprov".to_string(),
        slot: "0".to_string(),
        resolved_version: "2".to_string(),
        conflicting_atom: "app-misc/abiprov:0/1=".to_string(),
        instances: vec![inst("2", "2", inst0_parents), inst("1", "1", inst1_parents)],
    }
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

/// The `souprov` dropped pin: broken by the 2.0 merge, preserved by
/// installed 1.0, owned by `conpkg-1.0`.
fn residual_161_pin() -> RevDepPin {
    RevDepPin {
        cp: ("dev-libs".to_string(), "souprov".to_string()),
        atom: "=dev-libs/souprov-1.0:0/1".to_string(),
        raw_atom: "=dev-libs/souprov-1.0:0/1".to_string(),
        consumer: (
            "dev-libs".to_string(),
            "conpkg".to_string(),
            "1.0".to_string(),
        ),
    }
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
        complete_reuse: HashMap::new(),
        top_level: o.top_level.iter().map(|s| s.as_str()).collect(),
        top_level_cps: HashSet::new(),
        // Like production's `ResolveCtx::new`: the binary pool
        // derives from the config's scanned binpkgs (empty here
        // unless a leg stages some).
        local_binpkg: build_local_binpkg_index(config),
        tree_sim_memo: Default::default(),
        pool_memo: Default::default(),
        installed_meta_memo: Default::default(),
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

/// `direct_solve` legs over the `slotconflict*` fixtures.
fn ds_input_161<'a>(args: DsInput161Args<'a>) -> DirectSolveInput<'a> {
    let DsInput161Args {
        conflicts,
        entries,
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
        entries,
        top_level,
        excluded,
        selective,
        replace_cps,
        root,
        repos,
        config,
    }
}

/// The old/new shape with top-level args: `oldconsumer`'s
/// `<2.0` forces 1.0 to stay, so the bare `newconsumer` tuple drops
/// 2.0 with a skip row -- the baseline every leg below varies.
fn ds_s0_161() -> (
    [SlotConflict; 1],
    Vec<GraphEntry>,
    HashSet<(String, String)>,
) {
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(new_cpv, "dev-libs/slotconflicttarget", false)],
        ),
        (
            "1.0",
            false,
            vec![(old_cpv, "<dev-libs/slotconflicttarget-2.0", false)],
        ),
    ])];
    let entries = s2_entries(vec!["2.0", "1.0"]);
    let top: HashSet<(String, String)> = HashSet::from([
        (
            "dev-libs".to_string(),
            "slotconflictnewconsumer".to_string(),
        ),
        (
            "dev-libs".to_string(),
            "slotconflictoldconsumer".to_string(),
        ),
    ]);
    (conflicts, entries, top)
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

/// Arguments of [`blocker_161_write_pkg`].
struct Blocker161WritePkgArgs<'a> {
    repo: &'a Path,
    cp: &'a str,
    pv: &'a str,
    slot: &'a str,
    depend: &'a str,
    rdepend: &'a str,
}

/// Scratch-repo package writer shared by the blocker legs: `blockerparent-1.0`
/// merges `victim-1.0` through `RDEPEND` while softly blocking it (a runtime
/// key, always walked; `DEPEND` would need `--with-bdeps`).
fn blocker_161_write_pkg(args: Blocker161WritePkgArgs<'_>) {
    let Blocker161WritePkgArgs {
        repo,
        cp,
        pv,
        slot,
        depend,
        rdepend,
    } = args;
    blocker_161_write_pkg_full(Blocker161WritePkgFullArgs {
        repo,
        cp,
        pv,
        slot,
        depend,
        rdepend,
        bdepend: "",
    });
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

fn blocker_161_repo_config(repo: PathBuf) -> RepoConfig {
    RepoConfig {
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
    }
}

fn blocker_161_repo(base: &Path) -> Vec<RepoConfig> {
    let repo = base.join("repo");
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo,
        cp: "dev-libs/victim",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo,
        cp: "dev-libs/blockerparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/victim !dev-libs/victim",
    });
    vec![blocker_161_repo_config(repo)]
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

/// One scratch package in its own repo: for legs needing tree
/// versions the fixtures do not carry.
fn blocker_161_scratch_repo(args: Blocker161ScratchRepoArgs<'_>) -> RepoConfig {
    let Blocker161ScratchRepoArgs {
        base,
        cp,
        pv,
        slot,
        depend,
        rdepend,
    } = args;
    let repo = base.join("scratchrepo");
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo,
        cp,
        pv,
        slot,
        depend,
        rdepend,
    });
    blocker_161_repo_config(repo)
}

/// World=B shape shared by the unit tests that seed a world consumer: `dev-libs/abk0d` with
/// `--autounmask-backtrack=y` while installed `abk0b-1` is reachable through
/// the required sets. The fixture world file stays pristine, so the seed is
/// set directly (the `FX_WORLD_EXTRA=dev-libs/abk0b` equivalent the bed cell
/// stages).
fn graph_result_abk0_worldb_on(root: &Path, backtrack_max: u32) -> GraphResult {
    let fixture_root = fixtures_root();
    let mut config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &fixture_root,
        main_repo_location: &fixture_root.join("repo"),
        overlay_repos: &[("overlay".to_string(), fixture_root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &fixture_root,
    })
    .expect("fixture config resolves");
    config.autounmask_backtrack = true;
    config.complete_seed_atoms = vec!["dev-libs/abk0b".to_string()];
    ResolveRequest {
        autounmask_suggest_keywords: true,
        autounmask_suggest_use: true,
        autounmask_suggest_license: true,
        autounmask_suggest_masks: true,
        backtrack_max,
        ..ResolveRequest::new(
            &fixture_root,
            root,
            &["dev-libs/abk0d".to_string()],
            &config,
            &fixtures_root().join("distfiles"),
        )
    }
    .resolve()
    .unwrap_or_else(|e| panic!("resolving dev-libs/abk0d failed: {e}"))
}

fn graph_result_abk0_worldb(backtrack_max: u32) -> GraphResult {
    let root = fixtures_root();
    graph_result_abk0_worldb_on(&root, backtrack_max)
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

/// One shared assertion for the triangle's conflict record: installed
/// `paired-1.0` and merge `paired-2.0`, each filed under the hard atom that
/// pulled it, whichever argv order produced the collision.
fn assert_triangle_conflict(result: &GraphResult, installed_first: bool) {
    assert_eq!(
        result.slot_conflicts.len(),
        1,
        "expected exactly one slot conflict, got {:?}",
        result.slot_conflicts
    );
    let c = &result.slot_conflicts[0];
    assert_eq!(
        (c.category.as_str(), c.package.as_str()),
        ("dev-libs", "paired")
    );
    assert_eq!(c.slot, "0");
    let (installed, merge) = if installed_first {
        (&c.instances[0], &c.instances[1])
    } else {
        (&c.instances[1], &c.instances[0])
    };
    assert_eq!(c.resolved_version, c.instances[0].version);
    assert_eq!(
        (installed.version.as_str(), installed.installed),
        ("1.0", true)
    );
    assert_eq!((merge.version.as_str(), merge.installed), ("2.0", false));
    let atoms = |i: &SlotConflictInstance| -> Vec<(String, String)> {
        i.parents
            .iter()
            .map(|p| (p.parent_cpv.clone(), p.atom.clone()))
            .collect()
    };
    assert_eq!(
        atoms(installed),
        vec![(
            "dev-libs/othermod-1.0:0/0::testrepo".to_string(),
            "<dev-libs/paired-2.0".to_string()
        )]
    );
    assert_eq!(
        atoms(merge),
        vec![(
            "dev-libs/needer-1.0:0/0::testrepo".to_string(),
            ">=dev-libs/paired-2.0".to_string()
        )]
    );
}

fn graph_result_buildpkgonly(atom_str: &str) -> GraphResult {
    graph_result_with(atom_str, |r| r.buildpkgonly = true)
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
    build_id: Option<&'a str>,
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
        build_id,
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
        build_id,
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
        eapi: "8".to_string(),
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

    /// `parse_license_tree` reports exact token positions in every error
    /// (Portage signals these with a bare `InvalidDependString`).
    #[test]
    fn parse_license_tree_164_reports_exact_error_positions() {
        let toks = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        let no_use = HashSet::new();
        // `()` is an empty group at token 0: the lookahead reads
        // token 1, the error reports token 2.
        match parse_license_tree(&toks("( )"), &no_use) {
            Err(Error::LicenseExpectedDepString { token }) => assert_eq!(token, 2),
            other => panic!("an empty group must fail closed, got {other:?}"),
        }
        // A stray `)` with no opener: token 1 at position 0 ...
        match parse_license_tree(&toks(")"), &no_use) {
            Err(Error::LicenseNoMatchingOpen { token }) => assert_eq!(token, 1),
            other => panic!("a stray close must fail closed, got {other:?}"),
        }
        // ... and token 2 one step in.
        match parse_license_tree(&toks("GPL-2 )"), &no_use) {
            Err(Error::LicenseNoMatchingOpen { token }) => assert_eq!(token, 2),
            other => panic!("a trailing close must fail closed, got {other:?}"),
        }
        // A `)` where a `(` was promised by `||`.
        match parse_license_tree(&toks("|| )"), &no_use) {
            Err(Error::LicenseExpectedOpen { token }) => assert_eq!(token, 2),
            other => panic!("a close after `||` must fail closed, got {other:?}"),
        }
        // A second `||` where a `(` was promised.
        match parse_license_tree(&toks("|| ||"), &no_use) {
            Err(Error::LicenseExpectedOpenGotOr { token }) => assert_eq!(token, 2),
            other => panic!("a doubled `||` must fail closed, got {other:?}"),
        }
        // A bare token where a `(` was promised.
        match parse_license_tree(&toks("|| GPL-2"), &no_use) {
            Err(Error::LicenseExpectedOpenGotToken { token, at }) => {
                assert_eq!(token, "GPL-2");
                assert_eq!(at, 2);
            }
            other => panic!("a token after `||` must fail closed, got {other:?}"),
        }
        // Pins (no mutants): unclosed and dangling shapes.
        assert!(matches!(
            parse_license_tree(&toks("( GPL-2"), &no_use),
            Err(Error::LicenseMissingCloseParen)
        ));
        assert!(matches!(
            parse_license_tree(&toks("||"), &no_use),
            Err(Error::LicenseMissingOpenParen)
        ));
    }

    /// Hand-built binary candidate: `binary_use` is the baked USE set
    /// (Portage's `pkg.use.enabled`), `iuse` its domain.
    fn binary_164(version: &str, iuse: &str, use_flags: &[&str]) -> Candidate {
        let mut c = candidate(version, &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = iuse.to_string();
        c.binary_use = Some(use_flags.iter().map(|s| s.to_string()).collect());
        c
    }

    /// A candidate masked by KEYWORDS alone (Portage's `_getmaskingstatus`
    /// records one reason per failing category).
    #[test]
    fn keyword_masked_only_164_needs_keywords_and_only_keywords() {
        // `~amd64` under an `amd64` profile: keyword-masked, and
        // nothing else is wrong.
        let config = test_config();
        let kw_only = candidate("1.0", &["~amd64"]);
        assert!(keyword_masked_only(&kw_only, "dev-libs", "kwonly", &config));
        // Fully visible (`amd64`): not keyword-masked at all.
        let visible = candidate("1.0", &["amd64"]);
        assert!(!keyword_masked_only(
            &visible,
            "dev-libs",
            "kwvisible",
            &config
        ));
    }

    #[test]
    fn keyword_masked_only_164_false_when_another_reason_blocks() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        // Keywords rejected AND license unaccepted: the license gate
        // fires first, so this is not "keyword-masked only".
        let both = Candidate {
            keywords: vec!["~amd64".to_string()],
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["~amd64"])
        };
        assert!(!keyword_masked_only(&both, "dev-libs", "kwboth", &config));
        // Keywords rejected AND PROPERTIES unaccepted.
        let props = Candidate {
            keywords: vec!["~amd64".to_string()],
            properties: "interactive".to_string(),
            ..candidate("1.0", &["~amd64"])
        };
        assert!(!keyword_masked_only(&props, "dev-libs", "kwprops", &config));
        // Keywords rejected AND RESTRICT unaccepted.
        let restrict = Candidate {
            keywords: vec!["~amd64".to_string()],
            restrict: "fetch".to_string(),
            ..candidate("1.0", &["~amd64"])
        };
        assert!(!keyword_masked_only(
            &restrict,
            "dev-libs",
            "kwrestrict",
            &config
        ));
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

    #[test]
    fn keyword_masked_only_164_invalid_is_never_keyword_only() {
        // An `invalid` IUSE-conditional verdict disqualifies every "masked by
        // X alone" claim; a binary candidate carries its own dep keys, so no
        // repo staging is needed.
        let mut invalid = binary_164("1.0", "", &[]);
        invalid.keywords = vec!["~amd64".to_string()];
        invalid.binary_deps = HashMap::from([(
            "RDEPEND".to_string(),
            "bogusflag? ( dev-libs/foo )".to_string(),
        )]);
        assert!(!keyword_masked_only(
            &invalid,
            "dev-libs",
            "kwinvalid",
            &test_config()
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

    #[test]
    fn mask_masked_only_164_invalid_is_never_mask_only() {
        // Same shape as the keyword leg: an `invalid` verdict disqualifies the
        // "mask alone" claim even though the mask gate itself is open.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/maskinvalid".to_string()],
            ..Default::default()
        };
        let mut invalid = binary_164("1.0", "", &[]);
        invalid.binary_deps = HashMap::from([(
            "RDEPEND".to_string(),
            "bogusflag? ( dev-libs/foo )".to_string(),
        )]);
        assert!(!mask_masked_only(
            &invalid,
            "dev-libs",
            "maskinvalid",
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
    fn license_masked_only_164_false_when_another_reason_blocks() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            package_mask: vec!["dev-libs/licmasked".to_string()],
            ..Default::default()
        };
        let lic = || Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        // License-unaccepted AND keyword-rejected.
        let kw = Candidate {
            keywords: vec!["~amd64".to_string()],
            ..lic()
        };
        assert!(!license_masked_only(&kw, "dev-libs", "lickw", &config));
        // License-unaccepted AND PROPERTIES-unaccepted.
        let props = Candidate {
            properties: "interactive".to_string(),
            ..lic()
        };
        assert!(!license_masked_only(
            &props, "dev-libs", "licprops", &config
        ));
        // License-unaccepted AND RESTRICT-unaccepted.
        let restrict = Candidate {
            restrict: "fetch".to_string(),
            ..lic()
        };
        assert!(!license_masked_only(
            &restrict,
            "dev-libs",
            "licrestrict",
            &config
        ));
        // License-unaccepted AND package-masked.
        assert!(!license_masked_only(
            &lic(),
            "dev-libs",
            "licmasked",
            &config
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

    /// A visible candidate whose atom `[use]` deps fail (Portage checks a built
    /// package's own `[use]` deps against the baked set).
    #[test]
    fn use_masked_only_164_needs_an_unsatisfied_use_dep() {
        let use_dep = |flag: &str| portage_dep::UseDep {
            flag: flag.to_string(),
            op: portage_dep::UseDepOp::Enabled,
            default: None,
        };
        // `foo` declared but built disabled: `[foo]` unsatisfied.
        let unmet = binary_164("1.0", "foo", &[]);
        assert!(use_masked_only(
            &unmet,
            "dev-libs",
            "useunmet",
            &[use_dep("foo")],
            &test_config()
        ));
        // Same candidate built with `foo`: satisfied, not use-masked.
        let met = binary_164("1.0", "foo", &["foo"]);
        assert!(!use_masked_only(
            &met,
            "dev-libs",
            "usemet",
            &[use_dep("foo")],
            &test_config()
        ));
    }

    #[test]
    fn use_masked_only_164_invisible_is_never_use_only() {
        // A keyword-masked candidate fails `is_visible` first, so no USE
        // suggestion applies (Portage only suggests a change that would fix it).
        let use_dep = portage_dep::UseDep {
            flag: "foo".to_string(),
            op: portage_dep::UseDepOp::Enabled,
            default: None,
        };
        let mut masked = binary_164("1.0", "foo", &[]);
        masked.keywords = vec!["~amd64".to_string()];
        assert!(!use_masked_only(
            &masked,
            "dev-libs",
            "useinvisible",
            &[use_dep],
            &test_config()
        ));
    }

    /// Scratch repo writer with per-version KEYWORDS/LICENSE/SLOT.
    fn repo_164(base: &Path, pkgs: &[(&str, &str, &str, &str, &str)]) -> Vec<RepoConfig> {
        // (cp, pv, slot, keywords, license).
        use md5::Digest as _;
        use std::fmt::Write as _;
        let repo = base.join("repo164");
        for (cp, pv, slot, keywords, license) in pkgs {
            let (cat, pkg) = cp.split_once('/').expect("category/package");
            let dir = repo.join(cat).join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            let body = format!(
                "EAPI=8\nDESCRIPTION=\"164 visibility\"\nSLOT=\"{slot}\"\nKEYWORDS=\"{keywords}\"\nLICENSE=\"{license}\"\n"
            );
            std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
            let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
            let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=164 visibility\nEAPI=8\n".to_string();
            writeln!(entry, "KEYWORDS={keywords}").unwrap();
            writeln!(entry, "LICENSE={license}").unwrap();
            writeln!(entry, "SLOT={slot}\n_md5_={md5}").unwrap();
            let cachedir = repo.join("metadata/md5-cache").join(cat);
            std::fs::create_dir_all(&cachedir).unwrap();
            std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
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

    /// `visible_tree_matches` returns the visible candidates an atom selects
    /// (Portage's `_pkg_visibility_check`).
    #[test]
    fn visible_tree_matches_164_lists_only_visible_matches() {
        let dir = slotundo_temp_dir("164-vtm");
        let repos = repo_164(
            &dir,
            &[
                ("dev-libs/vtm", "1.0", "0", "amd64", "MIT"),
                ("dev-libs/vtm", "2.0", "0", "~amd64", "MIT"),
            ],
        );
        // `test_config` accepts no license by default; the scratch
        // packages declare MIT, so accept it (the KEYWORDS verdict
        // under test is orthogonal).
        let mut config = test_config();
        config.accept_license = vec!["*".to_string()];
        let got = visible_tree_matches(&repos, "dev-libs/vtm", &config, &[]);
        assert_eq!(got, vec![("1.0".to_string(), "0".to_string())]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn visible_tree_matches_164_negated_constraint_excludes() {
        // `!atom` drops the match it names: with 1.0 negated nothing
        // is left (2.0 is keyword-masked).
        let dir = slotundo_temp_dir("164-vtm-neg");
        let repos = repo_164(
            &dir,
            &[
                ("dev-libs/vtmneg", "1.0", "0", "amd64", "MIT"),
                ("dev-libs/vtmneg", "2.0", "0", "~amd64", "MIT"),
            ],
        );
        let mut config = test_config();
        config.accept_license = vec!["*".to_string()];
        // NOTE: the version half needs its `=` operator --
        // portage-dep rejects an operator-less versioned atom as
        // PMS-ambiguous (mirroring Portage's `Atom`), which
        // would make the constraint vacuously pass.
        let neg_one = ["!=dev-libs/vtmneg-1.0".to_string()];
        let got_none = visible_tree_matches(&repos, "dev-libs/vtmneg", &config, &neg_one);
        assert_eq!(got_none, Vec::new());
        // A negation naming nothing keeps the visible match ...
        let neg_idle = ["!=dev-libs/vtmneg-9.0".to_string()];
        let got_one = visible_tree_matches(&repos, "dev-libs/vtmneg", &config, &neg_idle);
        assert_eq!(got_one, vec![("1.0".to_string(), "0".to_string())]);
        // ... and so does a positive constraint it satisfies.
        let pos_one = ["=dev-libs/vtmneg-1.0".to_string()];
        let got_pos = visible_tree_matches(&repos, "dev-libs/vtmneg", &config, &pos_one);
        assert_eq!(got_pos, vec![("1.0".to_string(), "0".to_string())]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A binary-only `slot_operator_mask_built` negative hides only the binary
    /// instance; the same-version ebuild stays visible (see [`is_binary_only_mask`]).
    #[test]
    fn visible_tree_matches_212_ignores_binary_only_masks() {
        let dir = slotundo_temp_dir("212-vtm-bin");
        let repos = repo_164(&dir, &[("dev-libs/vtmbin", "1.0", "0", "amd64", "MIT")]);
        let mut config = test_config();
        config.accept_license = vec!["*".to_string()];
        // A plain negative hides the ebuild ...
        let plain = ["!=dev-libs/vtmbin-1.0".to_string()];
        assert_eq!(
            visible_tree_matches(&repos, "dev-libs/vtmbin", &config, &plain),
            Vec::new()
        );
        // ... while the binary-marked form leaves it visible.
        let marked = ["!=dev-libs/vtmbin-1.0[binary]".to_string()];
        assert_eq!(
            visible_tree_matches(&repos, "dev-libs/vtmbin", &config, &marked),
            vec![("1.0".to_string(), "0".to_string())]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn visible_tree_matches_164_atom_still_selects_within_visible() {
        // Both versions visible here: `=...-1.0` must not leak 2.0
        // past the atom match.
        let dir = slotundo_temp_dir("164-vtm-sel");
        let repos = repo_164(
            &dir,
            &[
                ("dev-libs/vtmsel", "1.0", "0", "amd64", "MIT"),
                ("dev-libs/vtmsel", "2.0", "0", "amd64", "MIT"),
            ],
        );
        let mut config = test_config();
        config.accept_license = vec!["*".to_string()];
        let got = visible_tree_matches(&repos, "=dev-libs/vtmsel-1.0", &config, &[]);
        assert_eq!(got, vec![("1.0".to_string(), "0".to_string())]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn visible_tree_matches_164_license_masked_versions_stay_out() {
        // Portage `LicenseManager.getMissingLicenses` verdict inside the
        // visible set: 1.0 declares an unaccepted license, 2.0 an
        // accepted one.
        let dir = slotundo_temp_dir("164-vtm-lic");
        let repos = repo_164(
            &dir,
            &[
                ("dev-libs/vtmlic", "1.0", "0", "amd64", "GPL-2"),
                ("dev-libs/vtmlic", "2.0", "0", "amd64", "MIT"),
            ],
        );
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        let got = visible_tree_matches(&repos, "dev-libs/vtmlic", &config, &[]);
        assert_eq!(got, vec![("2.0".to_string(), "0".to_string())]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn visible_tree_matches_164_unparseable_atom_and_missing_package_match_nothing() {
        let dir = slotundo_temp_dir("164-vtm-empty");
        let repos = repo_164(&dir, &[("dev-libs/vtmempty", "1.0", "0", "amd64", "MIT")]);
        let config = test_config();
        let got_bad = visible_tree_matches(&repos, "][", &config, &[]);
        assert_eq!(got_bad, Vec::new());
        let got_missing = visible_tree_matches(&repos, "dev-libs/nosuchpkg", &config, &[]);
        assert_eq!(got_missing, Vec::new());
        let _ = fs::remove_dir_all(&dir);
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

    /// `masked_dep_chain` walks from a `NoVisibleCandidate`'s requirers up to
    /// the command-line argument (Portage's `_show_unsatisfied_dep` tail).
    #[test]
    fn masked_dep_chain_164_walks_to_the_argument() {
        let dir = slotundo_temp_dir("164-mdc");
        let entries = vec![
            // Decoy: merge-bound but the wrong package.
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "decoy",
                outcome: PretendOutcome::New {
                    version: "9.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
            // Decoy: NVC but the wrong package.
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "sibling",
                outcome: PretendOutcome::NoVisibleCandidate,
                required_by: Vec::new(),
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "target",
                outcome: PretendOutcome::NoVisibleCandidate,
                required_by: vec![("dev-libs".to_string(), "parent".to_string())],
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            new_164("parent", "1.0", "grandma"),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "grandma",
                outcome: PretendOutcome::New {
                    version: "1.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let atoms = ["dev-libs/grandma".to_string()];
        let got = masked_dep_chain(MaskedDepChainArgs {
            repos: &[],
            config: &portage_profile::Config::default(),
            entries: &entries,
            category: "dev-libs",
            package: "target",
            atoms: &atoms,
            argument_sets: &HashMap::new(),
            root: &dir,
            atom: "",
        });
        assert_eq!(
            got,
            vec![
                (
                    "dev-libs/parent-1.0::testrepo".to_string(),
                    "ebuild".to_string()
                ),
                (
                    "dev-libs/grandma-1.0::testrepo".to_string(),
                    "ebuild".to_string()
                ),
                ("dev-libs/grandma".to_string(), "argument".to_string()),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn masked_dep_chain_164_installed_parent_reports_the_vdb_repo() {
        // An `AlreadyInstalled` parent renders with the vdb repo and the
        // `installed` type; the same-category decoy must not shadow the parent.
        let dir = slotundo_temp_dir("164-mdc-inst");
        let entries = vec![
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "aidecoy",
                outcome: PretendOutcome::AlreadyInstalled {
                    version: "7.0".to_string(),
                },
                required_by: Vec::new(),
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "target",
                outcome: PretendOutcome::NoVisibleCandidate,
                required_by: vec![("dev-libs".to_string(), "parent".to_string())],
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "parent",
                outcome: PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string(),
                },
                required_by: vec![("dev-libs".to_string(), "grandma".to_string())],
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "grandma",
                outcome: PretendOutcome::New {
                    version: "1.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let atoms = ["dev-libs/grandma".to_string()];
        let got = masked_dep_chain(MaskedDepChainArgs {
            repos: &[],
            config: &portage_profile::Config::default(),
            entries: &entries,
            category: "dev-libs",
            package: "target",
            atoms: &atoms,
            argument_sets: &HashMap::new(),
            root: &dir,
            atom: "",
        });
        assert_eq!(
            got,
            vec![
                (
                    "dev-libs/parent-1.0::__unknown__".to_string(),
                    "installed".to_string()
                ),
                (
                    "dev-libs/grandma-1.0::testrepo".to_string(),
                    "ebuild".to_string()
                ),
                ("dev-libs/grandma".to_string(), "argument".to_string()),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn masked_dep_chain_164_argument_lines_skip_blockers_and_other_packages() {
        // `arg_lines` keeps only non-blocker atoms for this exact cp:
        // a `!`-blocker on the same cp and a plain atom on another cp
        // are not argument lines.
        let dir = slotundo_temp_dir("164-mdc-args");
        let entries = vec![
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "target",
                outcome: PretendOutcome::NoVisibleCandidate,
                required_by: vec![("dev-libs".to_string(), "parent".to_string())],
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            new_164("parent", "1.0", "grandma"),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "grandma",
                outcome: PretendOutcome::New {
                    version: "1.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let atoms = [
            "dev-libs/grandma".to_string(),
            "!dev-libs/grandma".to_string(),
            "dev-libs/unrelated".to_string(),
        ];
        let got = masked_dep_chain(MaskedDepChainArgs {
            repos: &[],
            config: &portage_profile::Config::default(),
            entries: &entries,
            category: "dev-libs",
            package: "target",
            atoms: &atoms,
            argument_sets: &HashMap::new(),
            root: &dir,
            atom: "",
        });
        assert_eq!(
            got,
            vec![
                (
                    "dev-libs/parent-1.0::testrepo".to_string(),
                    "ebuild".to_string()
                ),
                (
                    "dev-libs/grandma-1.0::testrepo".to_string(),
                    "ebuild".to_string()
                ),
                ("dev-libs/grandma".to_string(), "argument".to_string()),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
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
        let top: TopLevel<'_> = ["=dev-libs/ruparent-1.0"].into_iter().collect();
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
        let top: TopLevel<'_> = ["sys-apps/other"].into_iter().collect();
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
        let top: TopLevel<'_> = ["dev-libs/rusibling"].into_iter().collect();
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
        let top: TopLevel<'_> = ["dev-libs/rux"].into_iter().collect();
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
        let empty: TopLevel<'_> = TopLevel::from_iter([]);
        assert_eq!(
            required_use_dep_chain(&owner, &empty, &installed),
            Vec::new()
        );
    }

    /// `autounmask_dep_chain` renders the whole parent chain to the argument
    /// (Portage's `_get_dep_chain_as_comment`).
    #[test]
    fn autounmask_dep_chain_164_top_level_change_names_the_argument() {
        // No owner: a top-level change prints the argument atom as
        // given, not the change's own `>=<cpv>` left-hand form ...
        let entries: Vec<GraphEntry> = Vec::new();
        let root = Path::new("/");
        let top: TopLevel<'_> = ["dev-libs/aunone[bar]"].into_iter().collect();
        let got = autounmask_dep_chain(&None, ">=dev-libs/aunone-1.0", &top, &entries, root);
        assert_eq!(
            got,
            vec!["required by dev-libs/aunone[bar] (argument)".to_string()]
        );
        // ... or the bare atom when nothing in `top_level` matches.
        let other: TopLevel<'_> = ["sys-apps/other"].into_iter().collect();
        let got_fallback =
            autounmask_dep_chain(&None, ">=dev-libs/aunone-1.0", &other, &entries, root);
        assert_eq!(
            got_fallback,
            vec!["required by >=dev-libs/aunone-1.0 (argument)".to_string()]
        );
        // A same-category other-package atom still matches nothing:
        // both halves of the `cp` comparison must hold.
        let sibling: TopLevel<'_> = ["dev-libs/otherpkg"].into_iter().collect();
        let got_sibling =
            autounmask_dep_chain(&None, ">=dev-libs/aunone-1.0", &sibling, &entries, root);
        assert_eq!(
            got_sibling,
            vec!["required by >=dev-libs/aunone-1.0 (argument)".to_string()]
        );
    }

    /// A set member is named by its set (`@name`), like Portage's `SetArg`
    /// in `_get_dep_chain`; an atom outside the set map keeps its text (#345).
    #[test]
    fn autounmask_dep_chain_names_the_set_argument() {
        let entries: Vec<GraphEntry> = Vec::new();
        let sets: HashMap<String, String> =
            [("dev-libs/aunone".to_string(), "@myset".to_string())].into();
        let top = TopLevel {
            atoms: ["dev-libs/aunone", "sys-apps/plain"].into_iter().collect(),
            sets: &sets,
        };
        let root = Path::new("/");
        assert_eq!(
            autounmask_dep_chain(&None, ">=dev-libs/aunone-1.0", &top, &entries, root),
            vec!["required by @myset (argument)".to_string()]
        );
        assert_eq!(
            autounmask_dep_chain(&None, ">=sys-apps/plain-1.0", &top, &entries, root),
            vec!["required by sys-apps/plain (argument)".to_string()]
        );
    }

    #[test]
    fn autounmask_dep_chain_164_walks_the_whole_parent_chain() {
        let dir = slotundo_temp_dir("164-auc");
        let entries = vec![
            // Decoy: same category, wrong package -- must not win
            // the parent lookup at any hop.
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "audecoy",
                outcome: PretendOutcome::New {
                    version: "9.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "aumid",
                outcome: PretendOutcome::New {
                    version: "1.0".to_string(),
                },
                required_by: vec![("dev-libs".to_string(), "autop".to_string())],
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "autop",
                outcome: PretendOutcome::New {
                    version: "2.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let owner = Some(("dev-libs".to_string(), "aumid".to_string()));
        // A same-category sibling atom must not attach mid-chain.
        let top: TopLevel<'_> = ["dev-libs/autop", "dev-libs/ausibling"]
            .into_iter()
            .collect();
        let got = autounmask_dep_chain(&owner, "dev-libs/aumid", &top, &entries, &dir);
        assert_eq!(
            got,
            vec![
                "required by dev-libs/aumid-1.0::testrepo".to_string(),
                "required by dev-libs/autop-2.0::testrepo".to_string(),
                "required by dev-libs/autop (argument)".to_string(),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn autounmask_dep_chain_164_upgrade_parent_reports_the_to_version() {
        let dir = slotundo_temp_dir("164-auc-up");
        let entries = vec![
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "auup",
                outcome: PretendOutcome::Upgrade {
                    from: "1.0".to_string(),
                    to: "2.0".to_string(),
                },
                required_by: vec![("dev-libs".to_string(), "auuptop".to_string())],
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "auuptop",
                outcome: PretendOutcome::New {
                    version: "3.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let owner = Some(("dev-libs".to_string(), "auup".to_string()));
        let top: TopLevel<'_> = ["dev-libs/auuptop"].into_iter().collect();
        let got = autounmask_dep_chain(&owner, "dev-libs/auup", &top, &entries, &dir);
        assert_eq!(
            got,
            vec![
                "required by dev-libs/auup-2.0::testrepo".to_string(),
                "required by dev-libs/auuptop-3.0::testrepo".to_string(),
                "required by dev-libs/auuptop (argument)".to_string(),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn autounmask_dep_chain_164_installed_parent_reports_the_vdb_repo() {
        // An installed parent renders with the vdb repo; with no vdb
        // entry that repo is `__unknown__` (see `installed_pkg_repo`).
        let dir = slotundo_temp_dir("164-auc-inst");
        let entries = vec![
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "auinst",
                outcome: PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string(),
                },
                required_by: vec![("dev-libs".to_string(), "auinsttop".to_string())],
                repo: None,
                source: CandidateSource::Ebuild,
            }),
            entry_164(Entry164Args {
                category: "dev-libs",
                package: "auinsttop",
                outcome: PretendOutcome::New {
                    version: "2.0".to_string(),
                },
                required_by: Vec::new(),
                repo: Some("testrepo"),
                source: CandidateSource::Ebuild,
            }),
        ];
        let owner = Some(("dev-libs".to_string(), "auinst".to_string()));
        let top: TopLevel<'_> = ["dev-libs/auinsttop"].into_iter().collect();
        let got = autounmask_dep_chain(&owner, "dev-libs/auinst", &top, &entries, &dir);
        assert_eq!(
            got,
            vec![
                "required by dev-libs/auinst-1.0::__unknown__".to_string(),
                "required by dev-libs/auinsttop-2.0::testrepo".to_string(),
                "required by dev-libs/auinsttop (argument)".to_string(),
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn autounmask_dep_chain_164_self_requiring_parent_stops_after_its_row() {
        // A parent whose `required_by` names itself cannot ascend: the chain
        // is just its own row.
        let dir = slotundo_temp_dir("164-auc-self");
        let entries = vec![entry_164(Entry164Args {
            category: "dev-libs",
            package: "auself",
            outcome: PretendOutcome::New {
                version: "1.0".to_string(),
            },
            required_by: vec![("dev-libs".to_string(), "auself".to_string())],
            repo: Some("testrepo"),
            source: CandidateSource::Ebuild,
        })];
        let owner = Some(("dev-libs".to_string(), "auself".to_string()));
        let top: TopLevel<'_> = ["sys-apps/other"].into_iter().collect();
        let got = autounmask_dep_chain(&owner, "dev-libs/auselfdep", &top, &entries, &dir);
        assert_eq!(
            got,
            vec!["required by dev-libs/auself-1.0::testrepo".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
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
        build_id: None,
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

mod tests_195d;

mod abi_probe;
mod autounmask;
mod backtrack_params;
mod backtracker;
mod binary_index;
mod bind;
mod binpkg_use;
mod blockers;
mod candidates;
mod complete_graph;
mod config_match;
mod conflict_use_display;
mod cycles;
mod dep_validate;
mod depclean;
mod direct_solve;
mod disjunction_stack;
mod eliminate;
mod entry;
mod env_roots;
mod feedback;
mod force_scan;
mod graph;
mod greedy_slots;
mod info_pkgs;
mod installed;
mod installed_meta;
mod license;
mod masked_deps;
mod merge_bound;
mod merge_order;
mod metadata_cache;
mod pass;
mod pass_helpers;
mod pretend;
mod queue;
mod rebuild_scan;
mod reinstall;
mod repo_config;
mod resolver_trace;
mod result;
mod rev_dep;
mod root_deps;
mod skipped;
mod slot_conflict;
mod slot_undo;
mod unsat_probe;
mod updates;
mod use_display;
mod use_flags;
mod use_unsat;
mod visibility;
mod withhold;
