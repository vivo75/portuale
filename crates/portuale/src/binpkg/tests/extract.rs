use super::*;

/// The extractor's tar options as argv: none without `FEATURES=xattr`
/// or without a tar that knows `--xattrs`.
// Portage: BinpkgExtractorAsync._xpak_start
#[test]
fn xpak_xattr_tar_options_match_portages_extractor() {
    let on = crate::ebuild_merge::XattrPolicy {
        enabled: true,
        exclude: "security.evm user.xdg.*".to_string(),
    };
    assert_eq!(
        xpak_xattr_tar_options(&on, true),
        [
            "--xattrs",
            "--xattrs-include=*",
            "--xattrs-exclude=security.evm",
            "--xattrs-exclude=user.xdg.*",
        ]
    );
    assert!(xpak_xattr_tar_options(&on, false).is_empty());
    assert!(xpak_xattr_tar_options(&crate::ebuild_merge::XattrPolicy::default(), true).is_empty());
}

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

/// XPAK members are written verbatim: empty stays empty, trailing
/// spaces survive, `..` is skipped, a leading `/` is stripped, and
/// `sub/x` gets its directory.
// Portage: xpak.py, tbz2.unpackinfo
#[test]
fn extract_binpkg_writes_xpak_members_verbatim_like_portage_unpackinfo() {
    let real = fs::read(fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
    let tmp = TempDir::new("binpkg-xpak-verbatim").keep();
    // The image payload ends where the trailer's `infosize + 8` starts.
    let n = real.len();
    let infosize = u32::from_be_bytes(real[n - 8..n - 4].try_into().unwrap()) as usize;
    let prefix = &real[..n - infosize - 8];
    let bytes = make_xpak_binpkg(
        prefix,
        &[
            ("DEBUGBUILD", b""),
            ("NEEDED", b"/usr/bin/x libc.so.6 \n"),
            ("SLOT", b"0"),
            ("/abs", b"a"),
            ("sub/x", b"s"),
            ("../escape", b"e"),
        ],
    );
    let pkg = tmp.join("pkg.tbz2");
    fs::write(&pkg, bytes).unwrap();
    let image = tmp.join("image");
    let bi = tmp.join("bi");
    extract_binpkg(
        &pkg,
        &image,
        &bi,
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("extract succeeds");
    assert_eq!(fs::read(bi.join("DEBUGBUILD")).unwrap(), b"");
    assert_eq!(
        fs::read(bi.join("NEEDED")).unwrap(),
        b"/usr/bin/x libc.so.6 \n"
    );
    assert_eq!(fs::read(bi.join("SLOT")).unwrap(), b"0");
    assert_eq!(fs::read(bi.join("abs")).unwrap(), b"a");
    assert_eq!(fs::read(bi.join("sub/x")).unwrap(), b"s");
    assert!(
        !tmp.join("escape").exists(),
        "`..` must not escape build-info"
    );
    assert!(image.join("usr/share/packagepkg/hello.txt").is_file());
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

#[test]
fn extract_binpkg_rejects_a_gpkg_with_a_bad_manifest() {
    let img: &[u8] = b"pretend image tar bytes";
    let manifest = format!(
        "{}{}",
        data_line("gpkg-1", b""),
        "DATA image.tar 3 BLAKE2B x SHA512 y\n",
    );
    let g = build_gpkg(
        "foo-1.0",
        &[("gpkg-1", b""), ("image.tar", img)],
        Some(&manifest),
    );
    let tmp = TempDir::new("binpkg-gpkg-bad").keep();
    let err = extract_binpkg(
        &g,
        &tmp.join("image"),
        &tmp.join("build-info"),
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .unwrap_err();
    assert!(err.contains("size mismatch"), "{err}");
    let _ = fs::remove_dir_all(&tmp);
}

// The merge-path metadata extraction.

#[test]
fn extract_binpkg_refuses_an_inner_metadata_symlink_and_writes_nothing() {
    // Portage refuses a link member whatever it points at; the image side
    // is not unpacked either (both halves are validated before any write).
    let g = build_gpkg_with_inner_tars(
        "gen-1.0",
        &[
            inner_entry("metadata/SLOT", b"0\n"),
            inner_symlink("metadata/DESCRIPTION", "SLOT"),
        ],
        &[inner_entry("image/usr/bin/x", b"x\n")],
    );
    let tmp = TempDir::new("binpkg-gpkg-meta").keep();
    let image = tmp.join("image");
    let bi = tmp.join("build-info");
    let err = extract_binpkg(
        &g,
        &image,
        &bi,
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("Metadata member is not a regular file: metadata/DESCRIPTION"),
        "{err}"
    );
    assert!(
        read_dir_sorted(&image).unwrap().is_empty(),
        "image populated"
    );
    assert!(
        read_dir_sorted(&bi).unwrap().is_empty(),
        "build-info populated"
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn extract_binpkg_rejects_unsafe_inner_metadata_archives() {
    let check = |entries: &[InnerTestEntry<'_>], needle: &str| {
        let g = build_gpkg_with_inner_entries("gen-1.0", entries);
        verify_gpkg_manifest(&g, &GpgVerify::default()).expect("verifies");
        let tmp = TempDir::new("binpkg-gpkg-reject").keep();
        let err = extract_binpkg(
            &g,
            &tmp.join("image"),
            &tmp.join("build-info"),
            &GpgVerify::default(),
            &crate::ebuild_merge::XattrPolicy::default(),
        )
        .unwrap_err();
        assert!(err.contains(needle), "expected {needle:?} in {err:?}");
        let _ = fs::remove_dir_all(&tmp);
    };
    // An escaping symlink is rejected: it would land in build-info
    // where later readers follow it.
    check(
        &[
            inner_entry("metadata/SLOT", b"0\n"),
            inner_symlink("metadata/DESCRIPTION", "/etc/hostname"),
        ],
        "Metadata member is not a regular file: metadata/DESCRIPTION",
    );
    // A FIFO is rejected on merge too.
    check(
        &[inner_special("metadata/DESCRIPTION", tar::EntryType::Fifo)],
        "Metadata member is not a regular file",
    );
    // Nested keys.
    check(
        &[
            inner_special("metadata/sub/", tar::EntryType::Directory),
            inner_entry("metadata/sub/KEY", b"nested\n"),
        ],
        "Metadata member is not a regular file: metadata/sub",
    );
    check(&[inner_entry("metadata/sub/KEY", b"nested\n")], "is nested");
    // Duplicates are rejected.
    check(
        &[
            inner_entry("metadata/SLOT", b"0\n"),
            inner_entry("metadata/SLOT", b"1\n"),
        ],
        "duplicate members: SLOT",
    );
}

#[test]
fn extract_binpkg_keeps_extracting_image_symlinks() {
    // A legitimate relative image symlink still lands as a symlink;
    // only devices/dups/abs/traversal/escaping hardlinks are rejected.
    let g = build_gpkg_with_inner_tars(
        "gen-1.0",
        &[inner_entry("metadata/SLOT", b"0\n")],
        &[
            inner_entry("image/usr/lib/libreal.so", b"real\n"),
            inner_symlink("image/usr/lib/liblink.so", "libreal.so"),
        ],
    );
    let tmp = TempDir::new("binpkg-gpkg-imgsym").keep();
    let image = tmp.join("image");
    let bi = tmp.join("build-info");
    extract_binpkg(
        &g,
        &image,
        &bi,
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("extract succeeds");
    let link = image.join("usr/lib/liblink.so");
    let meta = fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_symlink(), "image symlink kept");
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("libreal.so"));
    assert_eq!(
        fs::read(image.join("usr/lib/libreal.so")).unwrap(),
        b"real\n"
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn extract_binpkg_rejects_unsafe_image_tars_before_unpacking() {
    let check = |image: &[InnerTestEntry<'_>], needle: &str| {
        let g =
            build_gpkg_with_inner_tars("gen-1.0", &[inner_entry("metadata/SLOT", b"0\n")], image);
        verify_gpkg_manifest(&g, &GpgVerify::default()).expect("verifies");
        let tmp = TempDir::new("binpkg-gpkg-imgreject").keep();
        let err = extract_binpkg(
            &g,
            &tmp.join("image"),
            &tmp.join("build-info"),
            &GpgVerify::default(),
            &crate::ebuild_merge::XattrPolicy::default(),
        )
        .unwrap_err();
        assert!(err.contains(needle), "expected {needle:?} in {err:?}");
        // The pre-scan runs before the unpack: the image dest stays
        // empty (a FIFO/device member never reaches the filesystem).
        assert!(
            !tmp.join("image").exists() || read_dir_sorted(&tmp.join("image")).unwrap().is_empty()
        );
        let _ = fs::remove_dir_all(&tmp);
    };
    // A char device or FIFO.
    check(
        &[inner_special("image/dev/null0", tar::EntryType::Char)],
        "Danger: device file detected: image/dev/null0",
    );
    check(
        &[inner_special("image/dev/pipe0", tar::EntryType::Fifo)],
        "Danger: device file detected: image/dev/pipe0",
    );
    // A hard link to a non-member or a later member.
    check(
        &[inner_hardlink("image/usr/bin/x", "/etc/hostname")],
        "Danger: hardlink escape detected: image/usr/bin/x",
    );
    check(
        &[
            inner_hardlink("image/usr/bin/x", "image/usr/bin/y"),
            inner_entry("image/usr/bin/y", b"later\n"),
        ],
        "Danger: hardlink escape detected: image/usr/bin/x",
    );
    // Duplicate names.
    check(
        &[
            inner_entry("image/usr/lib/libfoo.so", b"first\n"),
            inner_entry("image/usr/lib/libfoo.so", b"second\n"),
        ],
        "Danger: duplicate files detected: image/usr/lib/libfoo.so",
    );
    // Traversal / absolute member names.
    check(
        &[inner_entry("image/../etc/x", b"traversal\n")],
        "Danger: path traversal detected: image/../etc/x",
    );
    check(
        &[inner_entry("/image/etc/x", b"absolute\n")],
        "Danger: absolute path detected: /image/etc/x",
    );
    // A member under an earlier symlink member.
    check(
        &[
            inner_symlink("image/usr/lib/x", "/etc"),
            inner_entry("image/usr/lib/x/passwd", b"pwned\n"),
        ],
        "Danger: symlink escape detected: image/usr/lib/x",
    );
}

/// One hostile image per oracle shape: the verdict is Portage's, the
/// scan refuses before `tar` runs, so the image directory stays empty.
#[test]
fn extract_binpkg_refuses_the_portage_hostile_image_shapes() {
    let d = tar::EntryType::Directory;
    let base = || {
        vec![
            inner_special("image", d),
            inner_special("image/opt", d),
            inner_entry("image/opt/hello", b"hi\n"),
        ]
    };
    let cases: Vec<(&str, Vec<InnerTestEntry<'_>>, &str)> = vec![
        (
            "a/..",
            vec![inner_special("image/opt/..", d)],
            "path traversal detected: image/opt/..",
        ),
        (
            "bare ..",
            vec![inner_special("..", d)],
            "path traversal detected: ..",
        ),
        (
            "./..",
            vec![inner_entry("./../evil", b"x")],
            "path traversal detected: ./../evil",
        ),
        (
            "dir through symlink",
            vec![
                inner_symlink("image/link", "/var/tmp/outside-x"),
                inner_special("image/./link", d),
            ],
            "symlink escape detected: image/./link",
        ),
        (
            "abs symlink parent",
            vec![
                inner_symlink("image/link", "/var/tmp/outside-x"),
                inner_entry("image/link/marker", b"x"),
            ],
            "symlink escape detected: image/link",
        ),
        (
            "rel symlink parent",
            vec![
                inner_symlink("image/link", "../../x"),
                inner_entry("image/link/marker", b"x"),
            ],
            "symlink escape detected: image/link",
        ),
        (
            "symlink to root",
            vec![
                inner_symlink("image/x", "/"),
                inner_entry("image/x/etc/evil", b"x"),
            ],
            "symlink escape detected: image/x/etc",
        ),
        (
            "symlink through a symlink chain",
            vec![
                inner_symlink("image/a", "b"),
                inner_symlink("image/b", "../../out"),
                inner_entry("image/a/f", b"x"),
            ],
            "symlink escape detected: image/a",
        ),
        (
            "hardlink to an escaping absolute symlink",
            vec![
                inner_symlink("image/link", "/var/tmp/outside-x"),
                inner_hardlink("image/hardlink", "image/link"),
            ],
            "symlink escape detected: image/link",
        ),
        (
            "hardlink relocating a relative symlink",
            vec![
                inner_symlink("image/opt/link", "../../marker"),
                inner_hardlink("image/hard", "image/opt/link"),
            ],
            "symlink escape detected: image/../../marker",
        ),
        (
            "symlink loop",
            vec![
                inner_symlink("image/l", "l"),
                inner_entry("image/l/f", b"x"),
            ],
            "symlink escape detected: image/l",
        ),
    ];
    for (what, extra, needle) in cases {
        let image: Vec<_> = base().into_iter().chain(extra).collect();
        let g =
            build_gpkg_with_inner_tars("gen-1.0", &[inner_entry("metadata/SLOT", b"0\n")], &image);
        let tmp = TempDir::new("binpkg-gpkg-hostile").keep();
        let err = extract_binpkg(
            &g,
            &tmp.join("image"),
            &tmp.join("build-info"),
            &GpgVerify::default(),
            &crate::ebuild_merge::XattrPolicy::default(),
        )
        .unwrap_err();
        assert!(
            err.contains(needle),
            "{what}: expected {needle:?} in {err:?}"
        );
        assert!(
            read_dir_sorted(&tmp.join("image")).unwrap().is_empty(),
            "{what}: image populated"
        );
        assert!(
            read_dir_sorted(&tmp.join("build-info")).unwrap().is_empty(),
            "{what}: build-info populated"
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}

/// Portage's containment root is its temporary directory, one level
/// above the unpacked `image/` tree, so a symlink climbing exactly to
/// that root is accepted there and refused here. Oracle: `tar_safe_extract`
/// over the crafted tars as root: rc 0 on both managers for the bare link (the
/// link lands in the image); `b-link-to-temproot-with-member` is accepted
/// by the scan but GNU tar 1.35 defers `..`-target symlinks and stops
/// after a partial unpack, so portuale reports tar's failure (rc 1) with
/// the image directory left empty — the same GNU-tar boundary as the
/// hardlink shape below.
#[test]
fn extract_binpkg_accepts_a_symlink_climbing_to_the_temp_root_like_portage() {
    let meta = [inner_entry("metadata/SLOT", b"0\n")];
    // The bare link: accepted before and after, target byte-preserved.
    let g = build_gpkg_with_inner_tars(
        "gen-1.0",
        &meta,
        &[
            inner_entry("image/opt/h/hello", b"hi\n"),
            inner_symlink("image/opt/link", "../.."),
        ],
    );
    let tmp = TempDir::new("binpkg-gpkg-temproot").keep();
    let image = tmp.join("image");
    extract_binpkg(
        &g,
        &image,
        &tmp.join("build-info"),
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("a symlink to the temp root is accepted like Portage");
    let link = image.join("opt/link");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the temp-root symlink lands in the image"
    );
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("../.."));
    assert_eq!(fs::read(image.join("opt/h/hello")).unwrap(), b"hi\n");
    let _ = fs::remove_dir_all(&tmp);
    // A member under the link: Portage extracts it into its temp root and
    // discards it with the temp dir; GNU tar stops after a partial unpack.
    let g = build_gpkg_with_inner_tars(
        "gen-1.0",
        &meta,
        &[
            inner_entry("image/opt/h/hello", b"hi\n"),
            inner_symlink("image/opt/link", "../.."),
            inner_entry("image/opt/link/tool", b"t\n"),
        ],
    );
    let tmp = TempDir::new("binpkg-gpkg-temproot-member").keep();
    let image = tmp.join("image");
    let err = extract_binpkg(
        &g,
        &image,
        &tmp.join("build-info"),
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("failed"),
        "the GNU-tar truncation surfaces as a tar failure, got {err:?}"
    );
    assert!(
        read_dir_sorted(&image).unwrap().is_empty(),
        "the partial unpack never reaches the image directory"
    );
    let _ = fs::remove_dir_all(&tmp);
}

/// A hard link of an in-image symlink whose target contains `..`,
/// with a member under it, extracts in Portage (rc 0) but makes
/// GNU tar 1.35 stop after a partial unpack (rc 2): tar defers symlinks
/// whose target contains `..`, so the member under the hard link has no
/// directory to land in. Oracle: `tar_safe_extract` as root (rc 0) vs GNU
/// tar (rc 2, `Cannot open: Not a directory`) over the same tar
/// (oracle-matrix row `c-dotdot-same-depth`). Documented GNU-tar
/// divergence: portuale reports tar's failure (rc 1) with the image
/// directory left empty.
#[test]
fn extract_binpkg_reports_the_gnu_tar_truncation_for_a_dotdot_hardlink_shape() {
    let g = build_gpkg_with_inner_tars(
        "gen-1.0",
        &[inner_entry("metadata/SLOT", b"0\n")],
        &[
            inner_entry("image/marker/f", b"m\n"),
            inner_symlink("image/deep/link", "../marker"),
            inner_hardlink("image/deep/hard", "image/deep/link"),
            inner_entry("image/deep/hard/x", b"x\n"),
        ],
    );
    let tmp = TempDir::new("binpkg-gpkg-hardlink-trunc").keep();
    let image = tmp.join("image");
    let err = extract_binpkg(
        &g,
        &image,
        &tmp.join("build-info"),
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("failed"),
        "the GNU-tar truncation surfaces as a tar failure, got {err:?}"
    );
    assert!(
        read_dir_sorted(&image).unwrap().is_empty(),
        "the partial unpack never reaches the image directory"
    );
    let _ = fs::remove_dir_all(&tmp);
}

/// Upstream's contained-symlink-parent probe, plus the hard link of a
/// symlink that stays inside: Portage accepts both and so does portuale.
/// A hard link whose relocated symlink target stays inside the temp root
/// (`image/opt/deep/link -> ../x` relocated to `image/../x`) is accepted
/// too: Portage `tar_safe_extract` returns rc 0 on the shape (oracle-matrix
/// row `u-relocated-out-of-place`).
#[test]
fn extract_binpkg_accepts_contained_symlink_parents() {
    let g = build_gpkg_with_inner_tars(
        "gen-1.0",
        &[inner_entry("metadata/SLOT", b"0\n")],
        &[
            inner_entry("image/opt/real/seed", b"s\n"),
            inner_symlink("image/opt/bin", "real"),
            inner_entry("image/opt/bin/tool", b"t\n"),
            inner_symlink("image/opt/up", "../opt/real"),
            inner_hardlink("image/opt/hard", "image/opt/up"),
            inner_symlink("image/opt/deep/link", "../x"),
            inner_hardlink("image/opt/hard2", "image/opt/deep/link"),
        ],
    );
    let tmp = TempDir::new("binpkg-gpkg-contained").keep();
    let image = tmp.join("image");
    extract_binpkg(
        &g,
        &image,
        &tmp.join("build-info"),
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("a contained symlink parent is accepted");
    assert_eq!(fs::read(image.join("opt/real/tool")).unwrap(), b"t\n");
    // The hard link of an in-image symlink is itself a symlink (`os.link`).
    assert!(fs::symlink_metadata(image.join("opt/hard")).is_ok());
    assert!(fs::symlink_metadata(image.join("opt/hard2")).is_ok());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn extract_binpkg_keeps_a_valid_sparse_xattr_image_working() {
    // The regression shape: legitimate symlink, hardlink-to-earlier,
    // sparse file and PAX xattr records must still extract (the
    // pre-scan only rejects).
    let tmp = TempDir::new("binpkg-gpkg-i19").keep();
    let stage = tmp.join("stage");
    fs::create_dir_all(&stage).unwrap();
    let image_bytes = build_gnu_sparse_image(&stage);
    let meta = build_inner_tar(&[inner_entry("metadata/SLOT", b"0\n")]);
    let g = build_gpkg_with_tar_bytes("gen-1.0", &meta, &image_bytes);
    verify_gpkg_manifest(&g, &GpgVerify::default()).expect("verifies");
    let image = tmp.join("image");
    let bi = tmp.join("build-info");
    extract_binpkg(
        &g,
        &image,
        &bi,
        &GpgVerify::default(),
        &crate::ebuild_merge::XattrPolicy::default(),
    )
    .expect("extract succeeds");

    use std::os::unix::fs::MetadataExt;
    let link = image.join("usr/lib/liblink.so");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("libreal.so"));
    // The hard link pair keeps one inode.
    let realbin = image.join("usr/bin/realbin");
    let hardlink = image.join("usr/bin/hardlink");
    assert_eq!(
        fs::metadata(&realbin).unwrap().ino(),
        fs::metadata(&hardlink).unwrap().ino(),
        "the hard link target stays hard-linked"
    );
    assert_eq!(fs::read(&realbin).unwrap(), b"#!/bin/sh\nexit 0\n");
    // The sparse file stays sparse (1 MiB, a handful of blocks).
    let sparse = fs::metadata(image.join("usr/share/sparse.bin")).unwrap();
    assert_eq!(sparse.len(), 1024 * 1024 + 4);
    assert!(
        sparse.blocks() * 512 < sparse.len() / 2,
        "sparseness lost: {} blocks for {} bytes",
        sparse.blocks(),
        sparse.len()
    );
    assert_eq!(
        fs::read(image.join("usr/share/xattr.txt")).unwrap(),
        b"xattr carrier\n"
    );
    // GNU `tar -xpf` (no `--xattrs`) ignores the PAX xattr records.
    let xpath = std::ffi::CString::new(
        image
            .join("usr/share/xattr.txt")
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();
    // SAFETY: `listxattr` takes a valid NUL-terminated path and a
    // size-0/null buffer returns the needed size.
    let xattr_size = unsafe { libc::listxattr(xpath.as_ptr(), std::ptr::null_mut(), 0) };
    assert_eq!(xattr_size, 0, "extraction must not invent xattrs");
    let _ = fs::remove_dir_all(&tmp);
}
