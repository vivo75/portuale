# crates/portuale/src/binpkg/tests/manifest.rs
Manifest verification tests: digests, membership, signatures.

## fn verify_gpkg_manifest_accepts_the_real_fixture
- The committed fixture's Manifest verifies.

## fn verify_gpkg_manifest_rejects_a_size_mismatch_without_hashing
- Size is checked before any hashing.

## fn verify_gpkg_manifest_accepts_a_real_signed_container
- The committed fixture was signed by unmodified `bin/gpkg-helper.py compress` with the trusted test key: the Manifest clear-sign plus both detached `.sig` sidecars verify. Also verifies under `binpkg-request-signature`.

## fn verify_gpkg_manifest_skips_gpg_entirely_under_ignore_signature
- Under `binpkg-ignore-signature` even a garbage `gpg` binary path succeeds, proving no subprocess runs at all; the cleartext `DATA` body is read straight through the armor (the standing unsigned-container behaviour).

## fn verify_gpkg_manifest_rejects_an_unsigned_container_under_request_signature
- The unsigned fixture has no `.sig` members and a plain Manifest: Portage's `_verify_binpkg` tries the clear-sign verify first and reports the NODATA cause (Portage's `show_gpg_error` "binpkg appears unsigned" summary).

## fn verify_gpkg_manifest_rejects_a_tampered_signed_member
- Flipping a byte of the signed `image.tar.gz` breaks the detached `.sig` before digests are even compared. Repacks via `repack_gpkg_with_mutation` (which unpacks the outer tar and repacks, since flipping packed bytes would only corrupt the tar framing).

## fn verify_gpkg_manifest_rejects_a_member_without_its_sig_sidecar
- Validly clear-signed Manifest but no `payload.bin.sig`: Portage's `MissingSignature` (`gpkg.py:1783-1786`).

## fn verify_gpkg_manifest_rejects_a_signature_from_an_untrusted_key
- Signed end-to-end with the committed untrusted test key: the cryptography is valid, but Portage's `_check_gpg_status` demands `TRUST_ULTIMATE`/`TRUST_FULLY` ("signed with a known key of undefined trust").
