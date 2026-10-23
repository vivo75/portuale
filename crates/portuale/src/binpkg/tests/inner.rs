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

/// Portage refuses every metadata member that is not a regular file,
/// whatever a link points at (`gpkg._check_metadata_files`); the class is
/// `Invalid` (`InvalidBinaryPackageFormat`), the skip class.
#[test]
fn read_inner_metadata_refuses_every_non_regular_member() {
    let cases: [(&str, InnerTestEntry<'_>); 8] = [
        ("symlink", inner_symlink("metadata/DESCRIPTION", "SLOT")),
        (
            "escaping symlink",
            inner_symlink("metadata/DESCRIPTION", "/etc/hostname"),
        ),
        (
            "symlink loop",
            inner_symlink("metadata/DESCRIPTION", "DESCRIPTION"),
        ),
        (
            "hardlink",
            inner_hardlink("metadata/DESCRIPTION", "metadata/SLOT"),
        ),
        (
            "missing hardlink",
            inner_hardlink("metadata/DESCRIPTION", "etc/hostname"),
        ),
        (
            "fifo",
            inner_special("metadata/DESCRIPTION", tar::EntryType::Fifo),
        ),
        (
            "character device",
            inner_special("metadata/DESCRIPTION", tar::EntryType::Char),
        ),
        (
            "block device",
            inner_special("metadata/DESCRIPTION", tar::EntryType::Block),
        ),
    ];
    for (what, bad) in cases {
        // A regular `SLOT` first: the link target exists, and is still refused.
        let err = read_inner(&[inner_entry("metadata/SLOT", b"0\n"), bad]).unwrap_err();
        assert!(err.is_invalid(), "{what}: {err:?}");
        assert!(
            err.message()
                .ends_with("Metadata member is not a regular file: metadata/DESCRIPTION"),
            "{what}: {err}"
        );
    }
}

#[test]
fn read_inner_metadata_refuses_directory_members() {
    // The `metadata/` directory member and a nested one: both are
    // directories, not regular files (`tarfile` strips the trailing `/`).
    for (dir, shown) in [("metadata/", "metadata"), ("metadata/sub/", "metadata/sub")] {
        let err = read_inner(&[
            inner_special(dir, tar::EntryType::Directory),
            inner_entry("metadata/SLOT", b"0\n"),
        ])
        .unwrap_err();
        assert!(err.is_invalid(), "{dir}: {err:?}");
        assert!(
            err.message()
                .ends_with(&format!("Metadata member is not a regular file: {shown}")),
            "{dir}: {err}"
        );
    }
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
fn read_inner_metadata_refuses_unknown_entry_types() {
    // `TarInfo.isreg` is false for a type byte `tarfile` does not know.
    // Portage: gpkg.py, `_check_metadata_files`
    let err = read_inner(&[inner_special("metadata/ODD", tar::EntryType::new(b'X'))]).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(
        err.message().contains("not a regular file: metadata/ODD"),
        "{err}"
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
