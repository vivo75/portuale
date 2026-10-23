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

/// The two-class error split the scan caller makes. The inner-member
/// cells' classes are asserted in their own tests; this pins the
/// outer-container and xpak rows.
// Portage: bintree.py, _populate_local
#[test]
fn binpkg_error_classes_mirror_portages_caller() {
    // A trusted outer member that is not a regular file.
    let g = build_gpkg_entries(&[
        ("pre/gpkg-1", GpkgEntry::File(b"")),
        ("pre/metadata.tar", GpkgEntry::Fifo),
    ]);
    let err = read_gpkg_metadata(&g).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("not a regular file"), "{err}");

    // A container with no `metadata.tar` member.
    let g = build_gpkg_entries(&[
        ("pre/gpkg-1", GpkgEntry::File(b"")),
        ("pre/image.tar", GpkgEntry::File(b"x")),
    ]);
    let err = read_gpkg_metadata(&g).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("no `metadata.tar` member"), "{err}");

    // A file that is not a tar at all: the skip class too.
    let scratch = ScratchDir::new("gpkg-notar").unwrap();
    let junk = scratch.path().join("junk.gpkg.tar");
    fs::write(&junk, vec![0xffu8; 2048]).unwrap();
    let err = read_gpkg_metadata(&junk).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");

    // An xpak trailer pointing at a segment that is not an xpak.
    let scratch = ScratchDir::new("xpak-badpack").unwrap();
    let path = scratch.path().join("bad.tbz2");
    let mut bytes = vec![0u8; 16];
    bytes.extend_from_slice(b"XPAKSTOP");
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(b"STOP");
    fs::write(&path, &bytes).unwrap();
    let err = read_xpak_metadata(&path).unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("XPAKPACK"), "{err}");
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
fn read_gpkg_metadata_refuses_inner_links_and_special_members() {
    // Portage: `gpkg._check_metadata_files`; the skip class, whatever the
    // link points at, and no host file's bytes leak into the message.
    let host = std::fs::read("/etc/hostname")
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
        .unwrap_or_default();
    for bad in [
        inner_symlink("metadata/DESCRIPTION", "SLOT"),
        inner_symlink("metadata/DESCRIPTION", "/etc/hostname"),
        inner_hardlink("metadata/DESCRIPTION", "metadata/SLOT"),
        inner_special("metadata/DESCRIPTION", tar::EntryType::Fifo),
    ] {
        let g =
            build_gpkg_with_inner_entries("gen-1.0", &[inner_entry("metadata/SLOT", b"0\n"), bad]);
        verify_gpkg_manifest(&g, &GpgVerify::default()).expect("the crafted container verifies");
        let err = read_gpkg_metadata(&g).unwrap_err();
        assert!(err.is_invalid(), "{err:?}");
        assert!(
            err.message()
                .contains("Metadata member is not a regular file: metadata/DESCRIPTION"),
            "{err}"
        );
        assert!(
            host.is_empty() || !err.message().contains(&host),
            "the host file's bytes leaked into {err:?}"
        );
    }
}

#[test]
fn read_gpkg_metadata_rejects_outside_names() {
    // An outside name: the old walk ignored the member entirely.
    let g = build_gpkg_with_inner_entries("gen-1.0", &[inner_entry("other/KEY", b"outside\n")]);
    verify_gpkg_manifest(&g, &GpgVerify::default()).expect("verifies");
    let err = read_gpkg_metadata(&g).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("outside metadata/"), "{err}");
}

#[test]
fn read_gpkg_metadata_reads_nested_keys_and_last_wins_duplicates() {
    // Nested keys read; last duplicate wins (the merge path refuses it).
    let g = build_gpkg_with_inner_entries(
        "gen-1.0",
        &[
            inner_entry("metadata/sub/KEY", b"nested\n"),
            inner_entry("metadata/SLOT", b"0\n"),
            inner_entry("metadata/SLOT", b"1\n"),
        ],
    );
    verify_gpkg_manifest(&g, &GpgVerify::default()).expect("verifies");
    let metadata = read_gpkg_metadata(&g).expect("reads");
    assert_eq!(metadata.get("sub/KEY").map(String::as_str), Some("nested"));
    assert_eq!(metadata.get("SLOT").map(String::as_str), Some("1"));
}

#[test]
fn read_gpkg_metadata_reads_the_regenerated_fixture_without_a_directory_member() {
    // The committed fixture has no inner `metadata/` directory member.
    let m = read_gpkg_metadata(&fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
        .expect("the regenerated fixture reads");
    assert_eq!(m.len(), 13);
    assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
}
