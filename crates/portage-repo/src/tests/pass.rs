use super::*;

/// A same-version/slot/sub-slot `Reinstall` whose installed repo differs carries
/// that instance as `oldbest` (Portage's `_get_installed_best`).
#[test]
fn run_pass_reinstall_with_only_repo_drift_carries_oldbest() {
    let dir = slotundo_temp_dir("161-run-repobest");
    let d = dir.join("var/db/pkg/dev-libs/sounneed-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("USE"), "\n").unwrap();
    // No `repository` file, so the installed repo is the `__unknown__` sentinel,
    // which never equals the `testrepo` candidate.
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: vec!["dev-libs/sounneed".to_string()],
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    let e = &pass.entries[0];
    assert!(
        matches!(
            e.outcome,
            PretendOutcome::Reinstall { ref version, .. } if version == "1.0"
        ),
        "the requested installed version reinstalls, got {:?}",
        e.outcome
    );
    assert_eq!(
        e.oldbest,
        vec![InstalledRef {
            version: "1.0".to_string(),
            slot: "0".to_string(),
            sub_slot: "1".to_string(),
            repo: "__unknown__".to_string(),
        }],
        "repo drift alone puts the installed instance in oldbest"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The `needer`/`othermod` triangle records its slot conflict on the pass.
#[test]
fn run_pass_records_the_needer_othermod_slot_conflict() {
    let fixture_root = fixtures_root();
    let atoms = vec![
        "dev-libs/needer".to_string(),
        "dev-libs/othermod".to_string(),
    ];
    let config = test_config();
    let repos = find_repos(&fixture_root).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&fixture_root, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(
        pass.slot_conflicts.iter().any(|sc| sc.package == "paired"),
        "the paired triangle records a slot conflict"
    );
    let _ = fixture_root;
}

/// Top-level atoms matched by `package.provided` ride out sorted in argument
/// order instead of resolving.
#[test]
fn run_pass_sorts_provided_atoms_in_argument_order() {
    let dir = slotundo_temp_dir("161-run-prov");
    let atoms = vec![
        "dev-libs/paired".to_string(),
        "dev-libs/sounneed".to_string(),
    ];
    let mut config = test_config();
    config.package_provided = vec![
        "dev-libs/paired-1.0".to_string(),
        "dev-libs/sounneed-1.0".to_string(),
    ];
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(pass.entries.is_empty());
    assert_eq!(pass.pprovided_atoms, atoms);
    let _ = fs::remove_dir_all(&dir);
}

/// A masked candidate reports through the masked-dep channel.
#[test]
fn run_pass_reports_a_masked_candidate() {
    let dir = slotundo_temp_dir("161-run-masked");
    let atoms = vec!["dev-libs/hardmaskedpkg".to_string()];
    let mut config = test_config();
    config.package_mask = vec!["dev-libs/hardmaskedpkg".to_string()];
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::NoVisibleCandidate
    ));
    assert!(
        pass.masked_deps
            .iter()
            .any(|r| r.category == "dev-libs" && r.package == "hardmaskedpkg"),
        "the masked candidate is reported"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A REQUIRED_USE violation fails the walk with no entries at all.
#[test]
fn run_pass_fails_a_required_use_violation() {
    let dir = slotundo_temp_dir("161-run-requse");
    let atoms = vec!["dev-libs/requiredusebadpkg".to_string()];
    let mut config = test_config();
    config.package_use = vec![(
        "dev-libs/requiredusebadpkg".to_string(),
        vec!["foo".to_string()],
    )];
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let Err(err) = run_pass(&ctx, &BacktrackParams::default(), true) else {
        panic!("expected a REQUIRED_USE violation");
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("foo") && msg.contains("bar"),
        "the violation names both flags, got: {msg}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `--with-test-deps` walks a top-level atom's `test?` conditional deps.
#[test]
fn run_pass_with_test_deps_walks_test_conditionals() {
    let dir = slotundo_temp_dir("161-run-testdeps");
    let atoms = vec!["dev-libs/withtestdeppkg".to_string()];
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let walk = |with_test_deps: bool| {
        let opts = CtxOpts161 {
            backtrack_max: 10,
            atoms: atoms.clone(),
            with_test_deps,
            ..Default::default()
        };
        let ctx = ctx_161(&dir, &config, repos.clone(), &opts);
        run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles")
    };
    let plain = walk(false);
    assert!(
        !plain.entries.iter().any(|e| e.package == "testonlydep"),
        "test? deps stay out without the flag"
    );
    let testing = walk(true);
    assert!(
        testing.entries.iter().any(|e| e.package == "testonlydep"),
        "test? deps walk with the flag"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// In complete mode a dependency outside the locked merge set resolves to
/// nothing.
#[test]
fn run_pass_complete_mode_locks_unlisted_dependencies_out() {
    let dir = slotundo_temp_dir("161-run-complete");
    let atoms = vec!["dev-libs/needer".to_string()];
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        complete: true,
        locked: HashSet::from([("dev-libs".to_string(), "otherpkg".to_string())]),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(
        pass.entries.iter().any(|e| e.package == "needer"),
        "the requested atom still resolves"
    );
    assert!(
        !pass.entries.iter().any(|e| e.package == "paired"),
        "an unlocked dependency resolves to nothing in complete mode"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// An unlimited deep walk recurses into an installed dependency's own deps.
#[test]
fn run_pass_deep_walk_recurses_into_installed_deps() {
    let dir = slotundo_temp_dir("161-run-deep");
    let mk = |name: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
    };
    mk("deeppkg-1.0");
    let atoms = vec!["dev-libs/completegraphpkg".to_string()];
    let walk = |deep: Deep| {
        let config = test_config();
        let repos = find_repos(&fixtures_root()).expect("fixture repos");
        let opts = CtxOpts161 {
            backtrack_max: 10,
            atoms: atoms.clone(),
            deep,
            ..Default::default()
        };
        let ctx = ctx_161(&dir, &config, repos, &opts);
        run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles")
    };
    let shallow = walk(Deep::NotRequested);
    let mut names: Vec<&str> = shallow.entries.iter().map(|e| e.package.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["completegraphpkg", "deeppkg"]);
    let deep = walk(Deep::Unlimited);
    let mut names: Vec<&str> = deep.entries.iter().map(|e| e.package.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["completegraphpkg", "deeppkg", "deeppkg2", "newpkg"]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Under `--usepkg` with a staged binary index, the atom resolves from the
/// binary pool instead of the tree.
#[test]
fn run_pass_usepkg_selects_the_staged_binary() {
    let dir = slotundo_temp_dir("161-run-binpkg");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/sounneed-1.0".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
        ("BUILD_ID".to_string(), "abc123".to_string()),
    ])]);
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkg: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(
        pass.entries[0].source,
        CandidateSource::Binary,
        "the staged binary wins over the ebuild"
    );
    assert_eq!(pass.entries[0].build_id.as_deref(), Some("abc123"));
    let _ = fs::remove_dir_all(&dir);
}

/// A solved in-walk slot conflict records its skipped update. Seed order
/// matters: `newconsumer` first pulls 2.0, then `oldconsumer` collides it.
#[test]
fn run_pass_records_skipped_updates_for_solved_conflicts() {
    let fixture_root = fixtures_root();
    let atoms = vec![
        "dev-libs/slotconflictoldconsumer".to_string(),
        "dev-libs/slotconflictnewconsumer".to_string(),
    ];
    let config = test_config();
    let repos = find_repos(&fixture_root).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&fixture_root, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(
        pass.skipped_updates
            .iter()
            .any(|s| s.skipped_version == "2.0"),
        "the dropped 2.0 rides out as a skipped update"
    );
    let _ = fixture_root;
}

/// Two owners blocking the same package file two rows: the dedup keys on the
/// full triple, so a second owner is never swallowed.
#[test]
fn run_pass_files_one_row_per_blocker_owner() {
    let dir = slotundo_temp_dir("161-run-blk2");
    let repo = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/blockerparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/victim !dev-libs/victim",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo.location,
        cp: "dev-libs/victim",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo.location,
        cp: "dev-libs/blockerparent2",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/victim !dev-libs/victim",
    });
    let d = dir.join("var/db/pkg/dev-libs/victim-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    fs::write(d.join("USE"), "\n").unwrap();
    let config = test_config();
    let atoms = vec![
        "dev-libs/blockerparent".to_string(),
        "dev-libs/blockerparent2".to_string(),
    ];
    let repos = vec![repo];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let rows: Vec<&str> = pass
        .entries
        .iter()
        .filter(|e| e.package == "blockerparent" || e.package == "blockerparent2")
        .filter(|e| !e.blockers.is_empty())
        .map(|e| e.package.as_str())
        .collect();
    assert_eq!(rows.len(), 2);
    let _ = fs::remove_dir_all(&dir);
}

/// With `--binpkg-respect-use`, a USE-mismatched binary is filtered out and
/// the ebuild wins instead.
#[test]
fn run_pass_respect_use_filters_a_stale_binary() {
    let dir = slotundo_temp_dir("161-run-respect");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let mut config = test_config();
    // The staged binary was built with +soflag; the tree resolves
    // it off.
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/sounneed-1.0".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
        ("USE".to_string(), "soflag".to_string()),
        ("IUSE".to_string(), "soflag".to_string()),
    ])]);
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkg: true,
        binpkg_respect_use: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(
        pass.entries[0].source,
        CandidateSource::Ebuild,
        "the USE-mismatched binary is filtered out"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// An older staged binary loses to the newer tree ebuild under `--usepkg`.
#[test]
fn run_pass_newer_ebuild_beats_older_binary() {
    let dir = slotundo_temp_dir("161-run-oldbin");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/sounneed-0.9".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
    ])]);
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkg: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(
        pass.entries[0].source,
        CandidateSource::Ebuild,
        "the newer tree ebuild wins"
    );
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::New { ref version } if version == "1.0"
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// An auto-replace seed is not a top-level argument: it walks ownerless at
/// depth 1 and never files an `(Argument)` puller row.
#[test]
fn run_pass_replace_seed_files_no_argument_puller() {
    let dir = slotundo_temp_dir("161-run-seed");
    let d = dir.join("var/db/pkg/dev-libs/sounneed-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    fs::write(d.join("USE"), "\n").unwrap();
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let atoms: Vec<String> = Vec::new();
    let mut bp = BacktrackParams::default();
    bp.slot_operator_replace_installed
        .insert(("dev-libs".to_string(), "sounneed".to_string()));
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &bp, true).expect("walk settles");
    assert!(
        pass.slot_pullers.is_empty(),
        "a lone seed files no Argument pullers"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Under `--usepkgonly`, a newer staged binary merges even with no ebuild twin
/// at its version.
#[test]
fn run_pass_usepkgonly_takes_a_newer_binary() {
    let dir = slotundo_temp_dir("161-run-newbin");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/sounneed-9.9".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
    ])]);
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkgonly: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(pass.entries[0].source, CandidateSource::Binary);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::New { ref version } if version == "9.9"
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Under plain `--usepkg`, a binary with no ebuild twin at its version loses
/// to the visible tree.
#[test]
fn run_pass_newer_binary_loses_to_the_tree() {
    let dir = slotundo_temp_dir("161-run-ebwin");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/sounneed-9.9".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
    ])]);
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkg: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(pass.entries[0].source, CandidateSource::Ebuild);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::New { ref version } if version == "1.0"
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// A keyword-masked tree ebuild does not count as an ebuild twin.
#[test]
fn run_pass_masked_ebuild_is_no_twin() {
    let dir = slotundo_temp_dir("161-run-maskbin");
    let atoms = vec!["dev-libs/kwonlypkg".to_string()];
    // Tree ebuild ~amd64 (masked under the amd64 accept); staged
    // binary amd64 (visible).
    let repo = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/kwonlypkg",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    let d = repo.location.join("dev-libs/kwonlypkg");
    let body = "EAPI=8\nDESCRIPTION=\"161 kw\"\nSLOT=\"0\"\nKEYWORDS=\"~amd64\"\n";
    std::fs::write(d.join("kwonlypkg-1.0.ebuild"), body).unwrap();
    use md5::Digest as _;
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let entry = format!(
        "DEFINED_PHASES=-\nDESCRIPTION=161 kw\nEAPI=8\nKEYWORDS=~amd64\nSLOT=0\n_md5_={md5}\n"
    );
    let cachedir = repo.location.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cachedir).unwrap();
    std::fs::write(cachedir.join("kwonlypkg-1.0"), entry).unwrap();
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/kwonlypkg-1.0".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
    ])]);
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkgonly: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repo], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(pass.entries[0].source, CandidateSource::Binary);
    let _ = fs::remove_dir_all(&dir);
}
