use super::*;

#[test]
fn execute_unmerge_forwards_quiet_and_noinfo_to_the_info_regen() {
    // Portage: post_emerge.py. `--quiet` and the resolved `noinfo` reach the info regen.
    // Hermetic: an empty removal and a `dir`-only info dir need no host binary.
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

    // `ExitCode` has no `PartialEq`, so success is asserted via `Debug`.
    let success = format!("{:?}", ExitCode::SUCCESS);
    let (root, inforoot) = scratch_info_root("record");
    let rc = execute_unmerge(ExecuteUnmergeArgs {
        removal_list: &[],
        root: &root,
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        color: &color,
        quiet: true,
        noinfo: false,
        repos: &[],
        config: &portage_profile::Config::default(),
    });
    assert_eq!(format!("{rc:?}"), success);
    assert!(
        crate::mtimedb::read_info_mtimes(&root).contains_key(&inforoot),
        "noinfo=false must let the post-unmerge regen record the memo"
    );
    let _ = std::fs::remove_dir_all(&root);

    let (root, _) = scratch_info_root("suppressed");
    let rc = execute_unmerge(ExecuteUnmergeArgs {
        removal_list: &[],
        root: &root,
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        color: &color,
        quiet: true,
        noinfo: true,
        repos: &[],
        config: &portage_profile::Config::default(),
    });
    assert_eq!(format!("{rc:?}"), success);
    assert!(crate::mtimedb::read_info_mtimes(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unmerge_end_notice_skips_an_empty_removal_list() {
    // Portage: post_emerge.py. An empty removal changes nothing, so no notice and no evaluation.
    let color = crate::color::Colorizer::new(false);
    let (base, root, repos, config) = news_notice_test_tree("unmerge_empty");
    let rc = execute_unmerge(ExecuteUnmergeArgs {
        removal_list: &[],
        root: &root,
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        color: &color,
        quiet: true,
        noinfo: true,
        repos: &repos,
        config: &config,
    });
    assert_eq!(format!("{rc:?}"), format!("{:?}", ExitCode::SUCCESS));
    assert!(
        !root.join("var/lib/gentoo/news").exists(),
        "an empty removal list must not evaluate news at all"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn still_listed_parents_applies_the_higher_slot_refinement() {
    // A higher different slot still matching the set atom suppresses the parent warning.
    let root = fixtures_root();
    let sets = vec![(
        "dualslotset".to_string(),
        vec!["dev-libs/dualslotpkg".to_string()],
    )];

    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());

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

/// A scratch config root resolving `FEATURES` from a `make.globals` carrying
/// `binpkg-multi-instance` plus `make_conf` as `make.conf`.
fn unmerge_features_probe_root(tag: &str, make_conf: &str) -> std::path::PathBuf {
    let root = TempDir::new(&format!("unmerge_features_{tag}")).keep();
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    let globals_dir = root.join("usr/share/portage/config");
    std::fs::create_dir_all(&prof).unwrap();
    std::fs::create_dir_all(&portage_dir).unwrap();
    std::fs::create_dir_all(&globals_dir).unwrap();
    std::fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n",
    )
    .unwrap();
    std::fs::write(portage_dir.join("make.conf"), make_conf).unwrap();
    std::fs::write(
        portage_dir.join("repos.conf"),
        "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = repo\n",
    )
    .unwrap();
    std::fs::write(
        globals_dir.join("make.globals"),
        "FEATURES=\"binpkg-multi-instance\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();
    root
}
