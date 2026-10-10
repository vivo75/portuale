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

    // The ordering markers `mergepkg`'s own hooks leave under `${T}`
    // are asserted below, so this merge keeps its build state the way
    // real `FEATURES=noclean` does -- the default post-merge clean is
    // pinned by `run_merge_post_cleans_the_builddir_unless_noclean`.
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

    // Real dblink.merge()'s own atomic dbtmpdir-then-rename: no
    // MERGING_IDENTIFIER-prefixed temp directory should survive a
    // successful merge.
    assert!(
        !root
            .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
            .exists()
    );

    // Real pkg_preinst/pkg_postinst ordering proof: the fixture's own
    // hooks only touch these markers if, respectively, the merged
    // file was *not yet* visible under ${ROOT} (preinst) and *was
    // already* visible, vdb entry included (postinst) -- see
    // mergepkg-1.0.ebuild's own pkg_preinst/pkg_postinst.
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

/// A *computed* `SLOT="$(ver_cut 1)"` (task #149) is read back as its
/// **evaluated** value -- the value real Portage's own metadata
/// generation and `dblink.treewalk` both use. Expected value comes
/// from real Portage: the mpdecimal probe on the container test bed
/// (`4.0.1` -> `SLOT=4` in the real vdb, and `build-info/SLOT` = `4`
/// from the install phase writing the evaluated `${SLOT}`); for this
/// fixture `ver_cut` of `1.0` evaluates to `1`.
///
/// Before the fix, `merge_after_install` derived `full_slot` by
/// regex-parsing the *raw* ebuild text (`parse_slot`), so the literal
/// `$(ver_cut 1)` itself landed in the vdb -- and every slot-keyed
/// consumer (`installed_instance_pf`/`find_collisions`/
/// `blocked_installed_packages`/`unmerge_replaced_same_slot`, all fed
/// `full_slot.split('/')`) keyed off the same corrupt value. On a
/// real image whose `dev-libs/mpdecimal-4.0.1` was installed by real
/// portage (evaluated `SLOT=4`), that mismatch made `find_collisions`
/// refuse to credit the installed instance, so a reinstall under
/// `FEATURES=protect-owned` aborted with `NOT merged due to file
/// collisions` -- the exact #149 symptom. Here the reinstall must not
/// abort under `MergeOptions::default()` (`protect_owned: true`), and
/// the vdb `SLOT` must record the evaluated `1`.
#[test]
fn computed_slot_is_evaluated_in_the_vdb_and_a_reinstall_does_not_abort_under_protect_owned() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/compuslotpkg/compuslotpkg-1.0.ebuild");

    // First install: lands the file and a vdb entry under the real
    // default FEATURES (protect_owned: true).
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

    // Reinstall under the same real-default protect_owned: the
    // installed instance owns its own file, so this must not abort.
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
    // `dev-libs/packagepkg` has a real md5-cache entry with
    // `RDEPEND="dev-libs/samepkg"`. Real `treewalk()` copies every
    // `build-info` file into the vdb, and
    // `ebuild_phases::write_post_install_metadata` now writes the
    // dependency-string metadata files there -- so the merged vdb
    // entry carries `RDEPEND`/`EAPI`/`KEYWORDS`/…, not just the
    // former `CATEGORY`/`SLOT`/`repository`/`CONTENTS`/`COUNTER`.
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
    // The bundled ebuild + saved environment come across too (real
    // `build-info` members).
    assert!(vdb.join("packagepkg-1.0.ebuild").is_file());
    assert!(vdb.join("environment.bz2").is_file());
    // An empty md5-cache value (`DEPEND=`) is not written at all
    // (real portage unlinks it; `bin/phase-functions.sh` never wrote
    // it).
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
    // Nothing was written at all.
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

    // Real qmerge assumes a prior real `install` already populated
    // ${D} -- run only the install phase directly here (not
    // run_merge, which would also run qmerge's own merge logic).
    let install_status = ebuild_phases::run_commands(
        &ebuild,
        &["install"],
        &root,
        &portage_tmpdir,
        &MergeOptions::default().distdir,
        false,
        &MergeOptions::default().config_root,
        ebuild_phases::ShellBackend::default(),
        &[],
    )
    .expect("install phase succeeds");
    assert_eq!(install_status, 0);
    // Real bin/phase-functions.sh's own __dyn_install already leaves
    // this marker behind -- confirms the test's own setup is
    // faithful to what a real prior `ebuild <file> install` run
    // leaves for qmerge to find.
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
    // Real pkg_preinst/pkg_postinst still run -- qmerge only skips
    // the install phase itself, not merge()'s own body.
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
    // Still a single, intact entry -- not a leftover-plus-new-copy.
    assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
    assert!(
        !root
            .join("var/db/pkg/dev-libs/-MERGING-mergepkg-1.0")
            .exists()
    );
}

/// Backlog #183: an upgrade publishes the new version's vdb entry only
/// after the replaced same-slot version is unmerged -- real
/// `treewalk()`'s own `_movefile(self.dbtmpdir, self.dbpkgdir)`
/// position, after the replace loop. Observable contract: the old
/// entry is gone, the new one is live with the new content, and no
/// temporary survives. (The in-between invisibility itself is what the
/// `l32` C4 candidate run checks live: the kill lands while the new
/// entry is still `-MERGING-*`.)
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

/// L3 smoke finding: a package whose ebuild `SLOT` carries a sub-slot
/// (`SLOT="0/1"`) must not collide with its own installed copy on a
/// reinstall / `-e` merge. `find_collisions` and
/// `installed_instance_pf` compare against the vdb `SLOT` file's
/// *main* slot, so passing the ebuild's full `slot/sub_slot` made
/// every file look foreign and `FEATURES=protect-owned` (real's
/// default) aborted the merge.
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

    // Same version, same slot: every file is owned by this exact
    // package, so the second merge must not abort.
    assert_eq!(
        run_merge(&ebuild, &root, &portage_tmpdir, &options, None).unwrap(),
        0
    );
    assert!(root.join("usr/share/subslotfilepkg/hello.txt").is_file());
}

/// Backlog #42: real `dblink.merge()`'s tail (`vartree.py:6183-6198`)
/// runs the `clean` phase after a merge unless `FEATURES=noclean`.
/// The `emerge` paths that reach `run_merge` therefore leave no
/// `${PORTAGE_BUILDDIR}` behind (which is what stopped a later `-B`
/// from repackaging an already-`instprep`ped image, #38 S4); with
/// `noclean` the build state survives.
#[test]
fn run_merge_post_cleans_the_builddir_unless_noclean() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

    // Default (no noclean): the builddir is cleaned after the merge.
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

    // `FEATURES=noclean`: real keeps the builddir (bug #704866's
    // sibling gate); `.installed` is still there for a later
    // `qmerge`/inspection.
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
    // Simulate a pre-existing, locally-modified /etc file -- as if
    // this package (or an earlier version of it) had installed a
    // default that the admin then edited by hand.
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

    // The real, logical /etc/configpkg.conf is never touched.
    assert_eq!(
        std::fs::read_to_string(root.join("etc/configpkg.conf")).unwrap(),
        "admin's own edits\n"
    );
    // The new content the ebuild wanted to install lands in a real
    // ._cfg0000_ sibling instead.
    assert_eq!(
        std::fs::read_to_string(root.join("etc/._cfg0000_configpkg.conf")).unwrap(),
        "new content from configpkg\n"
    );
    // The vdb's own CONTENTS still considers /etc/configpkg.conf
    // (the logical path) this package's own -- not the ._cfg
    // variant.
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
    // Simulate a pre-existing, locally-modified /etc symlink -- as if
    // this package (or an earlier version of it) had installed a
    // default target the admin then repointed by hand.
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

    // The real, logical /etc/configsympkg.conf is never touched.
    assert_eq!(
        std::fs::read_link(root.join("etc/configsympkg.conf")).unwrap(),
        PathBuf::from("admins-own-target")
    );
    // The new target the ebuild wanted to install lands in a real
    // ._cfg0000_ sibling instead.
    assert_eq!(
        std::fs::read_link(root.join("etc/._cfg0000_configsympkg.conf")).unwrap(),
        PathBuf::from("new-target")
    );
    // The vdb's own CONTENTS still considers /etc/configsympkg.conf
    // (the logical path) this package's own -- not the ._cfg
    // variant.
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

/// `MergeOptions::default()`, no overrides at all: an ordinary file
/// collision with an identifiable owner now aborts, matching real
/// portage's own real out-of-the-box behavior (real `protect-owned`
/// is a default-on `FEATURES` token, see `MergeOptions::
/// protect_owned`'s own doc comment). Complements
/// `protect_owned_alone_aborts_when_an_owner_is_identified` below,
/// which proves the same real logic via an explicit `protect_owned:
/// true` rather than relying on the real default.
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

    // Nothing was written: the file is still collisionpkg-a's own.
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-a\n"
    );
}

/// `FEATURES=collision-protect` on: real `dblink._collision_
/// protect`'s own abort -- `collisionpkg-c` would overwrite
/// `collisionpkg-a`'s own, different-package `shared.txt`, so the
/// merge aborts *before* writing anything (the file is left exactly
/// as `collisionpkg-a` installed it) and the error names
/// `collisionpkg-a` as the real owning package (`find_owners`).
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

    // Nothing was written: the file is still collisionpkg-a's own,
    // and collisionpkg-c's own vdb entry was never created.
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/collisiontest/shared.txt")).unwrap(),
        "hello from collisionpkg-a\n"
    );
    assert!(!root.join("var/db/pkg/dev-libs/collisionpkg-c-1.0").exists());
}

/// Real PMS 13.4's own symlink-over-directory ban: unconditional,
/// regardless of `FEATURES` -- `collisionpkg-b` installs a
/// file-target (dangling `nowhere`) symlink exactly where
/// `collisionpkg-a` already installed a real directory (`adir`),
/// which aborts the merge even with `collision_protect: false`
/// (`MergeOptions::default()`). Dangling/file-target symlinks are
/// the ones real `os.walk` lists in `files` (hence in `linklist`);
/// directory-target symlinks are in `dirs` and never checked -- see
/// the next test.
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

    // The real directory collisionpkg-a installed is still a real
    // directory -- never replaced by collisionpkg-b's own symlink.
    assert!(root.join("usr/share/collisiontest/adir").is_dir());
}

/// `FEATURES=protect-owned` alone (no `collision-protect`): real
/// `dblink.merge()`'s own separate abort condition aborts once
/// `find_owners` actually identifies an owning package for the
/// collision -- `collisionpkg-c` colliding with `collisionpkg-a`'s
/// own, different-package `shared.txt` is exactly that case.
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

/// Real "None of the installed packages claim the file(s)" case:
/// `FEATURES=protect-owned` alone must *not* abort when the
/// colliding destination is a stray file with no owning vdb entry
/// at all -- the distinguishing behavior from `collision-protect`,
/// which would abort unconditionally on any collision.
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

/// Sanity baseline (portuale's own "fixtures must actually
/// distinguish the new behavior" rule): with no preserve-libs
/// registry entry at all, `preservepkg-new` colliding with
/// `preservepkg-old` on the exact same path is an ordinary
/// collision-protect abort, same as `collisionpkg-a`/`-c` above --
/// proving the fixture pair is a genuine collision before the next
/// test shows the registry excluding it.
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

/// Real, end-to-end proof of `merge_tree`'s own new `fif` branch:
/// merging a real FIFO node (created via real `mkfifo(1)`, no
/// special privilege needed unlike a device node) actually creates a
/// real FIFO at the destination and records a real `fif` `CONTENTS`
/// line with no digest/mtime/target field at all (real
/// `_format_contents_line(node_type="fif", abs_path=myrealdest)`).
/// Re-merging over an already-existing node is a real no-op (real
/// `if mydmode is None:` only creates when nothing's there yet) --
/// proven by planting an unrelated real file at the destination
/// first and confirming it survives untouched. Unmerging leaves the
/// node in place too, matching real `_unmerge_pkgfiles()`'s own
/// `"fif"`/`"dev"` branches never calling `unlink()` at all (see
/// `ebuild_unmerge::remove_contents`'s own doc comment) -- this
/// portuale's own vdb entry is still removed as normal either way.
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

    // Re-merging (a real reinstall) must not recreate the node --
    // plant something else there and confirm it survives.
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

    // Restore a real FIFO before unmerging, so the "leave it in
    // place" assertion below is actually meaningful.
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
    assert!(
        std::fs::symlink_metadata(&fifo_path)
            .map(|m| m.file_type().is_fifo())
            .unwrap_or(false),
        "real portage never unlinks a fif/dev CONTENTS entry on unmerge"
    );
    assert!(
        !root.join("var/db/pkg/dev-libs/fifopkg-1.0").exists(),
        "the vdb entry itself is still removed, same as any other unmerge"
    );
}

/// Real `merge()`'s own ordering: `env_update()` runs after
/// `postinst`, so it sees the merge's *own* just-installed
/// `/etc/env.d/50-envupdatetest` (the fixture installs its own env.d
/// entry, not a separately-merged package's) and regenerates
/// `/etc/profile.env`/`/etc/csh.env`/`/etc/environment.d/
/// 10-gentoo-env.conf`/`/etc/ld.so.conf` from it.
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
    // LDPATH itself never appears in profile.env -- only ld.so.conf.
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

/// Real `env_update()` invokes the *target `ROOT`'s own*
/// `<ROOT>/sbin/ldconfig` (never a host `PATH` lookup -- see
/// `env_update.rs`'s own module doc comment). Seeding a fake,
/// marker-writing executable there before merging proves this
/// portuale's own real subprocess invocation, the same "prove it with a
/// marker file" style already used for `pkg_preinst`/`pkg_postinst`
/// ordering elsewhere in this file.
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

/// Real, end-to-end proof that `NEEDED.ELF.2` -- real, unmodified
/// `bin/misc-functions.sh install_qa_check`'s own real `scanelf`-
/// driven output, generated by the new post-install misc-functions
/// step (`ebuild_phases::run_commands_async`) -- actually lands in
/// the real vdb entry, matching real `dblink.merge()`'s own
/// `treewalk()` (`vartree.py:4912-4913`) copying it out of
/// `build-info` (see `populate_vdb_tmp`'s own doc comment for why
/// portuale copies only this one build-info file, not the whole
/// directory). Installs a real, dynamically-linked ELF binary
/// (`/bin/true`, whatever the real host machine actually has) so
/// real `scanelf` has something genuine to report on.
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

    // `crate::needed_elf::NeededEntry::parse_file` end to end against
    // this real, live `scanelf`-generated vdb file -- not just the
    // hand-crafted lines its own unit tests already cover.
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
    // #39: the install-phase `_post_src_install_soname_symlinks`
    // rewrite adds real's 6th multilib-category field, so the vdb
    // copy carries six `;`-separated fields.
    assert_eq!(entry.multilib_category.as_deref(), Some("x86_64"));
    assert!(
        needed.lines().all(|l| l.split(';').count() >= 6),
        "every rewritten line must carry the category field: {needed}"
    );

    // `crate::needed_elf::read_all_needed_entries` end to end: the
    // real vdb walk finds this exact package's own real entry too.
    let all = crate::needed_elf::read_all_needed_entries(&root);
    let (cpv, cpv_entries) = all
        .iter()
        .find(|(cpv, _)| cpv == "dev-libs/elfpkg-1.0")
        .expect("read_all_needed_entries should find the real installed package");
    assert_eq!(cpv, "dev-libs/elfpkg-1.0");
    assert!(cpv_entries.iter().any(|e| e.filename == "/usr/bin/true"));

    // `crate::needed_elf::rebuild` end to end: the real installed
    // binary's own real DT_NEEDED entries (whatever the real host's
    // own /bin/true actually links against -- typically libc.so.6)
    // get indexed as real consumers, keyed by its own real multilib
    // category.
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

/// Real, end-to-end proof of the full preserve-libs pipeline this
/// portuale's own `preserve_libs_on_unmerge` (see its own doc comment
/// above) actually wires into real `ebuild_unmerge::run_unmerge`:
/// merge a real library, merge a real consumer that's genuinely
/// linked against it (real `DT_NEEDED: libpreservetest.so.1`, baked
/// in by real `gcc` at fixture build time -- see the two fixture
/// ebuilds' own comments for why each independently rebuilds a
/// throwaway same-sonamed copy to link against), then unmerge the
/// library while the consumer is still installed. Real `_find_libs_
/// to_preserve` should find the still-installed consumer's own
/// `NEEDED.ELF.2` entry still needing this soname, so the real
/// library file must survive on disk (filtered out of `CONTENTS`
/// before `remove_contents`'s own per-file loop ever sees it -- see
/// `ebuild_unmerge::remove_contents`'s own `preserved_paths` doc
/// comment) and the real on-disk registry must record it under this
/// exact package's own real `category/pn:slot` key.
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

    // Real `_prune_plib_registry()`'s tail: once the last consumer is
    // gone too, the preserved library is orphaned and the next
    // merge/unmerge deletes it and clears the registry (here: the
    // consumer's own unmerge does it, `unmerge_no_replacement=True`).
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

/// Sanity baseline (portuale's own "fixtures must actually
/// distinguish the new behavior" rule): with `MergeOptions::default()`
/// (its own deliberately-inert `config_root` sentinel, see that
/// field's own doc comment), real config/USE resolution never even
/// attempts, so `blocked_installed_packages` degrades to an empty
/// set -- `mergeblockerpkg` colliding with `mergeblockedbypkg` on
/// `/usr/share/mergeblockertest/shared.txt` is an ordinary
/// collision-protect abort, exactly like `collisionpkg-a`/`-c`
/// above, proving the fixture pair is a genuine collision before the
/// next test shows real blocker resolution excluding it.
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

/// Real `mypkglist = others_in_slot + blockers`: `mergeblockerpkg`'s own
/// real `RDEPEND="!dev-libs/mergeblockedbypkg"` -- flattened via real
/// config/USE resolution rooted at `config_root` (the real
/// `fixtures` tree, which has its own real `repos.conf`) --
/// matches the already-installed `mergeblockedbypkg`, so the collision on
/// `/usr/share/mergeblockertest/shared.txt` is excluded even with
/// `collision_protect: true`, and `mergeblockerpkg` takes over the file.
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
