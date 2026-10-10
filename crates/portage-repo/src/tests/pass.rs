use super::*;

/// B1 bug 1: the dependency strings of a chosen binary instance are
/// read from the first `Packages` entry with the same `CPV`, not from
/// the instance that was actually selected. Here build 2 (newest)
/// wins `dedup_binary_instances`, but the walk follows build 1's
/// `RDEPEND` (`dev-libs/olddep`) -- in the field that is
/// `ghostscript-gpl:0/10.06=`, which forces a downgrade of the
/// installed 10.08. Expected value: real `_iter_match_pkgs` +
/// `pkg._metadata` of the selected instance (the `Packages` entry
/// carrying that `BUILD_ID`).
#[test]
fn multi_instance_walk_reads_the_selected_instances_own_deps() {
    let dir = slotundo_temp_dir("1010-multiinst-deps");
    let repo = dir.join("scratchrepo");
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo,
        cp: "dev-libs/spectre",
        pv: "0.2.12",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/newdep",
    });
    for cp in ["dev-libs/olddep", "dev-libs/newdep"] {
        blocker_161_write_pkg(Blocker161WritePkgArgs {
            repo: &repo,
            cp,
            pv: "1.0",
            slot: "0",
            depend: "",
            rdepend: "",
        });
    }
    let repos = vec![blocker_161_repo_config(repo)];
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![
        multi_instance_entry_1010("1", 1000, "dev-libs/olddep"),
        multi_instance_entry_1010("2", 2000, "dev-libs/newdep"),
    ]);
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: vec!["dev-libs/spectre".to_string()],
        usepkg: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let parent = pass
        .entries
        .iter()
        .find(|e| e.package == "spectre")
        .expect("the parent merges");
    assert_eq!(parent.build_id.as_deref(), Some("2"), "newest build wins");
    let names: Vec<&str> = pass.entries.iter().map(|e| e.package.as_str()).collect();
    assert!(
        names.contains(&"newdep"),
        "build 2's own RDEPEND must be walked, got {names:?}"
    );
    assert!(
        !names.contains(&"olddep"),
        "build 1's RDEPEND must not leak into build 2's walk, got {names:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #247: a same-version/slot/sub-slot `Reinstall` whose
/// installed instance's repo differs from the merged candidate's
/// carries that instance as `oldbest` -- real
/// `_get_installed_best`'s third disjunct (`not quiet_repo_display
/// and repo differs`, `output.py:721-731`; owner B5 reopened the
/// cut). The staged vdb omits the `repository` file (like the
/// hand-staged abk0/samepkg fixture vdbs), so the installed repo
/// is real's `__unknown__` sentinel while the candidate merges
/// from `testrepo`.
#[test]
fn run_pass_reinstall_with_only_repo_drift_carries_oldbest() {
    let dir = slotundo_temp_dir("161-run-repobest");
    let d = dir.join("var/db/pkg/dev-libs/sounneed-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("USE"), "\n").unwrap();
    // No `repository` file: `installed_pkg_repo` falls back to
    // `__unknown__`, which never equals the `testrepo` candidate.
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

/// Backlog #161 S6: the `needer`/`othermod` triangle records its
/// slot conflict on the pass instead of resolving silently.
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

/// Backlog #161 S6: a runtime blocker on an installed package
/// that stays put files a satisfied blocker row on it.
#[test]
fn run_pass_files_a_depend_blocker_row() {
    let dir = slotundo_temp_dir("161-run-blk");
    let repos = blocker_161_repo(&dir);
    // victim-1.0 stays installed while the parent merges around
    // it under a soft block.
    let d = dir.join("var/db/pkg/dev-libs/victim-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    fs::write(d.join("USE"), "\n").unwrap();
    let config = test_config();
    let atoms = vec!["dev-libs/blockerparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    // The soft block files on the merging parent; the installed
    // victim gains the uninstall-model row beside its installed one.
    let parent = pass
        .entries
        .iter()
        .find(|e| e.package == "blockerparent")
        .expect("parent merges");
    assert_eq!(parent.blockers.len(), 1);
    assert_eq!(parent.blockers[0].atom_str, "!dev-libs/victim");
    assert!(!parent.blockers[0].strong);
    assert_eq!(parent.blockers[0].matched_version, "1.0");
    assert!(
        pass.entries
            .iter()
            .any(|e| e.package == "victim"
                && matches!(e.outcome, PretendOutcome::Uninstall { .. })),
        "the blocked installed instance gains an uninstall row"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: under `--nodeps` the all-installed blocker
/// scan never runs -- an installed owner's blocker stays invisible
/// even when its target upgrades right beside it.
#[test]
fn run_pass_nodeps_skips_the_unwalked_blocker_scan() {
    let dir = slotundo_temp_dir("161-run-nodepsblk");
    // Unwalked installed owner blocking `scanvictim`, which this
    // run upgrades 1.0 -> 2.0 (so a scan would find a merge-bound
    // match and file rows).
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    // Unwalked installed owner blocking `scanvictim`, which this
    // run upgrades 1.0 -> 2.0 (so a scan would find a merge-bound
    // match and file rows). Unversioned: an op-less versioned
    // blocker is ambiguous and rejected on both sides (real
    // `Atom('!dev-libs/scanvictim-1.0')` raises too).
    mk("scanowner-1.0", "0", "!dev-libs/scanvictim");
    mk("scanvictim-1.0", "0", "");
    let scratch = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/scanvictim",
        pv: "2.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    let config = test_config();
    let mut repos = find_repos(&fixtures_root()).expect("fixture repos");
    repos.push(scratch);
    let atoms = vec!["dev-libs/scanvictim".to_string()];
    // The owner rides the required-set closure so a found blocker
    // has a display home (the #73 set-parent arm); without the
    // `--nodeps` gate the scan would file rows through it.
    let owner_closure: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "scanowner".to_string())]);
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        update: true,
        nodeps: true,
        blocker_closure: owner_closure,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let victim = pass
        .entries
        .iter()
        .find(|e| e.package == "scanvictim")
        .expect("victim upgrades");
    assert!(matches!(victim.outcome, PretendOutcome::Upgrade { .. }));
    assert!(
        victim.blockers.is_empty(),
        "no scan runs under --nodeps, so no blocker rows file"
    );
    assert!(pass.orphan_blockers.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: top-level atoms matched by `package.provided`
/// skip resolution and ride out sorted in argument order.
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

/// Backlog #161 S6: a masked candidate reports through the
/// masked-dep channel instead of resolving.
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

/// Backlog #161 S6: a REQUIRED_USE violation fails the walk with
/// no entries at all (its own ebuild documents the trigger: `foo`
/// forced on via `package.use`, `bar` left off).
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

/// Backlog #161 S6: `--with-test-deps` walks a top-level atom's
/// `test?` conditional deps; without it they stay out.
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

/// Backlog #161 S6: a dependency nothing satisfies -- not even
/// its bare form -- arms the missing-dep retry trigger.
/// (A USE-only failure instead stays out of this path -- its bare
/// form still satisfies the probe -- so the shape uses a fully
/// missing package.)
#[test]
fn run_pass_arms_the_missing_dep_trigger() {
    let dir = slotundo_temp_dir("161-run-missdep");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/nosuchdep",
    });
    let config = test_config();
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let mut pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let _ = pass.missing_dep_trigger.take().expect("trigger armed");
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a repeated NVC under active search masks is
/// flagged suppressed (the #57 surely-masked shape: the second
/// visit dedups, and the mask gate marks the pass a dead end);
/// without masks the dep merges and nothing suppresses.
#[test]
fn run_pass_flags_nvc_under_masks_suppressed() {
    let dir = slotundo_temp_dir("161-run-suppr");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/victim2 dev-libs/victim2:0",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repos.location,
        cp: "dev-libs/victim2",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    let config = test_config();
    let atoms = vec!["dev-libs/depparent".to_string()];
    let walk = |masks: bool| {
        let mut bp = BacktrackParams::default();
        if masks {
            bp.runtime_pkg_mask.insert(
                ("dev-libs".to_string(), "victim2".to_string()),
                vec![MaskEntry {
                    neg: "!=dev-libs/victim2-1.0".to_string(),
                    reason: MaskReason::MissingDependency {
                        parent: ("dev-libs".to_string(), "depparent".to_string()),
                        atom: "dev-libs/victim2".to_string(),
                    },
                }],
            );
        }
        let opts = CtxOpts161 {
            backtrack_max: 10,
            atoms: atoms.clone(),
            ..Default::default()
        };
        let ctx = ctx_161(&dir, &config, vec![repos.clone()], &opts);
        run_pass(&ctx, &bp, true).expect("walk settles")
    };
    let plain = walk(false);
    assert!(
        plain
            .entries
            .iter()
            .any(|e| e.package == "victim2" && matches!(e.outcome, PretendOutcome::New { .. })),
        "unmasked victim2 merges"
    );
    assert!(!plain.suppressed_nvc);
    assert!(walk(true).suppressed_nvc);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a dependency whose candidates are all
/// masked reports once through the masked-dep channel, however
/// many dep edges name it.
#[test]
fn run_pass_reports_a_masked_dependency_once() {
    let dir = slotundo_temp_dir("161-run-depmask");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/victim2 dev-libs/victim2:0",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repos.location,
        cp: "dev-libs/victim2",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    let mut config = test_config();
    config.package_mask = vec!["dev-libs/victim2".to_string()];
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let rows: Vec<&str> = pass
        .masked_deps
        .iter()
        .map(|r| r.package.as_str())
        .collect();
    assert_eq!(rows, vec!["victim2"]);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: in complete mode a dependency outside the
/// locked merge set resolves to nothing -- the cheap
/// graph-or-installed check instead of a fresh resolve.
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

/// Backlog #161 S6: an unlimited deep walk recurses into an
/// installed dependency's own deps; without it they stay out.
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

/// Backlog #161 S6: under `--usepkg` with a staged binary index,
/// the atom resolves from the binary pool instead of the tree,
/// carrying the index's build identity.
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

/// Backlog #161 S6: a binary candidate's build-time keys stay out
/// of the walk without `--with-bdeps`.
#[test]
fn run_pass_binary_hides_buildtime_deps_without_bdeps() {
    let dir = slotundo_temp_dir("161-run-binbdep");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/binparent",
        pv: "1.0",
        slot: "0",
        depend: "dev-libs/buildonlydep",
        rdepend: "dev-libs/runtimedep",
    });
    for (cp, pv) in [
        ("dev-libs/buildonlydep", "1.0"),
        ("dev-libs/runtimedep", "1.0"),
    ] {
        blocker_161_write_pkg(Blocker161WritePkgArgs {
            repo: &repos.location,
            cp,
            pv,
            slot: "0",
            depend: "",
            rdepend: "",
        });
    }
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/binparent-1.0".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
        ("BDEPEND".to_string(), "dev-libs/buildonlydep".to_string()),
        ("RDEPEND".to_string(), "dev-libs/runtimedep".to_string()),
    ])]);
    let atoms = vec!["dev-libs/binparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkg: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let names: Vec<&str> = pass.entries.iter().map(|e| e.package.as_str()).collect();
    assert!(names.contains(&"binparent"));
    assert!(names.contains(&"runtimedep"));
    assert!(
        !names.contains(&"buildonlydep"),
        "build-time keys stay out without --with-bdeps"
    );
    // The blanking twin: the recorded merge-order edges skip the
    // build-time dep too.
    let parent = pass
        .entries
        .iter()
        .find(|e| e.package == "binparent")
        .expect("parent merges");
    assert!(
        !parent.deps.iter().any(|d| d.package == "buildonlydep"),
        "the blanked build-time dep stays out of the edges too"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: two plain-missing deps on one cp file a single
/// plain-miss report.
#[test]
fn run_pass_dedups_plain_miss_reports() {
    let dir = slotundo_temp_dir("161-run-missdup");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/nosuchdep dev-libs/nosuchdep:0",
    });
    let config = test_config();
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.plain_miss_deps.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a solved in-walk slot conflict records its
/// skipped update for the withhold notice. Seed order matters:
/// `newconsumer` first pulls 2.0, then `oldconsumer` collides it
/// with 1.0.
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

/// Backlog #161 S6: the missing-dep trigger names the upgraded
/// parent version (the `Upgrade` arm, not just `New`).
#[test]
fn run_pass_arms_the_trigger_for_an_upgrading_parent() {
    let dir = slotundo_temp_dir("161-run-misup");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparentup",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/nosuchdep",
    });
    let d = dir.join("var/db/pkg/dev-libs/depparentup-0.9");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    fs::write(d.join("USE"), "\n").unwrap();
    let config = test_config();
    let atoms = vec!["dev-libs/depparentup".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        update: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let mut pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let trigger = pass.missing_dep_trigger.take().expect("trigger armed");
    assert_eq!(trigger.1, "!=dev-libs/depparentup-1.0");
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #286: a top-level set member (`@world`/`@selected`
/// expansion, real `SetArg` "world"/"selected") that selects no
/// package drops into `missing_args` instead of failing the walk
/// (real `_missing_args`, `depgraph.py:5577-5605`); the same atom
/// requested explicitly (no set membership) fails with the bare
/// miss. The drop leaves no entry, so the healed rest of the graph
/// can settle around it.
#[test]
fn run_pass_drops_an_unresolvable_set_member() {
    let dir = slotundo_temp_dir("286-run-missing-arg");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/someother",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    let config = test_config();
    let atoms = vec!["dev-libs/nosuchpkg".to_string()];
    let walk = |from_set: bool| {
        let opts = CtxOpts161 {
            backtrack_max: 10,
            atoms: atoms.clone(),
            top_level: atoms.clone(),
            set_args: if from_set {
                HashSet::from(["dev-libs/nosuchpkg".to_string()])
            } else {
                HashSet::new()
            },
            ..Default::default()
        };
        let ctx = ctx_161(&dir, &config, vec![repos.clone()], &opts);
        run_pass(&ctx, &BacktrackParams::default(), true)
    };
    let Err(err) = walk(false) else {
        panic!("an explicit miss fails the walk");
    };
    assert!(
        format!("{err}").contains("there are no ebuilds to satisfy"),
        "explicit miss keeps the bare report, got: {err}"
    );
    let pass = walk(true).expect("a set member drops instead of failing");
    assert_eq!(pass.missing_args, vec!["dev-libs/nosuchpkg".to_string()]);
    assert!(
        pass.entries.is_empty(),
        "the dropped member leaves no entry, saw {:?}",
        pass.entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect::<Vec<_>>()
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: one report per missing cp, even across
/// categories sharing a package name.
#[test]
fn run_pass_reports_each_plain_miss_once() {
    let dir = slotundo_temp_dir("161-run-miss4");
    // Four missing deps: two sharing dev-libs, two sharing the
    // package name across categories.
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/nosucha dev-libs/nosuchb dev-libs/dupmiss sys-libs/dupmiss",
    });
    let config = test_config();
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let mut rows: Vec<String> = pass
        .plain_miss_deps
        .iter()
        .map(|r| format!("{}/{}", r.category, r.package))
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        vec![
            "dev-libs/dupmiss",
            "dev-libs/nosucha",
            "dev-libs/nosuchb",
            "sys-libs/dupmiss",
        ]
    );
    // Backtrack disabled: no missing-dep retry arms either.
    assert!(pass.missing_dep_trigger.is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a repeated missing dep without active masks is
/// not suppressed -- suppression needs the mask gate.
#[test]
fn run_pass_repeated_miss_without_masks_is_not_suppressed() {
    let dir = slotundo_temp_dir("161-run-nosuppr");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/nosuchdep dev-libs/nosuchdep:0",
    });
    let config = test_config();
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(!pass.suppressed_nvc);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: one masked report per masked cp, even
/// across categories sharing a package name.
#[test]
fn run_pass_reports_each_masked_dep_once() {
    let dir = slotundo_temp_dir("161-run-mask4");
    let repo = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/masked1 dev-libs/masked2 dev-libs/dupmask sys-libs/dupmask",
    });
    for (cp, pv) in [
        ("dev-libs/masked1", "1.0"),
        ("dev-libs/masked2", "1.0"),
        ("dev-libs/dupmask", "1.0"),
        ("sys-libs/dupmask", "1.0"),
    ] {
        blocker_161_write_pkg(Blocker161WritePkgArgs {
            repo: &repo.location,
            cp,
            pv,
            slot: "0",
            depend: "",
            rdepend: "",
        });
    }
    let mut config = test_config();
    config.package_mask = vec![
        "dev-libs/masked1".to_string(),
        "dev-libs/masked2".to_string(),
        "dev-libs/dupmask".to_string(),
        "sys-libs/dupmask".to_string(),
    ];
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repo], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let mut rows: Vec<String> = pass
        .masked_deps
        .iter()
        .map(|r| format!("{}/{}", r.category, r.package))
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        vec![
            "dev-libs/dupmask",
            "dev-libs/masked1",
            "dev-libs/masked2",
            "sys-libs/dupmask",
        ]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: one use-unsat report per unsatisfiable cp,
/// even across categories sharing a package name.
#[test]
fn run_pass_reports_each_use_unsat_once() {
    let dir = slotundo_temp_dir("161-run-use4");
    let repo = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/unsat1[soflag] dev-libs/unsat2[soflag] dev-libs/dupuse[soflag] sys-libs/dupuse[soflag]",
    });
    for (cp, pv) in [
        ("dev-libs/unsat1", "1.0"),
        ("dev-libs/unsat2", "1.0"),
        ("dev-libs/dupuse", "1.0"),
        ("sys-libs/dupuse", "1.0"),
    ] {
        let d = repo
            .location
            .join(cp.split_once('/').unwrap().0)
            .join(cp.split_once('/').unwrap().1);
        std::fs::create_dir_all(&d).unwrap();
        let pkg = cp.split_once('/').unwrap().1;
        let body = "EAPI=8\nDESCRIPTION=\"161 useunsat\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nIUSE=\"soflag\"\n";
        std::fs::write(d.join(format!("{pkg}-{pv}.ebuild")), body).unwrap();
        use md5::Digest as _;
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let entry = format!(
            "DEFINED_PHASES=-\nDESCRIPTION=161 useunsat\nEAPI=8\nIUSE=soflag\nKEYWORDS=amd64\nSLOT=0\n_md5_={md5}\n"
        );
        let cachedir = repo
            .location
            .join("metadata/md5-cache")
            .join(cp.split_once('/').unwrap().0);
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    }
    let config = test_config();
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repo], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let mut rows: Vec<String> = pass
        .use_unsat_deps
        .iter()
        .map(|r| format!("{}/{}", r.category, r.package))
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        vec![
            "dev-libs/dupuse",
            "dev-libs/unsat1",
            "dev-libs/unsat2",
            "sys-libs/dupuse",
        ]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: two owners blocking the same package file two
/// rows -- the pending dedup keys on the full triple, so a second
/// owner is never swallowed.
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

/// Backlog #161 S6: with `--with-bdeps` a binary candidate's
/// build-time keys walk too.
#[test]
fn run_pass_binary_walks_buildtime_deps_with_bdeps() {
    let dir = slotundo_temp_dir("161-run-binbdep2");
    // NOTE (repaired S6p): the first version of this leg built the
    // tree parent with `DEPEND="dev-libs/buildonlydep"` (the only
    // key the helper could write) while staging the binary with
    // `BDEPEND="dev-libs/buildonlydep"`. Under `--with-bdeps`
    // `--binpkg-changed-deps` compares all five keys, saw the
    // `DEPEND` mismatch, and rejected the binary -- so the leg
    // walked the ebuild and proved nothing about the
    // `Binary && !with_bdeps` gates. The parent now carries the
    // dep in `BDEPEND` on both sides, so the binary survives
    // selection and the walk genuinely takes the all-keys arm.
    let repo = dir.join("scratchrepo");
    blocker_161_write_pkg_full(Blocker161WritePkgFullArgs {
        repo: &repo,
        cp: "dev-libs/binparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/runtimedep",
        bdepend: "dev-libs/buildonlydep",
    });
    for (cp, pv) in [
        ("dev-libs/buildonlydep", "1.0"),
        ("dev-libs/runtimedep", "1.0"),
    ] {
        blocker_161_write_pkg(Blocker161WritePkgArgs {
            repo: &repo,
            cp,
            pv,
            slot: "0",
            depend: "",
            rdepend: "",
        });
    }
    let repos = vec![blocker_161_repo_config(repo)];
    let mut config = test_config();
    config.scanned_binpkgs = Some(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/binparent-1.0".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "testrepo".to_string()),
        ("BDEPEND".to_string(), "dev-libs/buildonlydep".to_string()),
        ("RDEPEND".to_string(), "dev-libs/runtimedep".to_string()),
    ])]);
    let atoms = vec!["dev-libs/binparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        usepkg: true,
        with_bdeps: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let names: Vec<&str> = pass.entries.iter().map(|e| e.package.as_str()).collect();
    assert!(names.contains(&"binparent"));
    assert!(names.contains(&"runtimedep"));
    assert!(
        names.contains(&"buildonlydep"),
        "a selected binary walks its build-time keys under --with-bdeps"
    );
    // The all-keys arm also feeds the merge-order digraph: the
    // parent's recorded edges name the build-time dep.
    let parent = pass
        .entries
        .iter()
        .find(|e| e.package == "binparent")
        .expect("parent merges");
    assert!(
        parent.deps.iter().any(|d| d.package == "buildonlydep"),
        "the binary's build-time dep reaches the merge-order edges"
    );
    // ... and the queue tags that edge build-time-hard (no
    // installed provider, no run-time alternative), which is what
    // the `buildtime_atoms` gate computes.
    assert_eq!(
        pass.edge_kind_map.get(&(
            (false, "dev-libs".to_string(), "buildonlydep".to_string()),
            (false, "dev-libs".to_string(), "binparent".to_string())
        )),
        Some(&(true, false)),
        "the build-time edge is hard, not soft"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6p: under `--buildpkgonly` (without `--deep`)
/// a source candidate walks only its build-time keys -- the
/// `dep_keys` second arm (`candidate_source != Binary`). The
/// scratch parent carries the same dep in `DEPEND` (walked) and
/// a second one in `RDEPEND` (blanked).
#[test]
fn run_pass_buildpkgonly_walks_buildtime_keys_only() {
    let dir = slotundo_temp_dir("161-run-bpkgonly");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/bpkgparent",
        pv: "1.0",
        slot: "0",
        depend: "dev-libs/builddep",
        rdepend: "dev-libs/rundep",
    });
    for (cp, pv) in [("dev-libs/builddep", "1.0"), ("dev-libs/rundep", "1.0")] {
        blocker_161_write_pkg(Blocker161WritePkgArgs {
            repo: &repos.location,
            cp,
            pv,
            slot: "0",
            depend: "",
            rdepend: "",
        });
    }
    let config = test_config();
    let atoms = vec!["dev-libs/bpkgparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        buildpkgonly: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repos], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let names: Vec<&str> = pass.entries.iter().map(|e| e.package.as_str()).collect();
    assert!(names.contains(&"bpkgparent"));
    assert!(
        names.contains(&"builddep"),
        "build-time keys still walk under --buildpkgonly"
    );
    assert!(
        !names.contains(&"rundep"),
        "run-time keys are blanked under --buildpkgonly"
    );
    let parent = pass
        .entries
        .iter()
        .find(|e| e.package == "bpkgparent")
        .expect("parent merges");
    assert!(
        parent.deps.iter().any(|d| d.package == "builddep"),
        "the kept build-time dep reaches the merge-order edges"
    );
    assert!(
        !parent.deps.iter().any(|d| d.package == "rundep"),
        "the blanked run-time dep stays out of the edges too"
    );
    // ... and the queue tags the kept dep build-time-hard (no
    // installed provider, no run-time alternative).
    assert_eq!(
        pass.edge_kind_map.get(&(
            (false, "dev-libs".to_string(), "builddep".to_string()),
            (false, "dev-libs".to_string(), "bpkgparent".to_string())
        )),
        Some(&(true, false)),
        "the build-time edge is hard, not soft"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: sibling report vecs do not shadow other
/// cps' misses -- visits run reverse-RDEPEND-text, so the plain
/// filings land last with every sibling vec populated.
#[test]
fn run_pass_plain_miss_ignores_other_cps_reports() {
    let dir = slotundo_temp_dir("161-run-misscombo");
    let repo = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/plainmiss sys-libs/dupmiss dev-libs/dupmiss[soflag] dev-libs/unsatone[soflag] dev-libs/maskedone",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo.location,
        cp: "dev-libs/maskedone",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "",
    });
    for (cp, pv) in [("dev-libs/unsatone", "1.0"), ("dev-libs/dupmiss", "1.0")] {
        let (cat, pkg) = cp.split_once('/').unwrap();
        let d = repo.location.join(cat).join(pkg);
        std::fs::create_dir_all(&d).unwrap();
        let body =
            "EAPI=8\nDESCRIPTION=\"161 combo\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nIUSE=\"soflag\"\n";
        std::fs::write(d.join(format!("{pkg}-{pv}.ebuild")), body).unwrap();
        use md5::Digest as _;
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let entry = format!(
            "DEFINED_PHASES=-\nDESCRIPTION=161 combo\nEAPI=8\nIUSE=soflag\nKEYWORDS=amd64\nSLOT=0\n_md5_={md5}\n"
        );
        let cachedir = repo.location.join("metadata/md5-cache").join(cat);
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    }
    let mut config = test_config();
    config.package_mask = vec!["dev-libs/maskedone".to_string()];
    let atoms = vec!["dev-libs/depparent".to_string()];
    let opts = CtxOpts161 {
        backtrack_max: 0,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repo], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert_eq!(pass.masked_deps.len(), 1);
    assert_eq!(pass.use_unsat_deps.len(), 2);
    let mut plain: Vec<String> = pass
        .plain_miss_deps
        .iter()
        .map(|r| format!("{}/{}", r.category, r.package))
        .collect();
    plain.sort();
    assert_eq!(plain, vec!["dev-libs/plainmiss", "sys-libs/dupmiss"]);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: with `--binpkg-respect-use`, a binary
/// whose USE disagrees with the tree is filtered out and the
/// ebuild wins instead.
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

/// Backlog #161 S6: an older staged binary loses to the newer tree
/// ebuild under `--usepkg`.
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

/// Backlog #161 S6: a bare `:=` pull landing on a resolved slot
/// reuses it silently -- the satisfaction holds through the
/// operator and binding arms when nothing rebuilt shifts.
#[test]
fn run_pass_bare_slotop_pull_reuses_the_resolved_slot() {
    let dir = slotundo_temp_dir("161-run-slotop");
    let repo = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/consumerb",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/prov",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo.location,
        cp: "dev-libs/prov",
        pv: "1.0",
        slot: "0/1",
        depend: "",
        rdepend: "",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo.location,
        cp: "dev-libs/prov",
        pv: "2.0",
        slot: "0/2",
        depend: "",
        rdepend: "",
    });
    blocker_161_write_pkg(Blocker161WritePkgArgs {
        repo: &repo.location,
        cp: "dev-libs/consumera",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/prov:=",
    });
    let config = test_config();
    // consumerb first resolves bare; consumera's `:=` then lands
    // on the taken slot and reuses it.
    let atoms = vec![
        "dev-libs/consumera".to_string(),
        "dev-libs/consumerb".to_string(),
    ];
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, vec![repo], &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(
        pass.slot_conflicts.iter().all(|sc| sc.package != "prov"),
        "a rebuildable := reuses the resolved slot silently"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: an auto-replace seed is not a top-level
/// argument -- it walks ownerless at depth 1 and never files an
/// `(Argument)` puller row. (Seeded alone, so no top-level visit
/// dedups it away first.)
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

/// Backlog #161 S6: the trigger resolves its owner against the
/// walked entries by exact cp -- a same-category neighbour walking
/// first never supplies it.
#[test]
fn run_pass_trigger_matches_the_owner_entry_exactly() {
    let dir = slotundo_temp_dir("161-run-misown");
    let repos = blocker_161_scratch_repo(Blocker161ScratchRepoArgs {
        base: &dir,
        cp: "dev-libs/depparent",
        pv: "1.0",
        slot: "0",
        depend: "",
        rdepend: "dev-libs/nosuchdep",
    });
    let config = test_config();
    // A neighbour resolving elsewhere walks first, so a loosened
    // owner lookup would build the wrong neg.
    let atoms = vec![
        "dev-libs/depparent".to_string(),
        "dev-libs/sounneed".to_string(),
    ];
    let repos = {
        let mut rs = find_repos(&fixtures_root()).expect("fixture repos");
        rs.push(repos);
        rs
    };
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let mut pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    let trigger = pass.missing_dep_trigger.take().expect("trigger armed");
    assert_eq!(
        trigger,
        (
            ("dev-libs".to_string(), "depparent".to_string()),
            "!=dev-libs/depparent-1.0".to_string(),
            "dev-libs/nosuchdep".to_string(),
        )
    );
    assert!(!pass.suppressed_nvc);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: under `--usepkgonly`, a newer staged binary
/// merges even with no ebuild twin at its version.
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

/// Backlog #161 S6: under plain `--usepkg`, a binary with no
/// ebuild twin at its version loses to the visible tree.
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

/// Backlog #161 S6: a keyword-masked tree ebuild does not count
/// as an ebuild twin -- the accepted staged binary still merges
/// under `--usepkgonly`.
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

/// Backlog #161 S6: a `package.use_mask` entry masks `test`
/// exactly like the global mask list.
#[test]
fn run_pass_package_use_mask_hides_test_deps() {
    let dir = slotundo_temp_dir("161-run-pumask");
    let atoms = vec!["dev-libs/withtestdeppkg".to_string()];
    let mut config = test_config();
    config.package_use_mask = vec![(
        "dev-libs/withtestdeppkg".to_string(),
        vec!["test".to_string()],
    )];
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let opts = CtxOpts161 {
        backtrack_max: 10,
        atoms: atoms.clone(),
        with_test_deps: true,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let pass = run_pass(&ctx, &BacktrackParams::default(), true).expect("walk settles");
    assert!(
        !pass.entries.iter().any(|e| e.package == "testonlydep"),
        "masked test stays out even with the flag"
    );
    let _ = fs::remove_dir_all(&dir);
}
