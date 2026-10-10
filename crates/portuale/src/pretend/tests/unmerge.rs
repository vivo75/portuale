use super::*;

#[test]
fn execute_unmerge_forwards_quiet_and_noinfo_to_the_info_regen() {
    // #176 review: `run_unmerge_pretend` -> `execute_unmerge` carry
    // real `--quiet` (`noiselimit < 0`) and the resolved-config
    // `noinfo` (real `"noinfo" not in settings.features`,
    // `post_emerge.py:127`) into `post_merge_info_update`. Hermetic:
    // an empty removal list touches no vdb package, and a stale
    // `dir`-only info dir needs no `install-info` run (every name is
    // skippable) yet still records the memo -- so neither case needs
    // the host binary.
    fn scratch_info_root(tag: &str) -> (std::path::PathBuf, String) {
        let root = TempDir::new(&format!("unmerge_info_wiring_{tag}")).keep();
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::write(
            root.join("etc/env.d/50-test"),
            "INFOPATH=\"/usr/share/info\"\n",
        )
        .unwrap();
        let info_dir = root.join("usr/share/info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(info_dir.join("dir"), "stale index\n").unwrap();
        let inforoot = format!("{}/usr/share/info", root.display());
        (root, inforoot)
    }
    let color = crate::color::Colorizer::new(false);

    // `quiet=true` prints nothing here (a `dir`-only regen counts
    // zero files), while `noinfo=false` lets the regen record the
    // memo -- proving the params arrive, not just exist.
    // (`ExitCode` exposes neither `PartialEq` nor its code, so
    // success is asserted via its `Debug` rendering.)
    let success = format!("{:?}", ExitCode::SUCCESS);
    let (root, inforoot) = scratch_info_root("record");
    // No repos and a default (news-less) config: the post-removal
    // GLEP 42 notice (backlog #196 fix round 1) is gated off, so
    // these info-regen assertions observe no notice output.
    let rc = execute_unmerge(
        &[],
        &root,
        crate::ebuild_phases::ShellBackend::default(),
        false,
        &color,
        true,
        false,
        &[],
        &portage_profile::Config::default(),
    );
    assert_eq!(format!("{rc:?}"), success);
    assert!(
        crate::mtimedb::read_info_mtimes(&root).contains_key(&inforoot),
        "noinfo=false must let the post-unmerge regen record the memo"
    );
    let _ = std::fs::remove_dir_all(&root);

    // `noinfo=true` (resolved-config `FEATURES=noinfo`) suppresses
    // the whole block: the memo stays empty.
    let (root, _) = scratch_info_root("suppressed");
    let rc = execute_unmerge(
        &[],
        &root,
        crate::ebuild_phases::ShellBackend::default(),
        false,
        &color,
        true,
        true,
        &[],
        &portage_profile::Config::default(),
    );
    assert_eq!(format!("{rc:?}"), success);
    assert!(crate::mtimedb::read_info_mtimes(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unmerge_end_notice_skips_an_empty_removal_list() {
    // Backlog #196 fix round 1: real `post_emerge` after the
    // uninstall actions prints the notice only when the vdb
    // changed -- an empty selection removes nothing, so no notice
    // (and no evaluation: no state dir appears). `noinfo=true`
    // skips the info regen (nothing to regenerate under a scratch
    // ROOT); the removal list is empty, so nothing else runs
    // either -- the positive path (a real removal prints) is
    // pinned by the contract suite instead.
    let color = crate::color::Colorizer::new(false);
    let (base, root, repos, config) = news_notice_test_tree("unmerge_empty");
    let rc = execute_unmerge(
        &[],
        &root,
        crate::ebuild_phases::ShellBackend::default(),
        false,
        &color,
        true,
        true,
        &repos,
        &config,
    );
    assert_eq!(format!("{rc:?}"), format!("{:?}", ExitCode::SUCCESS));
    assert!(
        !root.join("var/lib/gentoo/news").exists(),
        "an empty removal list must not evaluate news at all"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn still_listed_parents_applies_the_higher_slot_refinement() {
    // dev-libs/dualslotpkg is installed in slot 1 (1.0) and slot 2
    // (2.0); `dualslotset` lists the bare `dev-libs/dualslotpkg`.
    let root = fixtures_root();
    let sets = vec![(
        "dualslotset".to_string(),
        vec!["dev-libs/dualslotpkg".to_string()],
    )];

    // Removing the slot-1 version: slot 2 (higher, different slot)
    // still matches the bare set atom -> not a parent.
    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());

    // Removing the slot-2 version: nothing higher -> the set is a
    // parent.
    assert_eq!(
        still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "2.0"),
        vec!["dualslotset"]
    );
}

#[test]
fn still_listed_parents_is_empty_when_no_set_lists_the_cp() {
    let root = fixtures_root();
    let sets = vec![("someset".to_string(), vec!["dev-libs/other".to_string()])];
    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());
}
