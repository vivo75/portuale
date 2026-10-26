use super::*;

#[test]
fn read_gpkg_metadata_rejects_a_non_gpkg_tar() {
    // A flat top-level tar fails the one-level structure check before
    // any marker lookup.
    let scratch = ScratchDir::new("gpkg-negtest").unwrap();
    let junk = scratch.path().join("a.txt");
    fs::write(&junk, b"x").unwrap();
    let flat = scratch.path().join("plain.tar");
    run_tar(&["-cf", &lossy(&flat), "-C", &lossy(scratch.path()), "a.txt"]).unwrap();
    let err = read_gpkg_metadata(&flat).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(
        err.message().contains("gpkg file structure mismatch"),
        "{err}"
    );

    // A structurally valid container with no `gpkg-1` marker fails the
    // inner-tar lookup guard.
    let g = build_gpkg("foo-1.0", &[("metadata.tar", b"x")], None);
    let err = read_gpkg_metadata(&g).unwrap_err();
    assert!(err.is_invalid(), "{err:?}");
    assert!(err.message().contains("gpkg-1"), "{err}");
}

/// End to end: an xpak packed with `tar --xattrs` restores attributes
/// under `xattr` (minus the exclude list) and none without it.
// Portage: BinpkgExtractorAsync._xpak_start
#[test]
fn extract_binpkg_restores_xpak_xattrs_under_features_xattr() {
    use std::os::unix::ffi::OsStrExt;
    if !tar_supports_xattrs() {
        eprintln!("skipped: tar has no --xattrs");
        return;
    }
    let tmp = TempDir::new("binpkg-xpak-xattrs").keep();
    let _ = fs::remove_dir_all(&tmp);
    let stage = tmp.join("stage");
    fs::create_dir_all(stage.join("usr/share/xa")).unwrap();
    let f = stage.join("usr/share/xa/f");
    fs::write(&f, b"payload").unwrap();
    let fb = f.as_os_str().as_bytes();
    if crate::helpers::xattr_set(fb, b"user.keep", b"k").is_err() {
        eprintln!("skipped: no user xattrs on {}", tmp.display());
        return;
    }
    crate::helpers::xattr_set(fb, b"user.skip", b"s").unwrap();
    let tar_path = tmp.join("image.tar");
    run_tar(&[
        "-cf",
        &lossy(&tar_path),
        "--xattrs",
        "-C",
        &lossy(&stage),
        ".",
    ])
    .unwrap();
    let pkg = tmp.join("xa-1.tbz2");
    fs::write(
        &pkg,
        make_xpak_binpkg(&fs::read(&tar_path).unwrap(), &[("SLOT", b"0\n")]),
    )
    .unwrap();

    let on = crate::ebuild_merge::XattrPolicy {
        enabled: true,
        exclude: "user.sk*".to_string(),
    };
    let off = crate::ebuild_merge::XattrPolicy::default();
    for (name, policy, keep) in [("on", &on, true), ("off", &off, false)] {
        let image = tmp.join(format!("image-{name}"));
        extract_binpkg(
            &pkg,
            &image,
            &tmp.join(format!("bi-{name}")),
            &GpgVerify::default(),
            policy,
        )
        .expect("extract succeeds");
        let got = image.join("usr/share/xa/f");
        let gb = got.as_os_str().as_bytes();
        assert_eq!(fs::read(&got).unwrap(), b"payload");
        assert_eq!(
            crate::helpers::xattr_get(gb, b"user.keep").ok(),
            keep.then(|| b"k".to_vec()),
            "{name}"
        );
        assert_eq!(
            crate::helpers::xattr_get(gb, b"user.skip").ok(),
            None,
            "{name}"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn verify_binpkg_against_index_reports_size_then_digest_like_portage() {
    // The stat size is checked against the index `SIZE` first, then
    // each digest the record carries.
    let scratch = ScratchDir::new("verify-against-index").unwrap();
    let file = scratch.path().join("pkg-1.0.gpkg.tar");
    fs::write(&file, b"0123456789abcdef").unwrap();
    use md5::Digest as _;
    let md5: String = md5::Md5::digest(b"0123456789abcdef")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    // No SIZE: nothing to verify against.
    let mut record = HashMap::new();
    assert!(verify_binpkg_against_index(&file, &record, None).is_ok());

    // Size mismatch beats everything, even a correct digest.
    record.insert("SIZE".to_string(), "16".to_string());
    record.insert("MD5".to_string(), md5.clone());
    assert!(verify_binpkg_against_index(&file, &record, None).is_ok());
    record.insert("SIZE".to_string(), "10240".to_string());
    let mismatch = verify_binpkg_against_index(&file, &record, None)
        .expect_err("short file must fail the index SIZE");
    assert_eq!(mismatch.name, "size");
    assert_eq!(mismatch.got, "16");
    assert_eq!(mismatch.expected, "10240");

    // Matching size, wrong digest: the hash name is reported.
    record.insert("SIZE".to_string(), "16".to_string());
    record.insert("MD5".to_string(), "0".repeat(32));
    let mismatch =
        verify_binpkg_against_index(&file, &record, None).expect_err("wrong MD5 must fail");
    assert_eq!(mismatch.name, "MD5");
    assert_eq!(mismatch.got, md5);
    assert_eq!(mismatch.expected, "0".repeat(32));

    // The failure block shape: leading blank line, three `!!!` lines,
    // then the bare `File renamed to` line.
    let renamed = scratch.path().join("pkg-1.0.gpkg.tar._checksum_failure_.x");
    let block = digest_failure_block(&file, &mismatch, &renamed);
    assert_eq!(
        block,
        format!("\n!!! Digest verification failed:\n!!! {}", file.display())
            + &format!(
                "\n!!! Reason: Failed on MD5 verification\n!!! Got: {md5}\n!!! Expected: {}\nFile renamed to '{}'\n",
                "0".repeat(32),
                renamed.display(),
            )
    );
}

#[test]
fn checksum_failure_rename_moves_to_a_checksum_failure_sibling() {
    // The corrupt file is renamed to `<basename>._checksum_failure_.<rand>`
    // in its own directory (`<rand>` is 8 chars over `[a-z0-9_]`). A
    // second identical failure reuses the existing sibling instead of
    // piling up evidence.
    // Portage: fetch.py, _checksum_failure_temp_file
    let scratch = ScratchDir::new("checksum-failure-rename").unwrap();
    let dir = scratch.path();
    let first = dir.join("pkg-1.0.gpkg.tar");
    fs::write(&first, b"corrupt-bytes").unwrap();
    let renamed = checksum_failure_rename(&first).expect("rename works");
    assert!(!first.exists(), "the corrupt path is gone");
    assert!(renamed.is_file());
    let name = renamed.file_name().unwrap().to_str().unwrap();
    let rand = name
        .strip_prefix("pkg-1.0.gpkg.tar._checksum_failure_.")
        .unwrap_or_else(|| panic!("{name}"));
    assert_eq!(rand.len(), 8, "{name}");
    assert!(
        rand.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "{name}"
    );

    let second = dir.join("pkg-1.0.gpkg.tar");
    fs::write(&second, b"corrupt-bytes").unwrap();
    let reused = checksum_failure_rename(&second).expect("rename works");
    assert_eq!(reused, renamed, "identical bytes reuse the sibling");
    assert!(!second.exists());
    assert_eq!(fs::read(&reused).unwrap(), b"corrupt-bytes");
}

#[test]
fn index_has_size_and_checksum_needs_both() {
    let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    assert!(index_has_size_and_checksum(&rec(&[
        ("SIZE", "5"),
        ("SHA512", "ab")
    ])));
    assert!(index_has_size_and_checksum(&rec(&[
        ("SIZE", " 5 "),
        ("MD5", "ab")
    ])));
    assert!(!index_has_size_and_checksum(&rec(&[])));
    assert!(!index_has_size_and_checksum(&rec(&[("SIZE", "5")])));
    assert!(!index_has_size_and_checksum(&rec(&[("MD5", "ab")])));
    assert!(!index_has_size_and_checksum(&rec(&[
        ("SIZE", "5"),
        ("MD5", "")
    ])));
    assert!(!index_has_size_and_checksum(&rec(&[
        ("SIZE", "x"),
        ("MD5", "ab")
    ])));
}

#[test]
fn verify_binpkg_against_index_checks_every_digest_portage_verifies() {
    // Portage checks every digest the record carries, not just MD5/SHA1:
    // a record with SIZE plus a tampered SHA512 (or BLAKE2B) fails even
    // though the size matches.
    // Portage: lib/portage/checksum.py, verify_all
    let scratch = ScratchDir::new("verify-every-digest").unwrap();
    let file = scratch.path().join("pkg-1.0.gpkg.tar");
    let body = b"0123456789abcdef";
    fs::write(&file, body).unwrap();
    // One import covers every hasher below: md5/sha1/sha2/blake2 all
    // re-export the same `digest::Digest` trait.
    use sha2::Digest as _;
    let hex = |digest: &[u8]| {
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let correct: &[(&str, String)] = &[
        ("MD5", hex(&md5::Md5::digest(body))),
        ("SHA1", hex(&sha1::Sha1::digest(body))),
        ("SHA256", hex(&sha2::Sha256::digest(body))),
        ("SHA512", hex(&sha2::Sha512::digest(body))),
        ("BLAKE2B", hex(&blake2::Blake2b512::digest(body))),
        ("BLAKE2S", hex(&blake2::Blake2s256::digest(body))),
        // No sha3 hasher existed here before the fix: ground truth from
        // the host hashlib (S0 probe).
        (
            "SHA3_256",
            "a5df4caae9fdb5dbacf667075b709a2f30a115c43168af332062b42d4b0da01f".to_string(),
        ),
        (
            "SHA3_512",
            "59d06155d25dffdb982729de8dce9d7855ca094d8bab8124b347c40668477056b3c27ccb7d71b54043d207ccd187642bf9c8466f9a8d0dbefb4c41633a7e39ef".to_string(),
        ),
    ];
    let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    // All eight correct: verified.
    let owned: Vec<(String, String)> = correct
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let mut full_refs: Vec<(&str, &str)> = vec![("SIZE", "16")];
    full_refs.extend(owned.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    assert!(verify_binpkg_against_index(&file, &rec(&full_refs), None).is_ok());
    // Each algorithm alone and wrong: refused with that algorithm's name.
    for (algo, _) in correct {
        let len = match *algo {
            "MD5" => 32,
            "SHA1" => 40,
            "SHA256" | "BLAKE2S" | "SHA3_256" => 64,
            _ => 128,
        };
        let record = rec(&[("SIZE", "16"), (algo, &"0".repeat(len))]);
        let Err(mismatch) = verify_binpkg_against_index(&file, &record, None) else {
            panic!("wrong {algo} must fail");
        };
        assert_eq!(mismatch.name, *algo);
    }
    // Each algorithm alone and correct: verified.
    for (algo, value) in correct {
        let record = rec(&[("SIZE", "16"), (algo, value)]);
        assert!(
            verify_binpkg_against_index(&file, &record, None).is_ok(),
            "correct {algo} must verify"
        );
    }
}

#[test]
fn verify_binpkg_against_index_parses_size_like_portages_int() {
    // Portage runs SIZE through `int()`: surrounding whitespace is
    // ignored, `"-1"` parses (and then fails the size check), garbage
    // drops `size` from the digests so nothing is verified.
    // Portage: lib/portage/dbapi/bintree.py, _get_digests
    let scratch = ScratchDir::new("verify-int-size").unwrap();
    let file = scratch.path().join("pkg-1.0.gpkg.tar");
    fs::write(&file, b"0123456789abcdef").unwrap();
    let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    // Padded SIZE with no digests: the size alone convicts.
    let mismatch = verify_binpkg_against_index(&file, &rec(&[("SIZE", " 4618 ")]), None)
        .expect_err("padded SIZE 4618 must fail a 16-byte file");
    assert_eq!(mismatch.name, "size");
    assert_eq!(mismatch.got, "16");
    assert_eq!(mismatch.expected, "4618");
    // Negative SIZE parses and then fails the check.
    let mismatch = verify_binpkg_against_index(&file, &rec(&[("SIZE", "-1")]), None)
        .expect_err("SIZE -1 must fail");
    assert_eq!(mismatch.name, "size");
    assert_eq!(mismatch.expected, "-1");
    // Garbage SIZE verifies nothing, like Portage dropping `size`.
    assert!(verify_binpkg_against_index(&file, &rec(&[("SIZE", "x")]), None).is_ok());
    assert!(verify_binpkg_against_index(&file, &rec(&[]), None).is_ok());
}

#[test]
fn parse_index_size_matches_portages_int() {
    // Portage: lib/portage/dbapi/bintree.py, _get_digests
    let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    assert_eq!(parse_index_size(&rec(&[("SIZE", "4618")])), Some(4618));
    assert_eq!(parse_index_size(&rec(&[("SIZE", " 4618 ")])), Some(4618));
    assert_eq!(parse_index_size(&rec(&[("SIZE", "+5")])), Some(5));
    // Negative parses but matches no file: both paths fail closed.
    assert!(parse_index_size(&rec(&[("SIZE", "-1")])).is_some());
    assert_eq!(parse_index_size(&rec(&[("SIZE", "x")])), None);
    assert_eq!(parse_index_size(&rec(&[("SIZE", "")])), None);
    assert_eq!(parse_index_size(&rec(&[])), None);
}

#[test]
fn verify_binpkg_against_index_honours_portage_checksum_filter() {
    // A digest the filter removes is not checked (the `"* -HASH"` idiom;
    // a trailing `"*"` alone disables filtering). The filter cannot
    // excuse the only digest: at least one hash always survives.
    // Portage: _emerge/BinpkgVerifier.py, _start; lib/portage/checksum.py,
    // _apply_hash_filter
    let scratch = ScratchDir::new("verify-checksum-filter").unwrap();
    let file = scratch.path().join("pkg-1.0.gpkg.tar");
    fs::write(&file, b"0123456789abcdef").unwrap();
    let rec = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    use md5::Digest as _;
    let md5: String = md5::Md5::digest(b"0123456789abcdef")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // Wrong SHA512, excused while MD5 remains: verified.
    let record = rec(&[("SIZE", "16"), ("MD5", &md5), ("SHA512", &"0".repeat(128))]);
    assert!(verify_binpkg_against_index(&file, &record, Some("* -SHA512")).is_ok());
    // Same record, filter transparent: refused on SHA512.
    let mismatch = verify_binpkg_against_index(&file, &record, None)
        .expect_err("unfiltered wrong SHA512 must fail");
    assert_eq!(mismatch.name, "SHA512");
    // The filter cannot excuse the only digest: still refused.
    let record = rec(&[("SIZE", "16"), ("SHA512", &"0".repeat(128))]);
    let mismatch = verify_binpkg_against_index(&file, &record, Some("* -SHA512"))
        .expect_err("a lone digest survives the filter");
    assert_eq!(mismatch.name, "SHA512");
    // Correct SHA512 alone survives `"* -MD5"` (nothing to remove): verified.
    let sha512: String = sha2::Sha512::digest(b"0123456789abcdef")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let record = rec(&[("SIZE", "16"), ("SHA512", &sha512)]);
    assert!(verify_binpkg_against_index(&file, &record, Some("* -MD5")).is_ok());
}
