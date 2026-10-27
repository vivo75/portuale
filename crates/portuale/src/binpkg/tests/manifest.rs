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
    let (_guard, g) = build_gpkg("foo-1.0", &[("gpkg-1", b""), ("image.tar", b"img")], None);
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
    let (_guard, g) = build_gpkg(
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
    let (_guard, g) = build_gpkg(
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
    let (_guard, g) = build_gpkg(
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
    let (_guard, g) = build_gpkg("foo-1.0", &[("gpkg-1", b"")], Some(&manifest));
    let err = verify_gpkg_manifest(&g, &GpgVerify::default()).unwrap_err();
    assert!(err.contains("not present in the container"), "{err}");
}

#[test]
fn verify_gpkg_manifest_accepts_a_real_signed_container() {
    let home = test_gpg_home("signed-ok");
    let g = fixture("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar");
    // Signed by unmodified `bin/gpkg-helper.py compress` with the
    // trusted test key: Manifest clear-sign + both detached sidecars.
    verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true))
        .expect("signed fixture verifies");
    // `binpkg-request-signature` on a fully-signed container.
    verify_gpkg_manifest(&g, &test_gpg_verify(&home, true, true))
        .expect("signed fixture verifies under request-signature");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn verify_gpkg_manifest_skips_gpg_entirely_under_ignore_signature() {
    // Under `binpkg-ignore-signature` no subprocess runs at all (the
    // cleartext `DATA` body reads straight through the armor).
    let gpg = GpgVerify {
        verify_signature: false,
        request_signature: false,
        base_command: "/does/not/exist-gpg".to_string(),
        gpg_home: "/does/not/exist".to_string(),
    };
    verify_gpkg_manifest(&fixture("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"), &gpg)
        .expect("ignore-signature never invokes gpg");
}

#[test]
fn verify_gpkg_manifest_rejects_an_unsigned_container_under_request_signature() {
    let home = test_gpg_home("unsigned-request");
    // No `.sig` members and a plain Manifest: the clear-sign verify
    // runs first and reports NODATA ("binpkg appears unsigned").
    let err = verify_gpkg_manifest(
        &fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
        &test_gpg_verify(&home, true, true),
    )
    .unwrap_err();
    assert!(err.contains("GnuPG verification failed"), "{err}");
    assert!(err.contains("binpkg appears unsigned"), "{err}");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn verify_gpkg_manifest_rejects_a_tampered_signed_member() {
    let home = test_gpg_home("tampered");
    // A flipped byte breaks the detached `.sig` before digests compare.
    let (_guard, g) = repack_gpkg_with_mutation(
        &fixture("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"),
        "tampered",
        |outer| {
            let member = outer.join("gpgsignedpkg-1.0/image.tar.gz");
            let mut bytes = fs::read(&member).unwrap();
            let mid = bytes.len() / 2;
            bytes[mid] ^= 0xff;
            fs::write(&member, bytes).unwrap();
        },
    );
    let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
    assert!(err.contains("GnuPG verification failed"), "{err}");
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(g.parent().unwrap());
}

#[test]
fn verify_gpkg_manifest_rejects_a_member_without_its_sig_sidecar() {
    let home = test_gpg_home("missing-sidecar");
    // Clear-signed Manifest but no `payload.bin.sig`.
    // Portage: gpkg.py, MissingSignature
    let (_scratch, g) = build_signed_gpkg(&home, GPG_TRUSTED_KEY, false, None);
    let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
    assert!(err.contains("signature not found"), "{err}");
    kill_gpg_agent(&home);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(g.parent().unwrap());
}

#[test]
fn verify_gpkg_manifest_rejects_a_signature_from_an_untrusted_key() {
    let home = test_gpg_home("untrusted");
    // Valid crypto from the untrusted key: trust check demands
    // `TRUST_ULTIMATE`/`TRUST_FULLY`.
    let (_scratch, g) = build_signed_gpkg(&home, GPG_UNTRUSTED_KEY, true, None);
    let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
    assert!(err.contains("GnuPG verification failed"), "{err}");
    assert!(err.contains("undefined trust"), "{err}");
    kill_gpg_agent(&home);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(g.parent().unwrap());
}

#[test]
fn verify_gpkg_manifest_rejects_a_sidecar_from_an_untrusted_key() {
    let home = test_gpg_home("untrusted-sidecar");
    // Manifest signed by the trusted key, member sidecar by the
    // untrusted one -- the Manifest passes, the member fails.
    let (_scratch, g) = build_signed_gpkg(&home, GPG_TRUSTED_KEY, true, Some(GPG_UNTRUSTED_KEY));
    let err = verify_gpkg_manifest(&g, &test_gpg_verify(&home, false, true)).unwrap_err();
    assert!(err.contains("GnuPG verification failed"), "{err}");
    assert!(err.contains("undefined trust"), "{err}");
    kill_gpg_agent(&home);
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(g.parent().unwrap());
}
