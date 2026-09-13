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

/// Reads `INHERITED` from the md5-cache entry so phase re-sourcing
/// skips the spurious `Eclass inherited illegally` QA notice.
#[test]
fn compute_environment_reads_inherited_from_the_md5_cache() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild");
    let env = compute_environment(&ebuild_path, Path::new("/var/tmp/portage")).unwrap();
    assert_eq!(env.inherited.as_deref(), Some("pilotcheck"));

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
    // Portage: _prepare_fake_filesdir. Without the link every
    // ebuild patch via `eapply` dies.
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

    // Re-running must be idempotent (Portage unlinks a stale target).
    let env2 = compute_environment(&pkg_dir.join("patchpkg-1.0.ebuild"), &portage_tmp).unwrap();
    assert_eq!(std::fs::read_link(env2.filesdir()).unwrap(), expected);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn compute_environment_links_builddir_files_even_without_a_repo_filesdir() {
    // A missing repo `files/` yields a dangling symlink, never a
    // plain directory (whose cleanup aborts under `set -e`).
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
    // A stale plain dir left at builddir/files becomes the symlink again.
    let tmp = TempDir::new("ebuild-phases-test-filesdir-stale-dir").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("repo/dev-libs/dirpkg");
    std::fs::create_dir_all(pkg_dir.join("files")).unwrap();
    std::fs::write(pkg_dir.join("dirpkg-1.0.ebuild"), "EAPI=8\nSLOT=0\n").unwrap();
    let portage_tmp = tmp.join("ptmp");
    let builddir = portage_tmp.join("portage/dev-libs/dirpkg-1.0");
    std::fs::create_dir_all(builddir.join("files")).unwrap();
    let env = compute_environment(&pkg_dir.join("dirpkg-1.0.ebuild"), &portage_tmp).unwrap();
    assert_eq!(
        std::fs::read_link(env.filesdir()).unwrap(),
        pkg_dir.join("files")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
