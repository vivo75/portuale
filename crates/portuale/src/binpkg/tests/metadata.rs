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
fn read_xpak_metadata_walks_the_index_and_returns_every_key() {
    let scratch = ScratchDir::new("xpak-test").unwrap();
    let path = scratch.path().join("dev-libs:foo-1.0.tbz2");
    let bytes = make_xpak_binpkg(
        b"pretend this is a bzip2'd tarball, arbitrary length ......",
        &[
            ("EAPI", b"8\n"),
            ("SLOT", b"0\n"),
            ("KEYWORDS", b"amd64\n"),
            ("IUSE", b"xfoo xbar\n"),
            ("USE", b"\n"),
            ("RDEPEND", b"dev-libs/samepkg dev-libs/newpkg\n"),
            ("repository", b"gentoo\n"),
        ],
    );
    fs::write(&path, &bytes).unwrap();

    let m = read_xpak_metadata(&path).expect("reads");
    assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
    assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
    assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
    assert_eq!(m.get("IUSE").map(String::as_str), Some("xfoo xbar"));
    assert_eq!(m.get("USE").map(String::as_str), Some(""));
    assert_eq!(
        m.get("RDEPEND").map(String::as_str),
        Some("dev-libs/samepkg dev-libs/newpkg")
    );
    assert_eq!(m.get("repository").map(String::as_str), Some("gentoo"));
}

#[test]
fn read_xpak_metadata_rejects_a_file_with_no_xpak_trailer() {
    let scratch = ScratchDir::new("xpak-negtest").unwrap();
    let path = scratch.path().join("not-a-binpkg");
    fs::write(
        &path,
        b"just some bytes, definitely no XPAKSTOP here at all",
    )
    .unwrap();
    let err = read_xpak_metadata(&path).unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("XPAKSTOP"), "{err}");
}

/// Reads a genuine `.tbz2` built once by portuale's own
/// `ebuild <file> package`. Kept as a committed fixture rather than
/// rebuilt per-test: the read side doesn't need reproducible bytes.
#[test]
fn read_xpak_metadata_reads_a_real_ebuild_package_tbz2() {
    let tbz2 = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
    let m = read_xpak_metadata(&tbz2).expect("the real .tbz2 reads");
    assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
    assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
    assert_eq!(m.get("CATEGORY").map(String::as_str), Some("dev-libs"));
    assert_eq!(m.get("PF").map(String::as_str), Some("packagepkg-1.0"));
    assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
    // The fixture ebuild's `RDEPEND` came through via `build-info`,
    // no `Packages` index involved.
    assert_eq!(
        m.get("RDEPEND").map(String::as_str),
        Some("dev-libs/samepkg")
    );
    // The bundled `<pf>.ebuild` source is a member too.
    assert!(
        m.get("packagepkg-1.0.ebuild")
            .is_some_and(|e| e.contains("EAPI=8"))
    );
    // An xpak never carries CONTENTS (generated at merge time).
    assert!(!m.contains_key("CONTENTS"));
    // `environment.bz2` is binary -> lossy-decoded but present as a
    // key; a scan consumer only ever looks up scalar keys.
    assert!(m.contains_key("environment.bz2"));
}

#[test]
fn read_gpkg_metadata_reads_the_regenerated_fixture_without_a_directory_member() {
    // The committed fixture has no inner `metadata/` directory member.
    let m = read_gpkg_metadata(&fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
        .expect("the regenerated fixture reads");
    assert_eq!(m.len(), 13);
    assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
}
