//! Digest checks against the index and checksum-failure renames.

use crate::binpkg::*;

/// Whole seconds since the epoch, the same integer Portage's `_mtime_`
/// index field records.
// Portage: `bintree._pkgindex_entry`.
pub(crate) fn file_mtime(st: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    st.mtime()
}

/// A merge-time binpkg size/digest mismatch: the `(name, value, expected)`
/// triple, where `name` is `"size"` or a hash name (`"MD5"`, `"SHA1"`,
/// ...), `got` the live file's value and `expected` the index's claim.
// Portage: `_emerge/BinpkgVerifier._digest_exception`.
pub struct BinpkgDigestMismatch {
    /// `"size"` or the hash name, as reported.
    pub name: String,
    /// The live file's value (`st_size` / lowercase hex digest).
    pub got: String,
    /// The index record's claim.
    pub expected: String,
}

/// Verify a local `$PKGDIR` binpkg at merge time against the
/// `<pkgdir>/Packages` index record that vouches for it: `SIZE` first via
/// `stat(2)`, then every digest the record carries. A record with no
/// `SIZE` verifies nothing. Returns the first mismatch; the caller renames
/// the file and prints the failure block. Files the index does not vouch
/// for are already fully parsed at scan and never reach here with a record.
// Portage: `_emerge/BinpkgVerifier`, `bintree._get_digests`.
pub fn verify_binpkg_against_index(
    path: &Path,
    record: &HashMap<String, String>,
) -> Result<(), BinpkgDigestMismatch> {
    let indexed_size = record.get("SIZE").and_then(|s| s.parse::<u64>().ok());
    let Some(expected_size) = indexed_size else {
        return Ok(());
    };
    let actual_size = fs::metadata(path).map(|st| st.len()).unwrap_or(u64::MAX);
    if actual_size != expected_size {
        return Err(BinpkgDigestMismatch {
            name: "size".to_string(),
            got: actual_size.to_string(),
            expected: expected_size.to_string(),
        });
    }
    for algo in ["MD5", "SHA1", "SHA256", "SHA512"] {
        let Some(expected) = record.get(algo).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(actual) = file_hex_digest(path, algo) else {
            continue;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(BinpkgDigestMismatch {
                name: algo.to_string(),
                got: actual,
                expected: expected.clone(),
            });
        }
    }
    Ok(())
}

/// Hex digest of the whole file with `algo` (`MD5`/`SHA1`/`SHA256`/
/// `SHA512`). `None` for an unknown name or an unreadable file.
pub(crate) fn file_hex_digest(path: &Path, algo: &str) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let hex = |digest: &[u8]| digest.iter().map(|b| format!("{b:02x}")).collect();
    match algo {
        "MD5" => {
            use md5::Digest as _;
            Some(hex(&md5::Md5::digest(&bytes)))
        }
        "SHA1" => {
            use sha1::Digest as _;
            Some(hex(&sha1::Sha1::digest(&bytes)))
        }
        "SHA256" => {
            use sha2::Digest as _;
            Some(hex(&sha2::Sha256::digest(&bytes)))
        }
        "SHA512" => {
            use sha2::Digest as _;
            Some(hex(&sha2::Sha512::digest(&bytes)))
        }
        _ => None,
    }
}

/// Rename a digest-failed file to `<basename>._checksum_failure_.<rand>`
/// in its own directory and return the new path. A same-size same-md5
/// `<basename>._checksum_failure_.*` duplicate is reused instead (the
/// corrupt file is unlinked), so repeated failures don't pile up evidence
/// files. `None` when the rename itself fails (the caller still fails the
/// package).
// Portage: `package.ebuild.fetch._checksum_failure_temp_file`.
pub fn checksum_failure_rename(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let basename = path.file_name()?.to_str()?;
    let prefix = format!("{basename}._checksum_failure_.");
    let size = fs::metadata(path).map(|st| st.len()).ok()?;
    if let Ok(entries) = portage_util::read_dir_paths(dir) {
        let checksum = file_hex_digest(path, "MD5");
        for entry in entries {
            let name = entry
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !name.starts_with(&prefix) {
                continue;
            }
            let same_size = fs::metadata(&entry)
                .map(|st| st.len() == size)
                .unwrap_or(false);
            if !same_size {
                continue;
            }
            let same_md5 = match (&checksum, file_hex_digest(&entry, "MD5")) {
                (Some(a), Some(b)) => a == &b,
                _ => false,
            };
            if same_md5 && fs::remove_file(path).is_ok() {
                return Some(entry);
            }
        }
    }
    // The trailing random part is 8 characters from the same 37-character
    // alphabet Portage uses; the loop retries on collision instead of
    // clobbering.
    for attempt in 0..100u32 {
        let candidate = dir.join(format!("{prefix}{}", checksum_failure_rand(attempt)));
        if candidate.exists() {
            continue;
        }
        if fs::rename(path, &candidate).is_ok() {
            return Some(candidate);
        }
        if !path.exists() {
            break;
        }
    }
    None
}

/// An 8-character name over `"abcdefghijklmnopqrstuvwxyz0123456789_"`:
/// 8 bytes from the OS mixed onto that alphabet. The `attempt` salts the
/// deterministic fallback (no OS entropy available) so retries still vary;
/// the caller re-checks `exists()` either way.
pub(crate) fn checksum_failure_rand(attempt: u32) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    let mut bytes = [0u8; 8];
    let have_entropy = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| {
            use std::io::Read;
            f.read_exact(&mut bytes)
        })
        .is_ok();
    if !have_entropy {
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.write_u32(std::process::id());
        h.write_u32(attempt);
        let digest = h.finish();
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (digest >> ((i % 8) * 8)) as u8;
        }
    }
    bytes
        .iter()
        .map(|b| ALPHABET[(usize::from(*b) * ALPHABET.len()) / 256] as char)
        .collect()
}

/// The digest-failure block, byte-shaped: a leading blank line, three
/// `!!!` lines (the path, the `Failed on <name> verification` reason,
/// `Got`/`Expected`), then the bare `File renamed to '<path>'` line (no
/// `!!!` prefix). `renamed` is the `checksum_failure_rename` result; when
/// the rename itself failed the caller passes the original path, so the
/// line always names an existing location.
// Portage: `_emerge/BinpkgVerifier._digest_exception`.
pub fn digest_failure_block(
    path: &Path,
    mismatch: &BinpkgDigestMismatch,
    renamed: &Path,
) -> String {
    format!(
        "\n!!! Digest verification failed:\n!!! {}\n!!! Reason: Failed on {} verification\n!!! Got: {}\n!!! Expected: {}\nFile renamed to '{}'\n",
        path.display(),
        mismatch.name,
        mismatch.got,
        mismatch.expected,
        renamed.display(),
    )
}

pub(crate) fn run_tar(args: &[&str]) -> Result<(), String> {
    let status = Command::new("tar")
        .args(args)
        .status()
        .map_err(|e| format!("failed to spawn tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar {args:?} failed ({status})"));
    }
    Ok(())
}

pub(crate) fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
