//! Verifying a gpkg Manifest.

use crate::binpkg::*;

/// Real `portage.gpkg.gpkg._verify_binpkg` (`lib/portage/gpkg.py:1626`),
/// checksum layer + GPG signature layer. A `.gpkg.tar` is a plain
/// (outer) tar whose members are every one exactly one level deep under
/// a single shared prefix directory (real "gpkg file structure" guard);
/// the `<prefix>/Manifest` member records one
/// `DATA <basename> <size> BLAKE2B <hex> SHA512 <hex>` line per other
/// member (real `_record_checksum` / `_add_manifest`, and
/// `MANIFEST2_HASH_DEFAULTS = {BLAKE2B, SHA512}`). This checks:
///   - `#56`/GLEP 78: the one prefix is a **real** directory and every
///     trusted member inside it (`gpkg-1`, `Manifest`, the two inner
///     tars, `.sig` sidecars) is a **regular file** -- `symlink_metadata`,
///     no follow, before anything is read (real's own reader can never
///     read through one either; see `TEST/findings/l2.md` "## #56 S0");
///   - a `Manifest` member exists (real `MissingSignature` otherwise);
///   - the GPG layer (real `request_signature` / `signature_exist` /
///     `verify_signature`): when the container carries any `.sig`
///     member or an inline-signed Manifest -- or `gpg.request_signature`
///     (`FEATURES=binpkg-request-signature`) says signatures are
///     mandatory -- the Manifest is verified as a clear-signed message
///     and every other member against its detached `.sig` sidecar, via
///     the system `gpg` (see [`GpgVerify`]). A failed Manifest check is
///     fatal only when `gpg.verify_signature` (real's own
///     `if self.verify_signature: raise` -- `binpkg-ignore-signature`
///     falls back to the raw Manifest bytes); a missing sidecar is
///     always fatal under `request_signature` (real `MissingSignature`),
///     except for the never-signed `gpkg-1` version marker. `.sig`
///     members themselves are digest-checked against their own Manifest
///     records, exactly like every other member (real's loop verifies
///     the `.sig` file bytes too -- `f_signature` is only `None` for
///     choosing *plain* over *GPG* verification, not for skipping).
///   - every member has a `DATA` record whose `size` and *every*
///     recognised hash match -- reusing `portage_fetch::verify_digests`
///     (size first, then BLAKE2B/SHA512), with real's "at least one
///     supported checksum" floor;
///   - the member set and the record set match exactly (real's
///     `unverified_files` / `unverified_manifest` leftovers checks).
///
/// Deliberate cuts: no dropped-privilege (`nobody`/`nogroup`) `gpg`
/// spawn when root (real `checksum_helper`'s own `GPG_VERIFY_USER_DROP`;
/// tests run unprivileged and production merges already run as root
/// throughout); `[PORTAGE_CONFIG]`/`[SIGNATURE]` substitution is plain
/// whitespace splitting, not real's `shlex` + `varexpand`.
pub(crate) fn verify_gpkg_manifest(gpkg_path: &Path, gpg: &GpgVerify) -> Result<(), String> {
    if !gpkg_path.is_file() {
        return Err(format!("{}: not a file", gpkg_path.display()));
    }

    // `#58` S5: the outer container is read in process. Real
    // `_verify_binpkg`'s structure rules (relative names, one level deep
    // under one common prefix, no duplicates) and `#56`'s "a trusted
    // name is a regular member" all come from the tar headers here,
    // before any member bytes are read.
    let (members, _) = read_outer_members(gpkg_path)?;
    let manifest_member = members
        .iter()
        .find(|member| member.name == "Manifest")
        .ok_or_else(|| {
            format!(
                "{}: Manifest not found in the gpkg container",
                gpkg_path.display()
            )
        })?;
    let manifest_bytes = outer_member_read(gpkg_path, &manifest_member.raw_name, |entry| {
        read_member_bytes(entry)
    })?;
    let manifest_text = String::from_utf8_lossy(&manifest_bytes);

    // Real `_verify_binpkg`'s own GPG trigger (`gpkg.py:1682-1686` +
    // `:1711-1712`): "if any signature exists, we assume all files have
    // signature" -- any `.sig` sidecar member, or an inline PGP block in
    // the Manifest itself.
    let signature_exist = members.iter().any(|member| member.name.ends_with(".sig"))
        || manifest_text.contains("-----BEGIN PGP SIGNATURE-----");

    // Real Manifest-signature branch (`gpkg.py:1714-1731`): verify the
    // clear-signed Manifest and parse the cleartext. A failure is fatal
    // only under `verify_signature` -- with `binpkg-ignore-signature`
    // real falls back to the raw bytes (and the armor-skipping parse
    // below reads the cleartext body straight through, as portuale
    // always used to).
    let manifest_text: std::borrow::Cow<'_, str> = if gpg.request_signature || signature_exist {
        match verify_clearsigned_manifest(&manifest_bytes, gpg, gpkg_path) {
            Ok(cleartext) => {
                std::borrow::Cow::Owned(String::from_utf8_lossy(&cleartext).into_owned())
            }
            Err(e) => {
                if gpg.verify_signature {
                    return Err(e);
                }
                std::borrow::Cow::Borrowed(manifest_text.as_ref())
            }
        }
    } else {
        std::borrow::Cow::Borrowed(manifest_text.as_ref())
    };

    // Parse the `DATA` lines. When signature checking is off
    // (`binpkg-ignore-signature`) a clear-signed Manifest is still
    // parsed raw, so PGP-armor lines are skipped and the cleartext body
    // read straight through; a verified cleartext has no armor left.
    let mut records: HashMap<String, portage_fetch::DistfileDigests> = HashMap::new();
    for line in manifest_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("-----") || line.starts_with("Hash:") {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("DATA") => {}
            // A PGP-armored Manifest wraps `DATA` lines in a signed
            // block; anything else on a non-blank line is malformed.
            Some(_) if manifest_text.contains("BEGIN PGP") => continue,
            _ => {
                return Err(format!(
                    "{}: invalid Manifest line {line:?}",
                    gpkg_path.display()
                ));
            }
        }
        let name = parts
            .next()
            .ok_or_else(|| format!("{}: Manifest DATA line missing a name", gpkg_path.display()))?;
        let size = parts
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| {
                format!(
                    "{}: Manifest DATA line for {name:?} has no valid size",
                    gpkg_path.display()
                )
            })?;
        let rest: Vec<&str> = parts.collect();
        let mut hashes = HashMap::new();
        let mut i = 0;
        while i + 1 < rest.len() {
            hashes.insert(rest[i].to_string(), rest[i + 1].to_string());
            i += 2;
        }
        if records
            .insert(
                name.to_string(),
                portage_fetch::DistfileDigests { size, hashes },
            )
            .is_some()
        {
            return Err(format!(
                "{}: Manifest lists {name:?} more than once",
                gpkg_path.display()
            ));
        }
    }

    let mut unmatched: std::collections::BTreeSet<String> = records.keys().cloned().collect();
    for member in &members {
        let name = &member.name;
        if name == "Manifest" {
            continue;
        }
        let record = records.get(name.as_str()).ok_or_else(|| {
            format!(
                "{}: container member {name:?} is not listed in the Manifest",
                gpkg_path.display()
            )
        })?;
        if !record
            .hashes
            .keys()
            .any(|h| h == "BLAKE2B" || h == "SHA512")
        {
            return Err(format!(
                "{}: Manifest record for {name:?} carries no supported checksum",
                gpkg_path.display()
            ));
        }
        if !name.ends_with(".sig")
            && (gpg.request_signature || signature_exist)
            && gpg.verify_signature
        {
            // Real per-file GPG branch (`gpkg.py:1764-1788`): a member
            // whose `.sig` sidecar exists is verified detached before
            // its digests are compared; the never-signed `gpkg-1`
            // version marker is digest-only; anything else is
            // `MissingSignature`.
            let sidecar = format!("{name}.sig");
            match members.iter().find(|other| other.name == sidecar) {
                Some(sidecar_member) => {
                    // Signature verification needs the bytes in memory
                    // (`run_gpg_verify` writes them to gpg's stdin); the
                    // digest below reuses them instead of re-reading.
                    let member_bytes = outer_member_read(gpkg_path, &member.raw_name, |entry| {
                        read_member_bytes(entry)
                    })?;
                    let sig_bytes =
                        outer_member_read(gpkg_path, &sidecar_member.raw_name, |entry| {
                            read_member_bytes(entry)
                        })?;
                    verify_detached_signature(
                        &member_bytes,
                        &sig_bytes,
                        gpg,
                        &format!("{}: container member {name:?}", gpkg_path.display()),
                    )?;
                    portage_fetch::verify_digests_reader(name, &member_bytes[..], record).map_err(
                        |e| {
                            format!(
                                "{}: gpkg Manifest verification failed: {e}",
                                gpkg_path.display()
                            )
                        },
                    )?;
                    unmatched.remove(name.as_str());
                    continue;
                }
                None if name != "gpkg-1" => {
                    return Err(format!(
                        "{}: container member {name:?} signature not found in the gpkg container",
                        gpkg_path.display()
                    ));
                }
                None => {}
            }
        }
        // `#58` S5: the member is hashed straight from its container
        // stream (`verify_digests_reader`) -- no extraction to disk and
        // no buffering, so a large image member is a sequential read.
        outer_member_read(gpkg_path, &member.raw_name, |reader| {
            portage_fetch::verify_digests_reader(name, reader, record).map_err(|e| e.to_string())
        })
        .map_err(|e| {
            format!(
                "{}: gpkg Manifest verification failed: {e}",
                gpkg_path.display()
            )
        })?;
        unmatched.remove(name.as_str());
    }

    if !unmatched.is_empty() {
        return Err(format!(
            "{}: Manifest lists files not present in the container: {}",
            gpkg_path.display(),
            unmatched.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

/// Read an outer member fully into memory: only the small `Manifest` /
/// `.sig` members and the gpg branch's detached-signature input use this;
/// digest verification streams instead (`verify_digests_reader`).
pub(crate) fn read_member_bytes(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `read_member_bytes` with the `#58` S1 zip-bomb cap: used for the inner
/// metadata member, which the decompressed-size cap
/// ([`MAX_INNER_METADATA_BYTES`]) would otherwise not bound if it stayed
/// compressed.
pub(crate) fn read_member_bytes_capped(
    mut reader: impl Read,
    cap: u64,
    label: &str,
) -> Result<Vec<u8>, BinpkgError> {
    let mut bytes = Vec::new();
    // A truncated/corrupt member would fail real's Manifest size+digest
    // check on the metadata member first (DigestException -> skip).
    reader
        .by_ref()
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| BinpkgError::Invalid(format!("{label}: {e}")))?;
    if bytes.len() as u64 > cap {
        return Err(BinpkgError::Invalid(format!(
            "{label}: larger than {cap} bytes"
        )));
    }
    Ok(bytes)
}
