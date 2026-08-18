use super::*;

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
