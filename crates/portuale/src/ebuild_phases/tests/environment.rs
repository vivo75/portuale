use super::*;

#[test]
fn restrict_and_properties_reads_and_use_reduces_the_real_md5_cache_entry() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let portage_tmpdir =
        TempDir::new("ebuild-phases-test-restrict_and_properties_reads_real_md5_cache").keep();
    let env = compute_environment(
        &repo_root.join("dev-libs/propertiespkg/propertiespkg-1.0.ebuild"),
        &portage_tmpdir,
    )
    .expect("real fixture parses");
    let (restrict, properties) = restrict_and_properties(&env, &std::collections::HashSet::new());
    assert_eq!(restrict, "");
    assert_eq!(properties, "live");
}

#[test]
fn restrict_and_properties_is_empty_outside_any_repo_checkout() {
    let tmp = TempDir::new("ebuild-phases-test-restrict_and_properties_outside_repo").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("standalone");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("standalone-1.0.ebuild");
    std::fs::write(&ebuild, "EAPI=8\nSLOT=\"0\"\n").unwrap();
    let env = compute_environment(&ebuild, &tmp).expect("standalone ebuild parses");
    assert_eq!(
        restrict_and_properties(&env, &std::collections::HashSet::new()),
        (String::new(), String::new())
    );
}

/// `compute_environment` reads `INHERITED` from the ebuild's own
/// `metadata/md5-cache` entry (real `porttree.py:872`) so
/// `phase_env_vars` can export it and `bin/ebuild.sh` can snapshot it
/// into `__INHERITED_QA_CACHE` -- suppressing the spurious
/// `Eclass '…' inherited illegally` QA notice on a phase re-source.
/// `dev-libs/eclasspkg`'s fixture cache carries `INHERITED=pilotcheck`.
#[test]
fn compute_environment_reads_inherited_from_the_md5_cache() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild");
    let env = compute_environment(&ebuild_path, Path::new("/var/tmp/portage")).unwrap();
    assert_eq!(env.inherited.as_deref(), Some("pilotcheck"));

    // A standalone ebuild outside any repo -> no md5-cache -> None.
    let tmp = TempDir::new("ebuild-phases-test-inherited-none").keep();
    let pkg_dir = tmp.join("dev-libs/standalone");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let solo = pkg_dir.join("standalone-1.0.ebuild");
    std::fs::write(&solo, "EAPI=8\nSLOT=0\n").unwrap();
    let env = compute_environment(&solo, Path::new("/var/tmp/portage")).unwrap();
    assert_eq!(env.inherited, None);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn compute_environment_links_builddir_files_to_the_repo_filesdir() {
    // Real `_prepare_fake_filesdir`: FILESDIR is builddir/files, a
    // symlink to the ebuild's own repo `files/`. Without it every
    // ebuild patch via `eapply` dies (L2 S5).
    let tmp = TempDir::new("ebuild-phases-test-filesdir").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("repo/dev-libs/patchpkg");
    std::fs::create_dir_all(pkg_dir.join("files")).unwrap();
    std::fs::write(pkg_dir.join("patchpkg-1.0.ebuild"), "EAPI=8\nSLOT=0\n").unwrap();
    std::fs::write(pkg_dir.join("files/fix.patch"), "diff\n").unwrap();
    let portage_tmp = tmp.join("ptmp");
    let env = compute_environment(&pkg_dir.join("patchpkg-1.0.ebuild"), &portage_tmp).unwrap();
    let expected = pkg_dir.join("files");
    assert_eq!(std::fs::read_link(env.filesdir()).unwrap(), expected);
    assert!(env.filesdir().join("fix.patch").is_file());

    // Re-running must be idempotent (real unlinks a stale target).
    let env2 = compute_environment(&pkg_dir.join("patchpkg-1.0.ebuild"), &portage_tmp).unwrap();
    assert_eq!(std::fs::read_link(env2.filesdir()).unwrap(), expected);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn compute_environment_links_builddir_files_even_without_a_repo_filesdir() {
    // Backlog #94: real `_prepare_fake_filesdir` links
    // unconditionally -- a missing repo `files/` must yield a
    // dangling symlink, never a plain directory (whose `rm -f` in
    // `__dyn_clean` fails with "Is a directory" and aborts the
    // rest of the cleanup under `set -e`, leaving `/var/tmp`
    // residue -- host `emerge -1 sys-libs/timezone-data`).
    let tmp = TempDir::new("ebuild-phases-test-filesdir-missing").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("repo/sys-libs/notimezonefiles");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("notimezonefiles-1.0.ebuild"),
        "EAPI=8\nSLOT=0\n",
    )
    .unwrap();
    assert!(!pkg_dir.join("files").exists());
    let portage_tmp = tmp.join("ptmp");
    let env =
        compute_environment(&pkg_dir.join("notimezonefiles-1.0.ebuild"), &portage_tmp).unwrap();
    assert_eq!(
        std::fs::read_link(env.filesdir()).unwrap(),
        pkg_dir.join("files")
    );
    assert!(
        std::fs::symlink_metadata(env.filesdir())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // ... and `create_directories` must leave the dangling link
    // alone instead of failing on it (or materialising the target).
    create_directories(&env).unwrap();
    assert!(
        std::fs::symlink_metadata(env.filesdir())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!pkg_dir.join("files").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn compute_environment_replaces_a_stale_plain_filesdir_with_the_link() {
    // Migration path for pre-fix residue: a plain directory left at
    // builddir/files (the shape `__dyn_clean` choked on) becomes the
    // symlink again instead of erroring.
    let tmp = TempDir::new("ebuild-phases-test-filesdir-stale-dir").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("repo/dev-libs/dirpkg");
    std::fs::create_dir_all(pkg_dir.join("files")).unwrap();
    std::fs::write(pkg_dir.join("dirpkg-1.0.ebuild"), "EAPI=8\nSLOT=0\n").unwrap();
    let portage_tmp = tmp.join("ptmp");
    // Pre-seed the stale plain dir where the builddir will be.
    let builddir = portage_tmp.join("portage/dev-libs/dirpkg-1.0");
    std::fs::create_dir_all(builddir.join("files")).unwrap();
    let env = compute_environment(&pkg_dir.join("dirpkg-1.0.ebuild"), &portage_tmp).unwrap();
    assert_eq!(
        std::fs::read_link(env.filesdir()).unwrap(),
        pkg_dir.join("files")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Real `_prepare_fake_distdir` (`prepare_build_dirs.py:518-549`):
/// entries that are not a symlink named in `A` (a stale symlink, a
/// stray directory, a plain file) are removed, a symlink with the wrong
/// target is recreated, a correct one is kept.
#[test]
fn prepare_fake_distdir_prunes_and_relinks_like_portage() {
    use std::os::unix::fs::symlink;
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-prep-fake",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let env = compute_environment(&ebuild_path, &portage_tmpdir).unwrap();
    let fake = env.fake_distdir();
    std::fs::create_dir_all(fake.join("stray-dir/inner")).unwrap();
    std::fs::write(fake.join("stray-file"), b"x").unwrap();
    symlink("/real/stale", fake.join("stale")).unwrap();
    symlink("/old/wrong", fake.join("wrong")).unwrap();
    symlink("/real/keep", fake.join("keep")).unwrap();
    let before = std::fs::symlink_metadata(fake.join("keep")).unwrap();

    let alist = vec!["keep".to_string(), "wrong".to_string(), "new".to_string()];
    prepare_fake_distdir(&env, Path::new("/real"), &alist).unwrap();

    let mut names: Vec<String> = std::fs::read_dir(&fake)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["keep", "new", "wrong"]);
    for x in ["keep", "new", "wrong"] {
        assert_eq!(
            std::fs::read_link(fake.join(x)).unwrap(),
            Path::new("/real").join(x)
        );
    }
    // The correct link was left alone, not recreated.
    use std::os::unix::fs::MetadataExt;
    let after = std::fs::symlink_metadata(fake.join("keep")).unwrap();
    assert_eq!(before.ino(), after.ino());

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// `compute_environment` derives `PORTAGE_BUILDDIR` as
/// `<tmpdir>/portage/<cat>/<pf>`, so a matched per-package tmpdir
/// puts the whole build tree under it (S0 cell D's
/// `PORTAGE_BUILDDIR` half).
#[test]
fn compute_environment_builds_the_builddir_under_the_given_tmpdir() {
    let probe = TempDir::new("ebuild-phases-test-compute_environment_builddir").keep();
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
    let env = compute_environment(
        &repo_root.join("dev-libs/envdumppkg/envdumppkg-1.0.ebuild"),
        &probe,
    )
    .expect("fixture parses");
    assert_eq!(
        env.portage_builddir,
        probe.join("portage/dev-libs/envdumppkg-1.0")
    );
    assert_eq!(
        env.portage_tmpdir, probe,
        "the resolved tmpdir is what the phase exports as PORTAGE_TMPDIR"
    );
    std::fs::remove_dir_all(&probe).ok();
}
