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

/// Ports Portage's `_prepare_fake_distdir`: entries that are not a
/// symlink named in `A` are removed and wrong-target symlinks recreated.
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
    use std::os::unix::fs::MetadataExt;
    let after = std::fs::symlink_metadata(fake.join("keep")).unwrap();
    assert_eq!(before.ino(), after.ino());

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// `PORTAGE_BUILDDIR` derives from the given tmpdir, so a matched
/// per-package tmpdir moves the whole build tree under it.
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

/// The vendored `__filter_readonly_variables` drops `LOCPATH` with the
/// other locale variables under `--filter-locale`, keeps an ebuild-set
/// `QEMU_*` variable, and the vendored `phase-functions.sh` keeps both of
/// portuale's local changes.
/// Expected values from reading Portage's `phase-functions.sh` and a live
/// `env -i` run of the same function.
#[test]
fn vendored_filter_readonly_variables_matches_portage_3_0_83() {
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let src = std::fs::read_to_string(bin_dir.join("phase-functions.sh")).unwrap();
    assert!(src.contains("___save_and_filter_ebuild_env()"));
    assert!(src.contains("filtered_vars+=( \"PORTUALE_.*\" )"));
    assert!(src.contains("LC_ALL LANG LOCPATH"));
    let out = std::process::Command::new("bash")
        .env_clear()
        .env("PORTAGE_BIN_PATH", &bin_dir)
        .arg("-c")
        .arg(
            "source \"$PORTAGE_BIN_PATH/isolated-functions.sh\"\n\
             source \"$PORTAGE_BIN_PATH/phase-functions.sh\"\n\
             __filter_readonly_variables --filter-locale < <(\n\
             declare -x LOCPATH=/x LANG=C QEMU_LD_PREFIX=/q FOO=1\n\
             declare -p LOCPATH LANG QEMU_LD_PREFIX FOO)",
        )
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("declare -x FOO=\"1\""), "{text}");
    assert!(text.contains("QEMU_LD_PREFIX"), "{text}");
    for gone in ["LOCPATH", "LANG"] {
        assert!(!text.contains(gone), "{gone} must be filtered:\n{text}");
    }
}
