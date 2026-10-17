use super::*;

#[test]
fn is_merge_command_covers_exactly_merge() {
    assert!(is_merge_command("merge"));
    assert!(!is_merge_command("qmerge"));
    assert!(!is_merge_command("unmerge"));
    assert!(!is_merge_command("install"));
}

#[test]
fn is_qmerge_command_covers_exactly_qmerge() {
    assert!(is_qmerge_command("qmerge"));
    assert!(!is_qmerge_command("merge"));
    assert!(!is_qmerge_command("unmerge"));
    assert!(!is_qmerge_command("install"));
}

#[test]
fn real_merge_lands_files_and_a_symlink_and_writes_a_real_vdb_entry() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

    // Keeps build state (`FEATURES=noclean`) so the hook markers below survive;
    // the default post-merge clean is pinned by `run_merge_post_cleans_the_builddir_unless_noclean`.
    let options = MergeOptions {
        features: "noclean".to_string(),
        ..MergeOptions::default()
    };
    let status =
        run_merge(&ebuild, &root, &portage_tmpdir, &options, None).expect("run_merge succeeds");
    assert_eq!(status, 0);

    assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/mergepkg/hello.txt"))
            .unwrap()
            .trim(),
        "hello from mergepkg"
    );
    let link = root.join("usr/share/mergepkg/hello-link.txt");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        PathBuf::from("hello.txt")
    );

    let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("CATEGORY"))
            .unwrap()
            .trim(),
        "dev-libs"
    );
    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("SLOT"))
            .unwrap()
            .trim(),
        "0"
    );
    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("repository"))
            .unwrap()
            .trim(),
        "testrepo"
    );
    let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("obj /usr/share/mergepkg/hello.txt "))
    );
    assert!(contents.contains("sym /usr/share/mergepkg/hello-link.txt -> hello.txt"));

    let counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
        .unwrap()
        .parse()
        .expect("COUNTER is a bare integer");
    assert!(counter >= 0);

    // Portage merges via a temp dir renamed into place: no `-MERGING-` temp survives.
    assert!(
        !root
            .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
            .exists()
    );

    // Ordering proof: the hooks leave these markers only if preinst ran before the
    // merge was visible and postinst after (see mergepkg-1.0.ebuild).
    let t_dir = portage_tmpdir.join("portage/dev-libs/mergepkg-1.0/temp");
    assert!(
        t_dir.join("preinst-ran-before-merge").is_file(),
        "pkg_preinst must run, and see the file not yet merged"
    );
    assert!(
        t_dir.join("postinst-ran-after-merge").is_file(),
        "pkg_postinst must run, and see the file (and vdb entry) already merged"
    );
}

/// A *computed* `SLOT="$(ver_cut 1)"` is read back as its **evaluated** value.
/// Expected value from Portage (container bed mpdecimal probe, `4.0.1` -> `SLOT=4`).
/// A reinstall under default `protect-owned` must credit the installed instance.
#[test]
fn computed_slot_is_evaluated_in_the_vdb_and_a_reinstall_does_not_abort_under_protect_owned() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/compuslotpkg/compuslotpkg-1.0.ebuild");

    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("first merge succeeds");
    assert_eq!(status, 0);

    let vdb_dir = root.join("var/db/pkg/dev-libs/compuslotpkg-1.0");
    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("SLOT"))
            .unwrap()
            .trim(),
        "1",
        "vdb SLOT must be the evaluated ver_cut value, not the raw $(ver_cut 1) text"
    );

    // Reinstall under the same default options: the installed instance owns its
    // own file, so this must not abort.
    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("reinstall merge succeeds");
    assert_eq!(status, 0, "reinstall must not abort under protect-owned");
    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("SLOT"))
            .unwrap()
            .trim(),
        "1"
    );
}

#[test]
fn real_merge_writes_the_full_build_info_into_the_vdb_entry() {
    // `dev-libs/packagepkg` has an md5-cache entry with `RDEPEND="dev-libs/samepkg"`;
    // Portage copies every `build-info` file into the vdb.
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/packagepkg/packagepkg-1.0.ebuild");

    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
    let read = |f: &str| {
        std::fs::read_to_string(vdb.join(f))
            .unwrap_or_else(|e| panic!("vdb/{f}: {e}"))
            .trim()
            .to_string()
    };
    assert_eq!(read("RDEPEND"), "dev-libs/samepkg");
    assert_eq!(read("EAPI"), "8");
    assert_eq!(read("KEYWORDS"), "amd64");
    assert_eq!(read("SLOT"), "0");
    // The bundled ebuild + saved environment come across too.
    assert!(vdb.join("packagepkg-1.0.ebuild").is_file());
    assert!(vdb.join("environment.bz2").is_file());
    // An empty md5-cache value (`DEPEND=`) is not written at all (Portage unlinks it).
    assert!(!vdb.join("DEPEND").exists());
}

#[test]
fn run_qmerge_fails_when_the_install_phase_has_not_been_run() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

    let err = run_qmerge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default())
        .expect_err("qmerge without a prior install must fail");
    assert!(err.contains("install phase has not been run"), "{err}");
    assert!(!root.join("usr/share/mergepkg").exists());
}

#[test]
fn run_qmerge_merges_without_rerunning_the_install_phase() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

    // qmerge assumes a prior `install` populated ${D}; run only that phase here.
    let install_status = ebuild_phases::run_commands(ebuild_phases::RunCommandsArgs {
        ebuild_path: &ebuild,
        commands: &["install"],
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        distdir: &MergeOptions::default().distdir,
        debug: false,
        config_root: &MergeOptions::default().config_root,
        shell: ebuild_phases::ShellBackend::default(),
        build_env: &[],
    })
    .expect("install phase succeeds");
    assert_eq!(install_status, 0);
    // `__dyn_install` leaves this marker behind, as a prior install run would.
    assert!(
        portage_tmpdir
            .join("portage/dev-libs/mergepkg-1.0/.installed")
            .exists()
    );

    let status = run_qmerge(&ebuild, &root, &portage_tmpdir, &MergeOptions::default())
        .expect("run_qmerge succeeds");
    assert_eq!(status, 0);

    assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
    let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
    assert!(vdb_dir.join("CONTENTS").is_file());
    // pkg_preinst/pkg_postinst still run; qmerge skips only the install phase.
    let t_dir = portage_tmpdir.join("portage/dev-libs/mergepkg-1.0/temp");
    assert!(t_dir.join("preinst-ran-before-merge").is_file());
    assert!(t_dir.join("postinst-ran-after-merge").is_file());
}

#[test]
fn re_merging_the_same_package_replaces_the_vdb_entry_and_bumps_counter() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");
    let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");

    assert_eq!(
        run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None
        )
        .unwrap(),
        0
    );
    let first_counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
        .unwrap()
        .parse()
        .unwrap();

    assert_eq!(
        run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None
        )
        .unwrap(),
        0
    );
    let second_counter: i64 = std::fs::read_to_string(vdb_dir.join("COUNTER"))
        .unwrap()
        .parse()
        .unwrap();

    assert!(second_counter > first_counter);
    assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
    assert!(
        !root
            .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
            .exists()
    );
}

/// Reinstalling the same version removes the files the new image no longer
/// has and keeps the ones it still has (Portage: `dblink.treewalk` takes
/// the new package's ownership from its temporary `CONTENTS`, #354).
#[test]
fn reinstalling_the_same_version_removes_files_the_new_image_dropped() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let original =
        std::fs::read_to_string(repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild")).unwrap();
    let dir = tmp.join("repo/dev-libs/mergepkg");
    std::fs::create_dir_all(&dir).unwrap();
    let ebuild = dir.join("mergepkg-1.0.ebuild");
    // First image: the fixture plus an extra file in a directory of its own.
    std::fs::write(
        &ebuild,
        original.replace(
            "\tdosym hello.txt",
            "\tinsinto /usr/share/${PN}-extra\n\tdoins \"${T}/hello.txt\"\n\tdosym hello.txt",
        ),
    )
    .unwrap();
    let merge = || {
        run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
    };
    assert_eq!(merge().unwrap(), 0);
    assert!(root.join("usr/share/mergepkg-extra/hello.txt").is_file());

    // Second image of the same version: the extra file and the link are gone.
    std::fs::write(
        &ebuild,
        original.replace("\tdosym hello.txt /usr/share/${PN}/hello-link.txt\n", ""),
    )
    .unwrap();
    assert_eq!(merge().unwrap(), 0);
    assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
    assert!(!root.join("usr/share/mergepkg-extra/hello.txt").exists());
    assert!(!root.join("usr/share/mergepkg-extra").exists());
    assert!(std::fs::symlink_metadata(root.join("usr/share/mergepkg/hello-link.txt")).is_err());
    let contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/mergepkg-1.0/CONTENTS")).unwrap();
    assert!(contents.contains("/usr/share/mergepkg/hello.txt"));
    assert!(!contents.contains("mergepkg-extra"));
}

/// Backlog #357 S3: `CONTENTS` paths with spaces are removed on
/// unmerge (S1 shape) and on a same-version reinstall that drops one
/// (S2 shape), while an unowned prefix decoy is kept in both.
///
/// Expected values from Portage 3.0.82.2, S0 cells S1/S2 (not from
/// portuale output): variant 1 records `a b.txt`, `c d -> a b.txt` and
/// `sp dir/f`; `emerge -C` removes all three and keeps the
/// admin-created decoy `/usr/share/t355/a`. Variant 2 drops `a b.txt`;
/// reinstalling removes it through the replace loop and keeps the
/// decoy. Pre-fix portuale removed the decoy and leaked the space
/// paths in both shapes.
#[test]
fn a_contents_path_with_spaces_is_removed_and_the_prefix_decoy_is_kept() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo355 = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo-355");
    let original =
        std::fs::read_to_string(repo355.join("app-misc/t355-space/t355-space-1.ebuild")).unwrap();
    let dir = tmp.join("repo/app-misc/t355-space");
    std::fs::create_dir_all(&dir).unwrap();
    let ebuild = dir.join("t355-space-1.ebuild");
    std::fs::write(&ebuild, &original).unwrap();
    let merge = || {
        run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
    };
    let unmerge = || {
        unmerge_one_installed(UnmergeOneInstalledArgs {
            root: &root,
            category: "app-misc",
            package: "t355-space",
            pf: "t355-space-1",
            also_keep: &[],
            scratch_dir: &tmp.join("scratch"),
            portage_tmpdir: &portage_tmpdir,
            options: &MergeOptions::default(),
            backup: None,
            is_replacement: false,
            replaced_by_version: None,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
            deferred_delete: None,
        })
        .expect("unmerge succeeds")
    };

    // S1 shape: merge, check the CONTENTS bytes, plant the decoy, `-C`.
    assert_eq!(merge().unwrap(), 0);
    let contents =
        std::fs::read_to_string(root.join("var/db/pkg/app-misc/t355-space-1/CONTENTS")).unwrap();
    assert!(contents.contains("obj /usr/share/t355/a b.txt "));
    assert!(contents.contains("sym /usr/share/t355/c d -> a b.txt "));
    assert!(contents.contains("obj /usr/share/t355/sp dir/f "));
    std::fs::write(root.join("usr/share/t355/a"), b"decoy").unwrap();
    unmerge();
    assert!(!root.join("usr/share/t355/a b.txt").exists());
    assert!(std::fs::symlink_metadata(root.join("usr/share/t355/c d")).is_err());
    assert!(!root.join("usr/share/t355/sp dir/f").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/t355/a")).unwrap(),
        "decoy"
    );

    // S2 shape: merge variant 1 again, plant the decoy, reinstall with
    // variant 2 (drops `a b.txt`); the replace loop removes it and keeps
    // the decoy and the kept entries.
    assert_eq!(merge().unwrap(), 0);
    std::fs::write(root.join("usr/share/t355/a"), b"decoy").unwrap();
    std::fs::write(&ebuild, original.replace("${T355_V:-1}", "${T355_V:-2}")).unwrap();
    assert_eq!(merge().unwrap(), 0);
    assert!(!root.join("usr/share/t355/a b.txt").exists());
    assert!(root.join("usr/share/t355/keep2").is_file());
    assert_eq!(
        std::fs::read_link(root.join("usr/share/t355/c d")).unwrap(),
        PathBuf::from("a b.txt")
    );
    assert!(root.join("usr/share/t355/sp dir/f").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/t355/a")).unwrap(),
        "decoy"
    );
}

/// An upgrade publishes the new version's vdb entry only after unmerging the
/// replaced same-slot version (Portage's `treewalk` position, after the replace loop).
#[test]
fn upgrade_publishes_the_new_entry_after_unmerging_the_old() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let v1 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild");
    let v2 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild");

    assert_eq!(
        run_merge(&v1, &root, &portage_tmpdir, &MergeOptions::default(), None).unwrap(),
        0
    );
    assert!(
        root.join("var/db/pkg/dev-libs/othersinslotpkg-1.0")
            .is_dir()
    );

    assert_eq!(
        run_merge(&v2, &root, &portage_tmpdir, &MergeOptions::default(), None).unwrap(),
        0
    );
    let vdb_cat = root.join("var/db/pkg/dev-libs");
    let entries: Vec<String> = portage_util::read_dir_entries(&vdb_cat)
        .unwrap()
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries, vec!["othersinslotpkg-2.0".to_string()]);
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/othersinslotpkg/shared.txt")).unwrap(),
        "shared, from 2.0\n"
    );
    assert!(
        root.join("usr/share/othersinslotpkg/only-in-v2.txt")
            .is_file()
    );
    assert!(
        !root
            .join("usr/share/othersinslotpkg/only-in-v1.txt")
            .exists()
    );
}

/// A sub-slotted (`SLOT="0/1"`) package must not collide with its own installed
/// copy on reinstall: collision checks compare against the vdb `SLOT`'s main slot.
#[test]
fn re_merging_a_sub_slotted_package_does_not_collide_with_its_own_files() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let ebuild = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/subslotfilepkg/subslotfilepkg-1.0.ebuild");
    let options = MergeOptions {
        features: "sandbox".to_string(),
        ..MergeOptions::default()
    };

    assert_eq!(
        run_merge(&ebuild, &root, &portage_tmpdir, &options, None).unwrap(),
        0
    );
    let slot =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/subslotfilepkg-1.0/SLOT")).unwrap();
    assert_eq!(slot.trim(), "0/1");

    assert_eq!(
        run_merge(&ebuild, &root, &portage_tmpdir, &options, None).unwrap(),
        0
    );
    assert!(root.join("usr/share/subslotfilepkg/hello.txt").is_file());
}

/// `run_merge` runs the `clean` phase after a merge unless `FEATURES=noclean`,
/// so no `${PORTAGE_BUILDDIR}` survives by default (Portage's `dblink.merge` tail).
#[test]
fn run_merge_post_cleans_the_builddir_unless_noclean() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let options = MergeOptions {
        features: "sandbox".to_string(),
        ..MergeOptions::default()
    };
    let status =
        run_merge(&ebuild, &root, &portage_tmpdir, &options, None).expect("run_merge succeeds");
    assert_eq!(status, 0);
    assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
    let builddir = portage_tmpdir.join("portage/dev-libs/mergepkg-1.0");
    assert!(
        !builddir.join(".installed").exists(),
        "post-merge clean must drop .installed"
    );
    assert!(
        !builddir.join("image").exists(),
        "post-merge clean must drop the image"
    );

    // With `noclean` the builddir survives for a later `qmerge`/inspection.
    let tmp2 = tempdir();
    let root2 = tmp2.join("root");
    let portage_tmpdir2 = tmp2.join("tmp");
    std::fs::create_dir_all(&root2).unwrap();
    std::fs::create_dir_all(&portage_tmpdir2).unwrap();
    let options = MergeOptions {
        features: "noclean".to_string(),
        ..MergeOptions::default()
    };
    let status =
        run_merge(&ebuild, &root2, &portage_tmpdir2, &options, None).expect("run_merge succeeds");
    assert_eq!(status, 0);
    assert!(root2.join("usr/share/mergepkg/hello.txt").is_file());
    assert!(
        portage_tmpdir2
            .join("portage/dev-libs/mergepkg-1.0/.installed")
            .exists(),
        "noclean must keep the builddir"
    );
}

#[test]
fn real_merge_protects_a_locally_modified_etc_file_via_the_full_cli_path() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    std::fs::write(root.join("etc/configpkg.conf"), b"admin's own edits\n").unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/configpkg/configpkg-1.0.ebuild");

    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    assert_eq!(
        std::fs::read_to_string(root.join("etc/configpkg.conf")).unwrap(),
        "admin's own edits\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_configpkg.conf")).unwrap(),
        "new content from configpkg\n"
    );
    let contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/configpkg-1.0/CONTENTS")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("obj /etc/configpkg.conf "))
    );
    assert!(!contents.contains("._cfg0000_configpkg.conf"));
}

#[test]
fn real_merge_protects_a_locally_modified_etc_symlink_via_the_full_cli_path() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    std::os::unix::fs::symlink("admins-own-target", root.join("etc/configsympkg.conf")).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/configsympkg/configsympkg-1.0.ebuild");

    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    assert_eq!(
        std::fs::read_link(root.join("etc/configsympkg.conf")).unwrap(),
        PathBuf::from("admins-own-target")
    );
    assert_eq!(
        std::fs::read_link(root.join("etc/._cfg0000_configsympkg.conf")).unwrap(),
        PathBuf::from("new-target")
    );
    let contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/configsympkg-1.0/CONTENTS"))
            .unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.starts_with("sym /etc/configsympkg.conf -> new-target"))
    );
    assert!(!contents.contains("._cfg0000_configsympkg.conf"));
}

#[test]
fn ordinary_collision_is_merged_over_with_both_collision_protect_and_protect_owned_off() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("collisionpkg-a"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("collisionpkg-a merges cleanly");

    let options = MergeOptions {
        protect_owned: false,
        ..MergeOptions::default()
    };
    let status = run_merge(
        &collision_fixture("collisionpkg-c"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect("run_merge should not itself error");
    assert_eq!(status, 0);
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-c\n"
    );
}

/// With `MergeOptions::default()` an owned-file collision aborts: `protect-owned`
/// is a default-on `FEATURES` token (see `MergeOptions::protect_owned`).
#[test]
fn ordinary_collision_aborts_by_portage_default_via_protect_owned() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("collisionpkg-a"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("collisionpkg-a merges cleanly");

    let err = run_merge(
        &collision_fixture("collisionpkg-c"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect_err("protect-owned is on by real default, so this should abort");
    assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{err}");
    assert!(err.contains("/usr/share/collisiontest/shared.txt"), "{err}");

    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-a\n"
    );
}

/// With `collision-protect` the merge aborts before writing anything and names
/// the owning package (Portage's `dblink._collision_protect`).
#[test]
fn ordinary_collision_aborts_the_merge_and_names_the_owner_when_collision_protect_is_on() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("collisionpkg-a"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("collisionpkg-a merges cleanly");

    let options = MergeOptions {
        collision_protect: true,
        ..MergeOptions::default()
    };
    let err = run_merge(
        &collision_fixture("collisionpkg-c"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect_err("collision-protect should abort the merge");
    assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{err}");
    assert!(err.contains("/usr/share/collisiontest/shared.txt"), "{err}");
    assert!(err.contains("NOT merged due to file collisions"), "{err}");

    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-a\n"
    );
    assert!(!root.join("var/db/pkg/dev-libs/collisionpkg-c-1.0").exists());
}

/// Symlink-over-directory always aborts (PMS 13.4), regardless of `FEATURES`:
/// only file-target symlinks are checked, never directory-target ones.
#[test]
fn symlink_over_directory_always_aborts_regardless_of_collision_protect() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("collisionpkg-a"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("collisionpkg-a merges cleanly");

    let err = run_merge(
        &collision_fixture("collisionpkg-b"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect_err("a symlink-over-directory violation should always abort");
    assert!(err.contains("PMS section 13.4"), "{err}");
    assert!(err.contains("/usr/share/collisiontest/adir"), "{err}");

    assert!(root.join("usr/share/collisiontest/adir").is_dir());
}

/// `protect-owned` alone aborts once `find_owners` identifies an owning package
/// (Portage's `dblink.merge` abort condition).
#[test]
fn protect_owned_alone_aborts_when_an_owner_is_identified() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("collisionpkg-a"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("collisionpkg-a merges cleanly");

    let options = MergeOptions {
        protect_owned: true,
        ..MergeOptions::default()
    };
    let err = run_merge(
        &collision_fixture("collisionpkg-c"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect_err("protect-owned alone should abort once an owner is identified");
    assert!(err.contains("dev-libs/collisionpkg-a-1.0"), "{err}");
    assert!(err.contains("/usr/share/collisiontest/shared.txt"), "{err}");
    assert!(err.contains("NOT merged due to file collisions"), "{err}");

    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-a\n"
    );
}

/// `protect-owned` alone must *not* abort on a stray file no vdb entry owns, unlike
/// `collision-protect` (Portage's "None of the installed packages ..." case).
#[test]
fn protect_owned_alone_does_not_abort_an_unclaimed_stray_file() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(root.join("usr/share/collisiontest")).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    std::fs::write(
        root.join("usr/share/collisiontest/shared.txt"),
        "a stray, unowned file\n",
    )
    .unwrap();

    let options = MergeOptions {
        protect_owned: true,
        ..MergeOptions::default()
    };
    let status = run_merge(
        &collision_fixture("collisionpkg-c"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect("protect-owned alone must not abort an unclaimed collision");
    assert_eq!(status, 0);
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-c\n"
    );
}

/// Baseline: with no preserve-libs registry entry this pair is an ordinary
/// collision-protect abort, proving a genuine collision.
#[test]
fn preservepkg_new_collides_ordinarily_without_a_registry_entry() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("preservepkg-old"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("preservepkg-old merges cleanly");

    let options = MergeOptions {
        collision_protect: true,
        ..MergeOptions::default()
    };
    let err = run_merge(
        &collision_fixture("preservepkg-new"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect_err("without a registry entry this is an ordinary collision");
    assert!(err.contains("dev-libs/preservepkg-old-1.0"), "{err}");
    assert!(err.contains("/usr/lib/preservedtest/libfoo.so.1"), "{err}");
}

/// Merging a FIFO node creates it at the destination and records a `fif` CONTENTS
/// line; re-merging is a no-op and unmerge leaves the node in place.
#[test]
fn real_merge_creates_a_real_fifo_and_records_a_fif_contents_line() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let ebuild = collision_fixture("fifopkg");
    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    let fifo_path = root.join("usr/lib/fifopkg/myfifo");
    let meta = std::fs::symlink_metadata(&fifo_path).expect("the real FIFO was created");
    assert!(meta.file_type().is_fifo(), "{:?}", meta.file_type());

    let contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/fifopkg-1.0/CONTENTS")).unwrap();
    assert!(
        contents.contains("fif /usr/lib/fifopkg/myfifo\n"),
        "{contents}"
    );

    // Plant something else there: an existing node must be left alone.
    std::fs::remove_file(&fifo_path).unwrap();
    std::fs::write(&fifo_path, b"not actually a fifo anymore").unwrap();
    let status = run_merge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("second run_merge succeeds");
    assert_eq!(status, 0);
    assert_eq!(
        std::fs::read_to_string(&fifo_path).unwrap(),
        "not actually a fifo anymore",
        "an existing node at that path must be left completely alone"
    );

    // Restore a FIFO first so the leave-in-place assertion is meaningful.
    std::fs::remove_file(&fifo_path).unwrap();
    // SAFETY: `c_path` is a CString that outlives the `mkfifo`
    // call; the call itself takes only the NUL-terminated pointer
    // plus a plain mode int.
    unsafe {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
        assert_eq!(libc::mkfifo(c_path.as_ptr(), 0o644), 0);
    }

    let unmerge_status = crate::ebuild_unmerge::run_unmerge(
        &ebuild,
        &root,
        &portage_tmpdir,
        &crate::ebuild_unmerge::UnmergeOptions::default(),
    )
    .expect("run_unmerge succeeds");
    assert_eq!(unmerge_status, 0);
    // Portage 3.0.82.2, S0 cell F1a (not from portuale output): under
    // default `FEATURES` the orphan arm unlinks the fifo (`<<< fif`).
    assert!(
        std::fs::symlink_metadata(&fifo_path).is_err(),
        "default FEATURES unlinks a fifo on unmerge"
    );
    assert!(
        !root.join("var/db/pkg/dev-libs/fifopkg-1.0").exists(),
        "the vdb entry itself is still removed, same as any other unmerge"
    );
}

/// `env_update()` runs after `postinst`, so it regenerates its outputs from the
/// merge's own just-installed `/etc/env.d/50-envupdatetest` (Portage's order).
#[test]
fn real_merge_regenerates_env_update_outputs_from_its_own_env_d_entry() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let status = run_merge(
        &env_update_fixture(),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    assert!(root.join("etc/env.d/50-envupdatetest").is_file());

    let ld_so_conf = std::fs::read_to_string(root.join("etc/ld.so.conf")).unwrap();
    assert!(
        ld_so_conf.contains("/usr/lib/envupdatetest"),
        "{ld_so_conf}"
    );

    let profile_env = std::fs::read_to_string(root.join("etc/profile.env")).unwrap();
    assert!(
        profile_env.contains("export ENVUPDATETEST_VAR='hello from envupdatetest'"),
        "{profile_env}"
    );
    assert!(!profile_env.contains("LDPATH"));

    let csh_env = std::fs::read_to_string(root.join("etc/csh.env")).unwrap();
    assert!(
        csh_env.contains("setenv ENVUPDATETEST_VAR 'hello from envupdatetest'"),
        "{csh_env}"
    );

    let systemd_env =
        std::fs::read_to_string(root.join("etc/environment.d/10-gentoo-env.conf")).unwrap();
    assert!(
        systemd_env.contains("ENVUPDATETEST_VAR=hello from envupdatetest"),
        "{systemd_env}"
    );
}

/// `env_update()` invokes the target `ROOT`'s own `<ROOT>/sbin/ldconfig`, never a
/// host `PATH` lookup; a marker-writing fake there proves the invocation.
#[test]
fn real_merge_invokes_a_real_root_scoped_ldconfig_when_one_is_present() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    std::fs::create_dir_all(root.join("sbin")).unwrap();
    std::fs::write(
        root.join("sbin/ldconfig"),
        "#!/bin/sh\necho \"$@\" > \"$3/ldconfig-was-invoked\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        root.join("sbin/ldconfig"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let status = run_merge(
        &env_update_fixture(),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    let marker = std::fs::read_to_string(root.join("ldconfig-was-invoked"))
        .expect("the real ROOT-scoped ldconfig binary was really invoked");
    assert!(marker.contains("-X"), "{marker}");
    assert!(marker.contains("-r"), "{marker}");
}

/// `NEEDED.ELF.2` (the post-install `scanelf` output) lands in the vdb entry,
/// copied out of `build-info` like Portage's `treewalk` does.
#[test]
fn real_merge_copies_a_real_needed_elf2_into_the_vdb() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let status = run_merge(
        &collision_fixture("elfpkg"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0);

    let needed = std::fs::read_to_string(root.join("var/db/pkg/dev-libs/elfpkg-1.0/NEEDED.ELF.2"))
        .expect("NEEDED.ELF.2 should have been copied into the real vdb entry");
    assert!(
        needed.contains("/usr/bin/true"),
        "should report the real installed binary's own path: {needed}"
    );

    // Same file through `NeededEntry::parse_file`, not just hand-crafted lines.
    let entries = crate::needed_elf::NeededEntry::parse_file(&needed);
    let entry = entries
        .iter()
        .find(|e| e.filename == "/usr/bin/true")
        .expect("the real parser should find the real installed binary's own entry");
    assert_eq!(entry.arch, "X86_64");
    assert!(
        entry.needed.iter().any(|n| n.starts_with("libc.so")),
        "{:?}",
        entry.needed
    );
    // The vdb copy carries the 6th multilib-category field from the soname rewrite.
    assert_eq!(entry.multilib_category.as_deref(), Some("x86_64"));
    assert!(
        needed.lines().all(|l| l.split(';').count() >= 6),
        "every rewritten line must carry the category field: {needed}"
    );

    let all = crate::needed_elf::read_all_needed_entries(&root);
    let (cpv, cpv_entries) = all
        .iter()
        .find(|(cpv, _)| cpv == "dev-libs/elfpkg-1.0")
        .expect("read_all_needed_entries should find the real installed package");
    assert_eq!(cpv, "dev-libs/elfpkg-1.0");
    assert!(cpv_entries.iter().any(|e| e.filename == "/usr/bin/true"));

    let map = crate::needed_elf::rebuild(&root, &all);
    let key = crate::needed_elf::obj_key(&root, "/usr/bin/true");
    let props = map
        .obj_properties
        .get(&key)
        .expect("rebuild should index the real installed binary");
    assert_eq!(props.owner, "dev-libs/elfpkg-1.0");
    assert!(!props.needed.is_empty(), "{:?}", props.needed);
    let consumed_somewhere = map.libs.values().any(|sonames| {
        sonames
            .values()
            .any(|soname_map| soname_map.consumers.contains(&key))
    });
    assert!(consumed_somewhere);
}

/// Unmerging a library still needed by an installed consumer preserves the file
/// on disk and records it in the on-disk registry (Portage's preserve-libs).
#[test]
fn real_unmerge_preserves_a_still_needed_shared_library() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let lib_ebuild = collision_fixture("libpreservetest");
    let consumer_ebuild = collision_fixture("consumepreservetest");

    let lib_status = run_merge(
        &lib_ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge (library) succeeds");
    assert_eq!(lib_status, 0);

    let consumer_status = run_merge(
        &consumer_ebuild,
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge (consumer) succeeds");
    assert_eq!(consumer_status, 0);

    let lib_path = root.join("usr/lib/libpreservetest.so.1");
    assert!(
        lib_path.is_file(),
        "sanity: the library was really installed"
    );

    let unmerge_status = crate::ebuild_unmerge::run_unmerge(
        &lib_ebuild,
        &root,
        &portage_tmpdir,
        &crate::ebuild_unmerge::UnmergeOptions::default(),
    )
    .expect("run_unmerge succeeds");
    assert_eq!(unmerge_status, 0);

    assert!(
        lib_path.is_file(),
        "the still-needed shared library must survive unmerge, preserved on disk"
    );
    assert!(
        !root
            .join("var/db/pkg/dev-libs/libpreservetest-1.0")
            .exists(),
        "the vdb entry itself is still removed, same as any other unmerge"
    );

    let registry = read_plib_registry(&root);
    let preserved = registry.preserved_libs();
    let paths = preserved
        .get("dev-libs/libpreservetest-1.0")
        .expect("the real registry should record this package as the new keeper");
    assert!(
        paths.iter().any(|p| p == "/usr/lib/libpreservetest.so.1"),
        "{paths:?}"
    );

    // Once the last consumer is gone the preserved library is orphaned and removed.
    let consumer_unmerge = crate::ebuild_unmerge::run_unmerge(
        &consumer_ebuild,
        &root,
        &portage_tmpdir,
        &crate::ebuild_unmerge::UnmergeOptions::default(),
    )
    .expect("run_unmerge (consumer) succeeds");
    assert_eq!(consumer_unmerge, 0);

    assert!(
        !lib_path.exists(),
        "the preserved library must be removed once nothing links it any more"
    );
    assert!(
        read_plib_registry(&root).entries.is_empty(),
        "the registry must be empty after the last consumer is unmerged"
    );
}

/// Baseline: with the inert default `config_root` no config/USE resolution runs,
/// so this pair is an ordinary collision-protect abort, proving a genuine collision.
#[test]
fn mergeblockerpkg_collides_ordinarily_without_config_resolution() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("mergeblockedbypkg"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("mergeblockedbypkg merges cleanly");

    let options = MergeOptions {
        collision_protect: true,
        ..MergeOptions::default()
    };
    let err = run_merge(
        &collision_fixture("mergeblockerpkg"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect_err("without config resolution this is an ordinary collision");
    assert!(err.contains("dev-libs/mergeblockedbypkg-1.0"), "{err}");
    assert!(
        err.contains("/usr/share/mergeblockertest/shared.txt"),
        "{err}"
    );
}

/// `mergeblockerpkg`'s `RDEPEND="!dev-libs/mergeblockedbypkg"` matches the installed
/// package via config/USE resolution, so the collision is excluded (Portage's
/// `mypkglist = others_in_slot + blockers`).
#[test]
fn mergeblockerpkg_excludes_the_collision_via_a_real_blocker_atom() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("mergeblockedbypkg"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("mergeblockedbypkg merges cleanly");

    let options = MergeOptions {
        collision_protect: true,
        config_root: fixtures_root(),
        ..MergeOptions::default()
    };
    let status = run_merge(
        &collision_fixture("mergeblockerpkg"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect("a blocker-excluded collision is not an abort");
    assert_eq!(status, 0);
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/mergeblockertest/shared.txt")).unwrap(),
        "hello from mergeblockerpkg\n"
    );
}
