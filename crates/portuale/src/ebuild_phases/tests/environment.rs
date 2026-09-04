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
