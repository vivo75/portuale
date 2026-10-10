use super::*;

#[test]
fn resume_pretend_end_notice_prints_once_like_portage() {
    // Backlog #231 (a): real reaches `post_emerge` after the
    // `--resume --pretend` display too (`actions.py:4289-4297`),
    // whose `not _pkgs_changed` arm prints the notice under
    // `--pretend` (`post_emerge.py:112-117`) -- exactly once (the
    // pre-resolution notice is itself `--pretend`-gated,
    // `actions.py:4266`). A stale list + `FEATURES="news"` on an
    // isolated ROOT; no merge runs.
    let portuale_bin = built_portuale_bin();
    let (root, _) = stale_resume_root("news_pretend");
    let tmp = TempDir::new("resume_news_pretend").keep();
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--resume", "--pretend"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(news_resolve_env(&root, &tmp.join("pt-resume-news")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dev-libs/stale-a-1"), "{stdout}");
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resume_skipfirst_empty_list_prints_only_the_pre_notice_like_portage() {
    // Backlog #231 (a): real's pre-resolution notice precedes all
    // of `action_build`'s resume handling (`actions.py:4266` before
    // `:4289`), so even a `--resume --skipfirst` that empties the
    // list prints it -- while the post notice stays silent (the
    // vdb never changed; real `post_emerge.py:112-117`
    // early-returns on a non-`--pretend` run). Fast: a single-item
    // stale list, no merge runs.
    let portuale_bin = built_portuale_bin();
    let root = single_resume_root("news_empty");
    let tmp = TempDir::new("resume_news_empty").keep();
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--resume", "--skipfirst"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(news_resolve_env(&root, &tmp.join("pt-resume-empty")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("nothing to do"), "{stdout}");
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pretend_writes_no_resume_list_even_with_a_stale_list_present() {
    // #179: real `_emerge/actions.py` returns from the `--pretend`
    // branch (`display(...)`, `return os.EX_OK`) before `Scheduler`
    // exists -- and before the `resume_backup` rotation -- so a
    // preview never touches `mtimedb`. A stale list must come back
    // byte-identical, and no fresh list may appear on a bare root.
    let portuale_bin = built_portuale_bin();
    let tmp = TempDir::new("resume_no_write_pretend").keep();

    // Bare root first: `--pretend` resolves and prints, and leaves
    // no `mtimedb` behind at all.
    let bare = tmp.join("bare");
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--pretend", "--oneshot", "dev-libs/schedok"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(fixture_resolve_env(&bare, &tmp.join("pt-bare")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("dev-libs/schedok-1.0"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !crate::mtimedb::mtimedb_path(&bare).exists(),
        "--pretend must not create an mtimedb"
    );

    // Stale list present: byte-identical afterwards (neither a fresh
    // save nor the rotation may run).
    let (root, before) = stale_resume_root("pretend");
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--pretend", "--oneshot", "dev-libs/schedok"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(fixture_resolve_env(&root, &tmp.join("pt-stale")))
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(crate::mtimedb::mtimedb_path(&root)).unwrap(),
        before,
        "--pretend must leave a stale resume list byte-identical"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn entries_not_merged_tags_and_includes_both_source_and_binary_entries() {
    // Real portage's own resume mergelist can hold both a source
    // and a binary entry together -- `source_entries_not_merged`
    // (this function's own former name) used to silently drop
    // every `Binary` entry instead of tagging and including it.
    let root = TempDir::new("pretend-test-entries_not_merged_binary").keep();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let source_entry = emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
        category: "dev-libs",
        package: "src-pkg",
        version: "1.0",
        source: portage_repo::CandidateSource::Ebuild,
        repos: &[],
        binary_repo: None,
    });
    let binary_entry = emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
        category: "dev-libs",
        package: "bin-pkg",
        version: "2.0",
        source: portage_repo::CandidateSource::Binary,
        repos: &[],
        binary_repo: None,
    });
    let entries = vec![source_entry, binary_entry];

    // Neither has a real vdb CONTENTS under `root` -- both count as
    // "not merged".
    let unmerged = entries_not_merged(&root, &entries);
    assert_eq!(unmerged.len(), 2);
    assert_eq!(
        unmerged[0],
        (
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "src-pkg".to_string(),
            "1.0".to_string(),
        )
    );
    assert_eq!(
        unmerged[1],
        (
            crate::mtimedb::ResumeEntryKind::Binary,
            "dev-libs".to_string(),
            "bin-pkg".to_string(),
            "2.0".to_string(),
        )
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn up_front_save_covers_exactly_the_failure_path_tail_when_nothing_merged() {
    // #168: the up-front save (`emerge_build::resume_cpv` over the
    // whole plan) and the failure-path tail (`entries_not_merged`)
    // agree when no merge has landed yet -- so keeping the failure
    // writes alongside the new up-front save + per-merge shrink is
    // provably a harmless redundancy, and failure behaviour is
    // unchanged.
    let root = TempDir::new("pretend-test-up_front_matches_failure_tail").keep();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let entries = vec![
        emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
            category: "dev-libs",
            package: "src-pkg",
            version: "1.0",
            source: portage_repo::CandidateSource::Ebuild,
            repos: &[],
            binary_repo: None,
        }),
        emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
            category: "dev-libs",
            package: "bin-pkg",
            version: "2.0",
            source: portage_repo::CandidateSource::Binary,
            repos: &[],
            binary_repo: None,
        }),
    ];
    let up_front: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    assert_eq!(up_front.len(), 2);
    assert_eq!(entries_not_merged(&root, &entries), up_front);
    let _ = std::fs::remove_dir_all(&root);
}
