use super::*;

#[test]
fn read_inner_metadata_decompresses_through_the_pipe() {
    use std::io::Write;
    let tar = build_inner_tar(&[
        inner_entry("metadata/SLOT", b"0\n"),
        inner_entry("metadata/RDEPEND", b"dev-libs/newpkg\n"),
    ]);
    let scratch = ScratchDir::new("inner-zstd-test").unwrap();
    let path = scratch.path().join("metadata.tar.zst");
    let mut child = Command::new("zstd")
        .args(["-q", "-c"])
        .stdin(Stdio::piped())
        .stdout(fs::File::create(&path).unwrap())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&tar).unwrap();
    assert!(child.wait().unwrap().success());

    let compressed = fs::read(&path).unwrap();
    let metadata = read_inner_metadata(
        Path::new("/test/pkg.gpkg.tar"),
        "metadata.tar.zst",
        &compressed,
        Some(&["zstd", "-dc", "--long=31"]),
    )
    .expect("the zstd pipe reads");
    let by_key = inner_values(&metadata);
    assert_eq!(by_key["SLOT"], b"0\n");
    assert_eq!(by_key["RDEPEND"], b"dev-libs/newpkg\n");

    // A failing decompressor is still an error.
    let broken = scratch.path().join("broken.tar.zst");
    fs::write(&broken, b"this is not a zstd stream").unwrap();
    let err = read_inner_metadata(
        Path::new("/test/pkg.gpkg.tar"),
        "metadata.tar.zst",
        &fs::read(&broken).unwrap(),
        Some(&["zstd", "-dc", "--long=31"]),
    )
    .unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("zstd"), "{err}");
}

#[test]
fn read_inner_metadata_round_trips_regular_members() {
    let metadata = read_inner(&[
        inner_entry("metadata/SLOT", b"0\n"),
        inner_entry("metadata/EAPI", b"8\n"),
    ])
    .expect("reads");
    assert_eq!(
        inner_pairs(&metadata),
        vec![
            ("SLOT".to_string(), b"0\n".to_vec()),
            ("EAPI".to_string(), b"8\n".to_vec()),
        ]
    );
    assert!(metadata.duplicate_keys.is_empty());
}

#[test]
fn read_inner_metadata_resolves_symlinks_in_archive() {
    // Covers a `../`-normalized target and a link chain: links join
    // `dirname(name)/linkname`, normalize, and recurse.
    let metadata = read_inner(&[
        inner_entry("metadata/SLOT", b"0\n"),
        inner_symlink("metadata/DESCRIPTION", "SLOT"),
        inner_symlink("metadata/A", "B"),
        inner_symlink("metadata/B", "SLOT"),
        inner_symlink("metadata/sub/L", "../SLOT"),
    ])
    .expect("reads");
    let by_key = inner_values(&metadata);
    assert_eq!(by_key["DESCRIPTION"], b"0\n");
    assert_eq!(by_key["A"], b"0\n");
    assert_eq!(by_key["sub/L"], b"0\n");
}

#[test]
fn read_inner_metadata_rejects_a_symlink_out_of_the_archive() {
    // A link out of the archive errors; the host file is never read.
    let err = read_inner(&[
        inner_entry("metadata/SLOT", b"0\n"),
        inner_symlink("metadata/DESCRIPTION", "/etc/hostname"),
    ])
    .unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("not in the archive"), "{err}");
    assert!(err.message().contains("DESCRIPTION"), "{err}");
}

#[test]
fn read_inner_metadata_resolves_hardlinks_to_earlier_members() {
    // The target has to be *earlier* than the link.
    let metadata = read_inner(&[
        inner_entry("metadata/SLOT", b"0\n"),
        inner_hardlink("metadata/DESCRIPTION", "metadata/SLOT"),
    ])
    .expect("reads");
    assert_eq!(
        inner_pairs(&metadata),
        vec![
            ("SLOT".to_string(), b"0\n".to_vec()),
            ("DESCRIPTION".to_string(), b"0\n".to_vec()),
        ]
    );
}

#[test]
fn read_inner_metadata_rejects_hardlinks_to_later_or_missing_members() {
    // Target comes later.
    let err = read_inner(&[
        inner_hardlink("metadata/AAA", "metadata/SLOT"),
        inner_entry("metadata/SLOT", b"0\n"),
    ])
    .unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("not in the archive"), "{err}");
    // Target is not a member at all.
    let err = read_inner(&[inner_hardlink("metadata/DESCRIPTION", "etc/hostname")]).unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("not in the archive"), "{err}");
}

#[test]
fn read_inner_metadata_rejects_fifo_and_device_members() {
    // FIFO/device members error, naming the type.
    let err =
        read_inner(&[inner_special("metadata/DESCRIPTION", tar::EntryType::Fifo)]).unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("a FIFO"), "{err}");
    let err =
        read_inner(&[inner_special("metadata/DESCRIPTION", tar::EntryType::Char)]).unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("a character device"), "{err}");
    let err =
        read_inner(&[inner_special("metadata/DESCRIPTION", tar::EntryType::Block)]).unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("a block device"), "{err}");
}

#[test]
fn read_inner_metadata_rejects_directory_members_and_outside_names() {
    // The inner `metadata/` directory member is rejected as outside
    // `metadata/`: `tarfile` strips the trailing slash, leaving the
    // bare `metadata` name.
    let err = read_inner(&[
        inner_special("metadata/", tar::EntryType::Directory),
        inner_entry("metadata/SLOT", b"0\n"),
    ])
    .unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("outside metadata/"), "{err}");
    // A nested directory member errors; the `sub/KEY` member after it
    // never resolves.
    let err = read_inner(&[
        inner_special("metadata/sub/", tar::EntryType::Directory),
        inner_entry("metadata/sub/KEY", b"nested\n"),
    ])
    .unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("a directory"), "{err}");
    // A name outside `metadata/`.
    let err = read_inner(&[inner_entry("other/KEY", b"outside\n")]).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("outside metadata/"), "{err}");
}

#[test]
fn read_inner_metadata_reads_nested_keys_without_a_directory() {
    // The key is the remainder after `metadata/`.
    let metadata = read_inner(&[inner_entry("metadata/sub/KEY", b"nested\n")]).expect("reads");
    assert_eq!(
        inner_pairs(&metadata),
        vec![("sub/KEY".to_string(), b"nested\n".to_vec())]
    );
}

#[test]
fn read_inner_metadata_errors_on_symlink_cycles() {
    // A symlink cycle stops at the depth cap with a named error.
    let err = read_inner(&[
        inner_symlink("metadata/A", "B"),
        inner_symlink("metadata/B", "A"),
    ])
    .unwrap_err();
    assert!(err.is_fatal(), "{err:?}");
    assert!(err.message().contains("link chain is deeper than"), "{err}");
}

#[test]
fn read_inner_metadata_last_wins_and_flags_duplicate_keys() {
    // Last member with a name wins; the duplicate is flagged.
    let metadata = read_inner(&[
        inner_entry("metadata/SLOT", b"0\n"),
        inner_entry("metadata/SLOT", b"1\n"),
    ])
    .expect("reads");
    assert_eq!(
        inner_pairs(&metadata),
        vec![("SLOT".to_string(), b"1\n".to_vec())]
    );
    assert_eq!(metadata.duplicate_keys, vec!["SLOT".to_string()]);
}

#[test]
fn read_inner_metadata_rejects_traversal_and_absolute_names() {
    // Both traversal and absolute names are refused everywhere.
    for name in ["metadata/../KEY", "/metadata/KEY"] {
        let err = read_inner(&[inner_entry(name, b"x\n")]).unwrap_err();
        assert!(err.is_invalid(), "{name}: {err:?}");
        assert!(err.message().contains("unsafe name"), "{name}: {err}");
    }
}

#[test]
fn read_inner_metadata_reads_unknown_entry_types_as_regular_files() {
    // Unknown entry types read as regular files.
    // Portage: gpkg.py, SUPPORTED_TYPES
    let metadata =
        read_inner(&[inner_special("metadata/ODD", tar::EntryType::new(b'X'))]).expect("reads");
    assert_eq!(
        inner_pairs(&metadata),
        vec![("ODD".to_string(), Vec::new())]
    );
}

#[test]
fn classify_inner_member_matches_portage_extract_filename_compression() {
    assert!(matches!(
        classify_inner_member("metadata", "metadata.tar"),
        Some(None)
    ));
    assert!(matches!(
        classify_inner_member("metadata", "metadata.tar.zst"),
        Some(Some(_))
    ));
    assert!(matches!(
        classify_inner_member("metadata", "metadata.tar.xz"),
        Some(Some(_))
    ));
    assert!(classify_inner_member("metadata", "metadata.tar.zst.sig").is_none());
    assert!(classify_inner_member("metadata", "image.tar.zst").is_none());
    assert!(classify_inner_member("metadata", "Manifest").is_none());
}
