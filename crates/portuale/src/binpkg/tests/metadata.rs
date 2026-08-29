use super::*;

#[test]
fn read_gpkg_metadata_extracts_the_embedded_scalar_metadata() {
    // A hand-built `.gpkg.tar`: outer plain-tar container, `gpkg-1`
    // marker, compressed inner `metadata.tar` with `metadata/<KEY>` files.
    let m = read_gpkg_metadata(&fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
        .expect("the fixture gpkg reads");
    assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
    assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
    assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
    assert_eq!(m.get("IUSE").map(String::as_str), Some("grfoo"));
    assert_eq!(m.get("USE").map(String::as_str), Some(""));
    assert_eq!(m.get("DEPEND").map(String::as_str), Some("dev-libs/newpkg"));
    assert_eq!(
        m.get("RDEPEND").map(String::as_str),
        Some("dev-libs/newpkg")
    );
    assert_eq!(m.get("CATEGORY").map(String::as_str), Some("dev-libs"));
    assert_eq!(m.get("PF").map(String::as_str), Some("gpkgreadpkg-1.0"));
    assert_eq!(m.get("repository").map(String::as_str), Some("gentoo"));
}

#[test]
fn read_gpkg_metadata_reads_the_regenerated_fixture_without_a_directory_member() {
    // The committed fixture has no inner `metadata/` directory member.
    let m = read_gpkg_metadata(&fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
        .expect("the regenerated fixture reads");
    assert_eq!(m.len(), 13);
    assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
}
