use super::*;

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
