use super::*;

#[test]
fn verify_gpkg_manifest_accepts_the_real_fixture() {
    verify_gpkg_manifest(
        &fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
        &GpgVerify::default(),
    )
    .expect("the committed fixture's Manifest verifies");
}

#[test]
fn verify_gpkg_manifest_rejects_a_missing_manifest() {
    let g = build_gpkg("foo-1.0", &[("gpkg-1", b""), ("image.tar", b"img")], None);
    let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
    assert!(err.contains("Manifest not found"), "{err}");
}

#[test]
fn verify_gpkg_manifest_rejects_a_size_mismatch_without_hashing() {
    let img: &[u8] = b"the real image bytes";
    let manifest = format!(
        "{}{}",
        data_line("gpkg-1", b""),
        // Deliberately wrong size: caught before any hashing.
        "DATA image.tar 999999 BLAKE2B dead SHA512 beef\n",
    );
    let g = build_gpkg(
        "foo-1.0",
        &[("gpkg-1", b""), ("image.tar", img)],
        Some(&manifest),
    );
    let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
    assert!(err.contains("size mismatch"), "{err}");
}

#[test]
fn verify_gpkg_manifest_rejects_a_tampered_member() {
    let img: &[u8] = b"the real image bytes";
    // Manifest records the digests of different bytes.
    let manifest = format!(
        "{}{}",
        data_line("gpkg-1", b""),
        data_line("image.tar", b"some other bytes entirely"),
    );
    let g = build_gpkg(
        "foo-1.0",
        &[("gpkg-1", b""), ("image.tar", img)],
        Some(&manifest),
    );
    let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
}

#[test]
fn verify_gpkg_manifest_rejects_an_unlisted_member() {
    let manifest = data_line("gpkg-1", b"");
    let g = build_gpkg(
        "foo-1.0",
        &[("gpkg-1", b""), ("image.tar", b"img")],
        Some(&manifest),
    );
    let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
    assert!(err.contains("not listed in the Manifest"), "{err}");
}

#[test]
fn verify_gpkg_manifest_rejects_a_manifest_only_file() {
    let manifest = format!(
        "{}{}",
        data_line("gpkg-1", b""),
        data_line("image.tar", b"img"),
    );
    let g = build_gpkg("foo-1.0", &[("gpkg-1", b"")], Some(&manifest));
    let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
    assert!(err.contains("not present in the container"), "{err}");
}
