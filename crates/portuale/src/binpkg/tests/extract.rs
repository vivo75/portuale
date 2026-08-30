use super::*;

#[test]
fn extract_binpkg_unpacks_an_xpak_image_and_build_info() {
    let tmp = TempDir::new("binpkg-xpak").keep();
    let image = tmp.join("image");
    let bi = tmp.join("build-info");
    extract_binpkg(
        &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
        &image,
        &bi,
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("extract succeeds");

    let hello = image.join("usr/share/packagepkg/hello.txt");
    assert!(hello.is_file(), "the image tarball was unpacked");
    assert!(fs::read_to_string(&hello).unwrap().contains("hello"));

    assert_eq!(fs::read_to_string(bi.join("SLOT")).unwrap().trim(), "0");
    assert_eq!(
        fs::read_to_string(bi.join("RDEPEND")).unwrap().trim(),
        "dev-libs/samepkg"
    );
    // The two non-scalar members are kept verbatim: a bzip2 stream
    // (magic `BZh`) and the ebuild source.
    let env_bz2 = fs::read(bi.join("environment.bz2")).expect("environment.bz2 kept");
    assert_eq!(&env_bz2[..3], b"BZh", "a real bzip2 stream, byte-exact");
    assert!(
        fs::read_to_string(bi.join("packagepkg-1.0.ebuild"))
            .unwrap()
            .contains("EAPI=8")
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn extract_binpkg_unpacks_a_real_gpkg_image_and_build_info() {
    let tmp = TempDir::new("binpkg-gpkg").keep();
    let image = tmp.join("image");
    let bi = tmp.join("build-info");
    extract_binpkg(
        &fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
        &image,
        &bi,
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("gpkg extract succeeds");
    // The inner `image/` and `metadata/` top-level dirs are stripped:
    // members land directly, not one level deep.
    assert!(image.join("hello.txt").is_file(), "image prefix stripped");
    assert!(!image.join("image").exists(), "no leftover image/ dir");
    assert!(bi.join("SLOT").is_file(), "metadata prefix stripped");
    assert!(!bi.join("metadata").exists(), "no leftover metadata/ dir");
    // The metadata side writes resolved member bytes verbatim as
    // regular files with the header mode.
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(fs::read(bi.join("SLOT")).unwrap(), b"0\n");
    assert_eq!(fs::read(bi.join("EAPI")).unwrap(), b"8\n");
    assert_eq!(fs::read(bi.join("RDEPEND")).unwrap(), b"dev-libs/newpkg\n");
    let slot = fs::symlink_metadata(bi.join("SLOT")).unwrap();
    assert!(slot.file_type().is_file(), "a regular file, never a link");
    assert_eq!(slot.permissions().mode() & 0o7777, 0o644);
    assert_eq!(bi.join("DESCRIPTION").metadata().unwrap().len(), 78);
    let _ = fs::remove_dir_all(&tmp);
}
