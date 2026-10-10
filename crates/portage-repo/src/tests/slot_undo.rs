use super::*;

/// #250: real's build-edge satisfaction is `vardb.match_pkgs(atom)`,
/// which honours use-deps: an installed `dev-util/u250make` built
/// `-foo` does not satisfy `dev-util/u250make[foo]`, so the
/// `u250json -> u250make` build edge stays unbreakable (the pmtest
/// fixture; real's playground in `LLM/evidence/2026-09-30-250/`).
#[test]
fn run_pass_use_mismatched_installed_instance_leaves_the_build_edge_hard() {
    let dir = slotundo_temp_dir("250-edge");
    let d = dir.join("var/db/pkg/dev-util/u250make-1");
    fs::create_dir_all(&d).unwrap();
    for (f, v) in [
        ("CATEGORY", "dev-util"),
        ("SLOT", "0"),
        ("EAPI", "8"),
        ("IUSE", "foo"),
        ("USE", ""),
        ("repository", "testrepo"),
    ] {
        fs::write(d.join(f), format!("{v}\n")).unwrap();
    }
    let atoms = vec!["dev-util/u250make".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let edge = pass.edge_kind_map.get(&(
        (false, "dev-util".to_string(), "u250make".to_string()),
        (false, "dev-libs".to_string(), "u250json".to_string()),
    ));
    assert_eq!(
        edge,
        Some(&(true, false)),
        "the -foo installed build does not satisfy u250make[foo]"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a bare leaf atom resolves to a single `New`
/// entry at the tree version and slot, carrying no old slot and
/// pulling no arguments.
#[test]
fn run_pass_resolves_a_leaf_to_a_new_entry() {
    let dir = slotundo_temp_dir("161-run-new");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    let e = &pass.entries[0];
    assert_eq!(
        (e.category.as_str(), e.package.as_str()),
        ("dev-libs", "sounneed")
    );
    assert!(matches!(
        e.outcome,
        PretendOutcome::New { ref version } if version == "1.0"
    ));
    assert!(!e.new_slot, "nothing installed anywhere for this cp");
    assert!(e.oldbest.is_empty());
    // The USE line renders every IUSE flag, off here.
    assert_eq!(e.use_flags_display, vec![("soflag".to_string(), false)]);
    assert_eq!(
        pass.slot_pullers
            .get(&("dev-libs".to_string(), "sounneed".to_string())),
        Some(&vec![(
            String::new(),
            String::new(),
            String::new(),
            "dev-libs/sounneed".to_string(),
        )]),
        "a top-level atom is an Argument puller"
    );
    assert!(pass.slot_conflicts.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: an atom with no visible candidate resolves to a
/// `NoVisibleCandidate` entry, not an error.
#[test]
fn run_pass_reports_a_missing_atom_as_nvc() {
    let dir = slotundo_temp_dir("161-run-nvc");
    let atoms = vec!["dev-libs/nosuchpkg".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::NoVisibleCandidate
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a top-level atom for the installed version
/// resolves to a bare reasonless `Reinstall` (real's `[ebuild R]`
/// for an explicitly requested package -- `is_top_level &&
/// !selective`).
#[test]
fn run_pass_reinstalls_the_requested_installed_version() {
    let dir = slotundo_temp_dir("161-run-inst");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("sounneed", "1.0", "0/1")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::Reinstall {
            ref version,
            ref changed_flags,
            deps_changed: false,
            slot_changed: false,
            ..
        } if version == "1.0" && changed_flags.is_empty()
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: the same installed version reached only as a
/// dependency resolves `AlreadyInstalled` with no merge.
#[test]
fn run_pass_keeps_a_satisfied_dependency_installed() {
    let dir = slotundo_temp_dir("161-run-depinst");
    let atoms = vec!["dev-libs/needer".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("paired", "2.0", "0")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let paired = pass
        .entries
        .iter()
        .find(|e| e.package == "paired")
        .expect("paired walks as needer's dependency");
    assert!(matches!(
        paired.outcome,
        PretendOutcome::AlreadyInstalled { ref version } if version == "2.0"
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: with `--update`, an older installed instance
/// upgrades to the tree version.
#[test]
fn run_pass_upgrades_an_older_installed_version() {
    let dir = slotundo_temp_dir("161-run-up");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: true,
        installed: &[("sounneed", "0.9", "0/1"), ("sounneed", "0.8", "0/1")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::Upgrade { ref from, ref to } if from == "0.9" && to == "1.0"
    ));
    // The upgrade re-renders the USE line, trailing state marker
    // included, exactly as resolved.
    assert_eq!(
        pass.entries[0].use_flags_display,
        vec![("soflag".to_string(), false)]
    );
    assert_eq!(
        pass.entries[0].use_expand_display_p,
        vec![("USE".to_string(), "-soflag%".to_string())]
    );
    // Both installed same-slot instances ride as oldbest, sorted.
    assert_eq!(
        pass.entries[0]
            .oldbest
            .iter()
            .map(|r| r.version.as_str())
            .collect::<Vec<_>>(),
        vec!["0.8", "0.9"]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: with `--update`, a newer installed instance
/// downgrades to the tree version.
#[test]
fn run_pass_downgrades_a_newer_installed_version() {
    let dir = slotundo_temp_dir("161-run-down");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: true,
        installed: &[("sounneed", "2.0", "0/1")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::Downgrade { ref from, ref to } if from == "2.0" && to == "1.0"
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: dependencies walk transitively, and each
/// walked child records its parent atom.
#[test]
fn run_pass_walks_dependencies_and_records_parent_atoms() {
    let dir = slotundo_temp_dir("161-run-deps");
    let atoms = vec!["dev-libs/needer".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let mut names: Vec<&str> = pass.entries.iter().map(|e| e.package.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["needer", "paired"]);
    // (`parent_atoms` rows are collected under `--debug` only, like
    // every `resolver_trace` row -- pinned by the contract suite, so
    // there is no unit-observable assertion for them here.)
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: with `--newuse`, an installed instance whose
/// built USE misses a now-enabled flag reinstalls with the flag
/// change recorded -- and the newly-enabled conditional dep walks.
#[test]
fn run_pass_newuse_reinstalls_a_flag_change() {
    let dir = slotundo_temp_dir("161-run-newuse");
    let atoms = vec!["dev-libs/deepvdbusepkg".to_string()];
    // The vdb records no `wantdep`; the profile enables it.
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("deepvdbusepkg", "1.0", "0")],
        newuse: true,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let pkg = pass
        .entries
        .iter()
        .find(|e| e.package == "deepvdbusepkg")
        .expect("the requested package resolves");
    match &pkg.outcome {
        PretendOutcome::Reinstall { changed_flags, .. } => {
            assert_eq!(changed_flags, &vec!["wantdep".to_string()]);
        }
        other => panic!("expected a USE-change reinstall, got {other:?}"),
    }
    // The reinstall renders the recorded USE with the changed
    // flag marked.
    assert_eq!(pkg.use_flags_display, vec![("wantdep".to_string(), false)]);
    // The same-slot same-sub installed instance is not its own
    // oldbest: the `Reinstall` filter keeps only *other*
    // slot/sub instances (a reinstall in place supersedes, not
    // coexists).
    assert!(
        pkg.oldbest.is_empty(),
        "an in-place reinstall records no oldbest"
    );
    assert_eq!(
        pkg.use_expand_display_p,
        vec![("USE".to_string(), "-wantdep%".to_string())]
    );
    // The conditional dep itself resolves on the retry walk the
    // recorded flag change drives (contract-suite pinned); this
    // pass pins the flag delta.
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6r: a second atom resolving to an already-graphed
/// `Reinstall` reuses it -- the `resolved_slots` revisit reads the
/// entry's own version back out instead of resolving (and
/// conflicting) twice. Same shape as the use-unsat dedup pair,
/// but installed, so the first outcome is a reinstall.
#[test]
fn run_pass_revisiting_a_reinstall_reuses_the_entry() {
    let dir = slotundo_temp_dir("161-run-rereinst");
    let atoms = vec![
        "dev-libs/deepvdbusepkg".to_string(),
        "dev-libs/deepvdbusepkg:0".to_string(),
    ];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("deepvdbusepkg", "1.0", "0")],
        newuse: true,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let rows: Vec<_> = pass
        .entries
        .iter()
        .filter(|e| e.package == "deepvdbusepkg")
        .collect();
    assert_eq!(rows.len(), 1, "the revisit reuses the reinstall entry");
    assert!(
        matches!(rows[0].outcome, PretendOutcome::Reinstall { .. }),
        "the kept entry is still the reinstall"
    );
    assert!(
        pass.slot_conflicts.is_empty(),
        "a satisfied revisit files no conflict"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: without `--newuse` the same USE mismatch stays
/// a bare reinstall with no flag change and no extra walk.
#[test]
fn run_pass_without_newuse_keeps_a_use_mismatch_quiet() {
    let dir = slotundo_temp_dir("161-run-nouse");
    let atoms = vec!["dev-libs/deepvdbusepkg".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("deepvdbusepkg", "1.0", "0")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let pkg = pass
        .entries
        .iter()
        .find(|e| e.package == "deepvdbusepkg")
        .expect("the requested package resolves");
    match &pkg.outcome {
        PretendOutcome::Reinstall { changed_flags, .. } => {
            assert!(changed_flags.is_empty());
        }
        other => panic!("expected a bare reinstall, got {other:?}"),
    }
    assert!(
        !pass.entries.iter().any(|e| e.package == "deepvdbusetarget"),
        "the conditional dep stays unwalked without the flip"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: under `--usepkgonly` with no binary packages,
/// even a tree-present leaf has no mergeable candidate.
#[test]
fn run_pass_usepkgonly_without_binaries_reports_nvc() {
    let dir = slotundo_temp_dir("161-run-binonly");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: true,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::NoVisibleCandidate
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: under `--nodeps` only the requested atom
/// resolves -- dependencies never walk, and no dep-driven rows
/// appear.
#[test]
fn run_pass_nodeps_resolves_only_the_requested_atom() {
    let dir = slotundo_temp_dir("161-run-nodeps");
    let atoms = vec!["dev-libs/needer".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: true,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert_eq!(pass.entries[0].package.as_str(), "needer");
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: in selective mode the requested installed
/// version stays `AlreadyInstalled` instead of the bare reinstall.
#[test]
fn run_pass_selective_keeps_the_requested_version() {
    let dir = slotundo_temp_dir("161-run-sel");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("sounneed", "1.0", "0/1")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: true,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::AlreadyInstalled { .. }
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: multiple atoms resolve in walk order (the
/// seed stack runs argv reversed) -- both merge.
#[test]
fn run_pass_resolves_multiple_atoms_in_order() {
    let dir = slotundo_temp_dir("161-run-multi");
    let atoms = vec![
        "dev-libs/paired".to_string(),
        "dev-libs/sounneed".to_string(),
    ];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let names: Vec<&str> = pass.entries.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["sounneed", "paired"]);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a use-dep the resolved USE satisfies (here
/// `[-wantdep]`, off by default and untouched by the profile)
/// resolves the atom normally.
#[test]
fn run_pass_satisfies_a_disabled_use_dep() {
    let dir = slotundo_temp_dir("161-run-usedep");
    let atoms = vec!["dev-libs/deepvdbusepkg[-wantdep]".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.entries.len(), 1);
    assert!(matches!(
        pass.entries[0].outcome,
        PretendOutcome::New { .. } | PretendOutcome::Reinstall { .. }
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a use-dep nothing enables reports the atom
/// unsatisfiable instead of resolving.
#[test]
fn run_pass_reports_an_unenabled_use_dep_unsatisfied() {
    let dir = slotundo_temp_dir("161-run-nouse");
    let atoms = vec!["dev-libs/sounneed[soflag]".to_string()];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert!(
        pass.entries.iter().all(|e| !matches!(
            e.outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Reinstall { .. }
        )),
        "no merge without the flag"
    );
    assert!(!pass.use_unsat_deps.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: two unsatisfiable use-deps on one cp file a
/// single use-unsat report.
#[test]
fn run_pass_dedups_use_unsat_reports() {
    let dir = slotundo_temp_dir("161-run-usedup");
    let atoms = vec![
        "dev-libs/sounneed[soflag]".to_string(),
        "dev-libs/sounneed[soflag]:0".to_string(),
    ];
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    assert_eq!(pass.use_unsat_deps.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: a `New` into a slot with nothing installed is
/// still a plain new slot row only when another slot is installed.
#[test]
fn run_pass_marks_a_new_slot_row() {
    let dir = slotundo_temp_dir("161-run-newslot");
    let atoms = vec!["dev-libs/dualslotpkg".to_string()];
    // Slot 1 stays installed; the walk news slot 2 alongside it.
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: true,
        installed: &[("dualslotpkg", "1.0", "1")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let entry = pass
        .entries
        .iter()
        .find(|e| e.package == "dualslotpkg" && e.slot.as_deref() == Some("2"))
        .expect("slot-2 news alongside slot 1");
    assert!(matches!(
        entry.outcome,
        PretendOutcome::New { ref version } if version == "2.0"
    ));
    assert!(entry.new_slot);
    assert_eq!(entry.oldbest.len(), 1);
    assert_eq!(entry.oldbest[0].version, "1.0");
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S6: reinstalling at a moved sub-slot records the
/// installed instance for the `[oldver]` bracket.
#[test]
fn run_pass_records_oldbest_for_a_moved_reinstall() {
    let dir = slotundo_temp_dir("161-run-oldbest");
    let atoms = vec!["dev-libs/sounneed".to_string()];
    // Installed sub-slot 9 disagrees with the tree's 1.
    let pass = run_161(Run161Args {
        dir: &dir,
        atoms: &atoms,
        update: false,
        installed: &[("sounneed", "1.0", "0/9")],
        newuse: false,
        usepkgonly: false,
        nodeps: false,
        selective: false,
        usepkg: false,
        deep: Deep::NotRequested,
        with_bdeps: false,
    });
    let entry = pass
        .entries
        .iter()
        .find(|e| e.package == "sounneed")
        .expect("sounneed resolves");
    assert_eq!(entry.oldbest.len(), 1);
    assert_eq!(entry.oldbest[0].version, "1.0");
    assert_eq!(entry.oldbest[0].sub_slot, "9");
    let _ = fs::remove_dir_all(&dir);
}
