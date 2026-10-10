use super::*;

#[test]
fn resume_cpv_maps_source_and_binary_and_skips_non_merges() {
    // The up-front resume save (real `Scheduler._save_resume_list`'s
    // `operation == "merge"` filter) records every entry that will
    // actually merge -- source and binary alike -- and nothing else.
    let src = resume_entry(
        "dev-libs",
        "src-pkg",
        "1.0",
        CandidateSource::Ebuild,
        &[],
        None,
    );
    assert_eq!(
        resume_cpv(&src),
        Some((
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "src-pkg".to_string(),
            "1.0".to_string(),
        ))
    );
    let bin = resume_entry(
        "dev-libs",
        "bin-pkg",
        "2.0",
        CandidateSource::Binary,
        &[],
        None,
    );
    assert_eq!(
        resume_cpv(&bin),
        Some((
            crate::mtimedb::ResumeEntryKind::Binary,
            "dev-libs".to_string(),
            "bin-pkg".to_string(),
            "2.0".to_string(),
        ))
    );
    let mut installed = resume_entry(
        "dev-libs",
        "old-pkg",
        "3.0",
        CandidateSource::Ebuild,
        &[],
        None,
    );
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "3.0".into(),
    };
    assert_eq!(resume_cpv(&installed), None);
    let mut uninstalled = resume_entry(
        "dev-libs",
        "gone-pkg",
        "4.0",
        CandidateSource::Ebuild,
        &[],
        None,
    );
    uninstalled.outcome = PretendOutcome::NoVisibleCandidate;
    assert_eq!(resume_cpv(&uninstalled), None);
}

#[test]
fn resume_binary_repo_comes_from_the_binary_pool_not_the_ebuild_repos() {
    // #186: real `depgraph.py::_loadResumeCommand` re-resolves a
    // `["binary", root, cpv, "merge"]` resume item from the bintree,
    // so the resumed `(N of M) cpv::repo` line names the binary's own
    // repository. Local `$PKGDIR` index first, then the configured
    // remote binhosts; a record without `REPO` is `"__unknown__"`
    // (what a fresh binary merge shows for it); nothing anywhere is
    // `None` (the caller then keeps the ebuild fallback).
    use std::collections::HashMap;
    let record = |cpv: &str, repo: Option<&str>| {
        let mut m = HashMap::new();
        m.insert("CPV".to_string(), cpv.to_string());
        if let Some(r) = repo {
            m.insert("REPO".to_string(), r.to_string());
        }
        m
    };
    let local = portage_repo::BinaryIndex::from_entries(vec![
        record("dev-libs/bin-a-1.0", Some("binhostrepo")),
        record("dev-libs/bin-norepo-1.0", None),
        record("dev-libs/bin-s-1.0", Some("localrepo")),
    ]);
    let here = Path::new("/");
    assert_eq!(
        resume_binary_repo(&local, &[], here, "dev-libs", "bin-a", "1.0"),
        Some("binhostrepo".to_string())
    );
    assert_eq!(
        resume_binary_repo(&local, &[], here, "dev-libs", "bin-norepo", "1.0"),
        Some("__unknown__".to_string())
    );
    assert_eq!(
        resume_binary_repo(&local, &[], here, "dev-libs", "missing", "1.0"),
        None
    );

    // Remote hit through a `file://` binhost the local index lacks.
    let remote_dir = tempdir();
    fs::write(
        remote_dir.join("Packages"),
        "TIMESTAMP: 0\nPACKAGES: 2\n\nCPV: dev-libs/bin-r-2.0\nREPO: remoterepo\n\nCPV: dev-libs/bin-s-1.0\nREPO: remoterepo\n",
    )
    .unwrap();
    let binrepos = vec![portage_profile::BinRepo {
        name: "tmpremote".to_string(),
        sync_uri: format!("file://{}", remote_dir.display()),
        priority: 1,
        location: None,
        verify_signature: false,
        frozen: false,
    }];
    assert_eq!(
        resume_binary_repo(&local, &binrepos, here, "dev-libs", "bin-r", "2.0"),
        Some("remoterepo".to_string())
    );
    // The local index shadows the binhost for the same version.
    assert_eq!(
        resume_binary_repo(&local, &binrepos, here, "dev-libs", "bin-s", "1.0"),
        Some("localrepo".to_string())
    );
    let _ = fs::remove_dir_all(&remote_dir);
}

#[test]
fn resume_entry_uses_the_binary_repo_for_binary_entries_only() {
    // #186: `binary_repo` wins for a `Binary` entry (even when an
    // ebuild repo would resolve -- here there are no repos at all,
    // so the fallback would be `None` either way and the win is
    // exact), is ignored for an `Ebuild` entry, and `None` keeps the
    // ebuild fallback.
    let bin = resume_entry(
        "dev-libs",
        "bin-pkg",
        "2.0",
        CandidateSource::Binary,
        &[],
        Some("binhostrepo"),
    );
    assert_eq!(bin.repo_name.as_deref(), Some("binhostrepo"));
    let src = resume_entry(
        "dev-libs",
        "src-pkg",
        "1.0",
        CandidateSource::Ebuild,
        &[],
        Some("binhostrepo"),
    );
    assert_eq!(src.repo_name, None);
    let fallback = resume_entry(
        "dev-libs",
        "bin-pkg",
        "2.0",
        CandidateSource::Binary,
        &[],
        None,
    );
    assert_eq!(fallback.repo_name, None);
}

#[test]
fn run_merge_loop_shrinks_the_saved_resume_list_per_successful_merge() {
    // #168 through the production serial loop: with the full mergelist
    // saved up front, each successful merge removes exactly that entry
    // (real `Scheduler.py:1595-1602`), so a SIGKILL mid-run leaves the
    // tail and the last merge deletes the key.
    let root = tempdir();
    let entries = vec![
        resume_entry(
            "dev-libs",
            "loop-a",
            "1.0",
            CandidateSource::Ebuild,
            &[],
            None,
        ),
        resume_entry(
            "dev-libs",
            "loop-b",
            "1.0",
            CandidateSource::Ebuild,
            &[],
            None,
        ),
        resume_entry(
            "dev-libs",
            "loop-c",
            "1.0",
            CandidateSource::Ebuild,
            &[],
            None,
        ),
    ];
    let full: Vec<crate::mtimedb::ResumeCpv> =
        entries.iter().map(|e| resume_cpv(e).unwrap()).collect();
    crate::mtimedb::write_resume_list(
        &root,
        &["dev-libs/loop-a"],
        &full,
        &crate::mtimedb::ResumeOpts::default(),
    )
    .unwrap();

    run_merge_loop(&entries, false, &root, |_, entry| {
        // By the time this entry's own merge runs, every earlier entry
        // has already left the on-disk list.
        let pos = full
            .iter()
            .position(|(_, _, p, _)| p == &entry.package)
            .expect("loop entry is in the saved list");
        let (_, remaining, _) =
            crate::mtimedb::read_resume_list(&root).expect("resume list mid-run");
        assert_eq!(remaining, full[pos..]);
        Ok(())
    })
    .unwrap();

    assert!(crate::mtimedb::read_resume_list(&root).is_none());
    let _ = fs::remove_dir_all(&root);
}
