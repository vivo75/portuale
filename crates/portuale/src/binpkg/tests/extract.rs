use super::*;

/// #334: real `BinpkgExtractorAsync._xpak_start`'s tar options, as the
/// argv its shell string becomes: none without `FEATURES=xattr` or
/// without a tar that knows `--xattrs`.
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
    // The two non-scalar members are kept verbatim: a real bzip2
    // stream (magic `BZh`) and the real ebuild source.
    let env_bz2 = fs::read(bi.join("environment.bz2")).expect("environment.bz2 kept");
    assert_eq!(&env_bz2[..3], b"BZh", "a real bzip2 stream, byte-exact");
    assert!(
        fs::read_to_string(bi.join("packagepkg-1.0.ebuild"))
            .unwrap()
            .contains("EAPI=8")
    );
    let _ = fs::remove_dir_all(&tmp);
}

/// #326 S5 (L2 xpak control): real `tbz2.unpackinfo` writes every
/// XPAK member verbatim. An empty `DEBUGBUILD` stays empty, and the
/// trailing space real's `NEEDED` keeps survives. A name that climbs
/// out with `..` is skipped, a leading `/` is stripped, and a
/// `sub/x` member gets its directory (`xpak.py:486-524`).
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
    // The inner `image/` and `metadata/` top-level dirs real's
    // `tar_safe_extract` strips are stripped here too -- members land
    // directly, not one level deep. A missing strip is what made the
    // merged vdb `CONTENTS` record every path as `/image/...`.
    assert!(image.join("hello.txt").is_file(), "image prefix stripped");
    assert!(!image.join("image").exists(), "no leftover image/ dir");
    assert!(bi.join("SLOT").is_file(), "metadata prefix stripped");
    assert!(!bi.join("metadata").exists(), "no leftover metadata/ dir");
    // `#58` S3: the metadata side now writes the resolved member
    // bytes verbatim as regular files with the header mode (0o644 in
    // the fixture), the same bytes and modes the former `tar -xpf`
    // left (captured on `main`).
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

// ---- #58 S3: the merge-path metadata extraction ----

#[test]
fn extract_binpkg_writes_an_inner_metadata_symlink_as_a_regular_file() {
    // S0 i2: real's unpack extracts the symlink as a symlink (its
    // own writer never emits one); portuale writes the *resolved
    // target bytes* as a regular file, with the member's header
    // mode. `pretend.rs::run_info` calls extract_binpkg with no
    // prior read, so this path is what protects it.
    let g = build_gpkg_with_inner_entries(
        "gen-1.0",
        &[
            inner_entry("metadata/SLOT", b"0\n"),
            inner_symlink("metadata/DESCRIPTION", "SLOT"),
        ],
    );
    let tmp = TempDir::new("binpkg-gpkg-meta").keep();
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
    use std::os::unix::fs::PermissionsExt;
    let desc = fs::symlink_metadata(bi.join("DESCRIPTION")).unwrap();
    assert!(
        desc.file_type().is_file(),
        "the resolved bytes are a regular file, not a symlink"
    );
    assert_eq!(fs::read(bi.join("DESCRIPTION")).unwrap(), b"0\n");
    assert_eq!(desc.permissions().mode() & 0o7777, 0o644);
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
    // S0 i1: an escaping symlink (the old `tar -xpf` extracted the
    // symlink itself into build-info, where later readers follow it).
    check(
        &[
            inner_entry("metadata/SLOT", b"0\n"),
            inner_symlink("metadata/DESCRIPTION", "/etc/hostname"),
        ],
        "not in the archive",
    );
    // S0 i5: real's 3.14 `isdev()` rejects a FIFO on merge too; the
    // old `tar -xpf` created one in build-info.
    check(
        &[inner_special("metadata/DESCRIPTION", tar::EntryType::Fifo)],
        "a FIFO",
    );
    // S0 i7/i7b: nested keys (K4).
    check(
        &[
            inner_special("metadata/sub/", tar::EntryType::Directory),
            inner_entry("metadata/sub/KEY", b"nested\n"),
        ],
        "a directory",
    );
    check(&[inner_entry("metadata/sub/KEY", b"nested\n")], "is nested");
    // S0 i11: duplicates (real `tar_safe_extract`).
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
    // K2 does not touch the image until S4's pre-scan: a legitimate
    // relative symlink must still land as a symlink (real extracts
    // image symlinks as-is; only devices/dup/abs/traversal/escaping
    // hardlinks are rejected).
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
    // S0 i13/i13b: a char device (real `isdev()`; 3.14 includes FIFO).
    check(
        &[inner_special("image/dev/null0", tar::EntryType::Char)],
        "a character device",
    );
    check(
        &[inner_special("image/dev/pipe0", tar::EntryType::Fifo)],
        "a FIFO",
    );
    // S0 i14/i15: a hard link to a non-member or a later member.
    check(
        &[inner_hardlink("image/usr/bin/x", "/etc/hostname")],
        "not an earlier member",
    );
    check(
        &[
            inner_hardlink("image/usr/bin/x", "image/usr/bin/y"),
            inner_entry("image/usr/bin/y", b"later\n"),
        ],
        "not an earlier member",
    );
    // S0 i16: duplicate names (GNU tar used to last-wins silently).
    check(
        &[
            inner_entry("image/usr/lib/libfoo.so", b"first\n"),
            inner_entry("image/usr/lib/libfoo.so", b"second\n"),
        ],
        "is a duplicate",
    );
    // S0 i17/i17b: traversal / absolute member names.
    check(
        &[inner_entry("image/../etc/x", b"traversal\n")],
        "has a path traversal",
    );
    check(
        &[inner_entry("/image/etc/x", b"absolute\n")],
        "has an absolute name",
    );
    // S0 i18: a member under an earlier symlink member (the
    // owner-approved rule beyond real's own set).
    check(
        &[
            inner_symlink("image/usr/lib/x", "/etc"),
            inner_entry("image/usr/lib/x/passwd", b"pwned\n"),
        ],
        "inside the symlink member",
    );
}

#[test]
fn extract_binpkg_keeps_a_valid_sparse_xattr_image_working() {
    // S0 cell i19: the regression shape -- legitimate symlink,
    // hardlink-to-earlier, GNU sparse file, PAX xattr records -- must
    // still extract exactly as before S4 (the pre-scan only rejects).
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
    // GNU `tar -xpf` (no `--xattrs`) ignores the PAX xattr records,
    // exactly as before the pre-scan and as real's own `decompress`
    // (`TEST/findings/l2.md` "## #58 S0" cell i19).
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
