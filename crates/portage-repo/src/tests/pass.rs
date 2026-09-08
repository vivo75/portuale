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
