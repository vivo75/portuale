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

/// The index `SIZE`, parsed once for both verifiers the way Portage's
/// `int()` parses it: surrounding whitespace ignored, one optional
/// sign, ASCII digits. A negative (or out-of-`u64`-range) value parses
/// in Portage too (`int("-1") == -1`), but no file can be that size, so
/// it maps to `u64::MAX` and both paths fail the size check exactly as
/// Portage's `st_size != -1` comparison does. Garbage (`"x"`, `""`) is
/// `None`: Portage drops `size` from its digests then.
// Portage: `bintree._get_digests`.
pub(crate) fn parse_index_size(record: &HashMap<String, String>) -> Option<u64> {
    let text = record.get("SIZE")?.trim();
    let (negative, digits) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else {
        (false, text)
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if negative {
        return Some(u64::MAX);
    }
    Some(digits.parse::<u64>().unwrap_or(u64::MAX))
}

/// The `Expected:` value for a size mismatch: the parsed number, except
/// a sentinel parse (negative or out-of-range `SIZE`) prints the
/// index's own trimmed text the way Portage prints its parsed `int`.
pub(crate) fn index_size_expected(record: &HashMap<String, String>, parsed: u64) -> String {
    if parsed == u64::MAX
        && let Some(raw) = record.get("SIZE")
    {
        let trimmed = raw.trim().trim_start_matches('+');
        if trimmed != u64::MAX.to_string() {
            return trimmed.to_string();
        }
    }
    parsed.to_string()
}

/// Applies `PORTAGE_CHECKSUM_FILTER` to `hashes`, keeping at least one hash
/// (`portage.checksum._hash_filter` / `_apply_hash_filter`).
pub(crate) fn apply_checksum_filter(
    mut hashes: Vec<String>,
    filter_str: Option<&str>,
) -> Vec<String> {
    let Some(filter_str) = filter_str else {
        return hashes;
    };
    let tokens: Vec<String> = filter_str
        .split_whitespace()
        .map(|s| s.to_ascii_uppercase())
        .collect();
    if tokens.is_empty() || tokens.last().map(String::as_str) == Some("*") || hashes.len() <= 1 {
        return hashes;
    }
    let accepts = |hash_name: &str| -> bool {
        for token in tokens.iter().rev() {
            if token == "*" || token == hash_name {
                return true;
            }
            if let Some(rest) = token.strip_prefix('-')
                && (rest == "*" || rest == hash_name)
            {
                return false;
            }
        }
        false
    };
    let mut i = 0;
    while i < hashes.len() && hashes.len() > 1 {
        if !accepts(&hashes[i]) {
            hashes.remove(i);
        } else {
            i += 1;
        }
    }
    hashes
}

/// Every digest the verifier hashes over the file, ASCII-sorted the way
/// Portage's `verify_all` iterates `sorted(verifiable_hash_types)`
/// (`RMD160`/`WHIRLPOOL` included: Portage verifies them whenever the host
/// provides a hasher for them).
const VERIFIED_DIGESTS: [&str; 10] = [
    "BLAKE2B",
    "BLAKE2S",
    "MD5",
    "RMD160",
    "SHA1",
    "SHA256",
    "SHA3_256",
    "SHA3_512",
    "SHA512",
    "WHIRLPOOL",
];

/// The digest names the verifier checks and the success line prints: the
/// `VERIFIED_DIGESTS` keys the record carries non-empty, narrowed by
/// `PORTAGE_CHECKSUM_FILTER` (`None` disables the filter). Shared by the
/// verifier and the digest-names success line so both see the same set,
/// the way Portage verifies and prints the same filtered digests.
// Portage: `bintree._get_digests`, `_emerge/BinpkgVerifier` (`_start`, `_display_success`).
pub(crate) fn selected_digest_names(
    record: &HashMap<String, String>,
    checksum_filter: Option<&str>,
) -> Vec<String> {
    let names: Vec<String> = VERIFIED_DIGESTS
        .iter()
        .filter(|algo| record.get(**algo).is_some_and(|s| !s.is_empty()))
        .map(|algo| (*algo).to_string())
        .collect();
    apply_checksum_filter(names, checksum_filter)
}

/// Verify a binpkg at merge time against the index record that vouches
/// for it: `SIZE` first via `stat(2)`, then every digest the record
/// carries that survives `PORTAGE_CHECKSUM_FILTER` (`None` disables the
/// filter). A record with no parseable `SIZE` verifies nothing. Returns
/// the first mismatch; the caller renames the file and prints the
/// failure block. Files the index does not vouch for are already fully
/// parsed at scan and never reach here with a record.
// Portage: `_emerge/BinpkgVerifier`, `bintree._get_digests`.
pub fn verify_binpkg_against_index(
    path: &Path,
    record: &HashMap<String, String>,
    checksum_filter: Option<&str>,
) -> Result<(), BinpkgDigestMismatch> {
    let Some(expected_size) = parse_index_size(record) else {
        return Ok(());
    };
    let actual_size = fs::metadata(path).map(|st| st.len()).unwrap_or(u64::MAX);
    if actual_size != expected_size {
        return Err(BinpkgDigestMismatch {
            name: "size".to_string(),
            got: actual_size.to_string(),
            expected: index_size_expected(record, expected_size),
        });
    }
    let names = selected_digest_names(record, checksum_filter);
    for algo in &names {
        let Some(expected) = record.get(algo).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(actual) = file_hex_digest(path, algo) else {
            continue;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(BinpkgDigestMismatch {
                name: algo.clone(),
                got: actual,
                expected: expected.clone(),
            });
        }
    }
    Ok(())
}

/// Whether a remote index record carries both a parseable `SIZE` and at
/// least one non-empty checksum field. A record with only one of the two
/// (or neither) is refused after the download.
// Portage: `_emerge/BinpkgVerifier._start`, `bintree._get_digests`.
pub(crate) fn index_has_size_and_checksum(record: &HashMap<String, String>) -> bool {
    parse_index_size(record).is_some()
        && VERIFIED_DIGESTS
            .iter()
            .any(|k| record.get(*k).is_some_and(|v| !v.is_empty()))
}

/// The refusal block for a remote record without size and checksum: a
/// leading blank line, the `.partial` path, the reason, and no `File
/// renamed` line (the file is left as is).
// Portage: `_emerge/BinpkgVerifier._missing_digests_exception`.
pub fn missing_digests_block(partial: &Path) -> String {
    format!(
        "\n!!! Digest verification failed:\n!!! {}\n!!! Reason: binhost's Packages index lacks size and checksum\n!!! for this package\n",
        partial.display()
    )
}

/// Hex digest of `bytes` with `algo`, the in-memory half of
/// [`file_hex_digest`]. `None` for an unknown name.
pub(crate) fn bytes_hex_digest(bytes: &[u8], algo: &str) -> Option<String> {
    let hex = portage_util::hex_lower;
    match algo {
        "MD5" => {
            use md5::Digest as _;
            Some(hex(&md5::Md5::digest(bytes)))
        }
        "RMD160" => {
            use ripemd::Digest as _;
            Some(hex(&ripemd::Ripemd160::digest(bytes)))
        }
        "SHA1" => {
            use sha1::Digest as _;
            Some(hex(&sha1::Sha1::digest(bytes)))
        }
        "SHA256" => {
            use sha2::Digest as _;
            Some(hex(&sha2::Sha256::digest(bytes)))
        }
        "SHA512" => {
            use sha2::Digest as _;
            Some(hex(&sha2::Sha512::digest(bytes)))
        }
        "BLAKE2B" => {
            use blake2::Digest as _;
            Some(hex(&blake2::Blake2b512::digest(bytes)))
        }
        "BLAKE2S" => {
            use blake2::Digest as _;
            Some(hex(&blake2::Blake2s256::digest(bytes)))
        }
        "SHA3_256" => {
            use sha3::Digest as _;
            Some(hex(&sha3::Sha3_256::digest(bytes)))
        }
        "SHA3_512" => {
            use sha3::Digest as _;
            Some(hex(&sha3::Sha3_512::digest(bytes)))
        }
        "WHIRLPOOL" => {
            use whirlpool::Digest as _;
            Some(hex(&whirlpool::Whirlpool::digest(bytes)))
        }
        _ => None,
    }
}

/// Hex digest of the whole file with `algo`. `None` for an unknown
/// name or an unreadable file.
pub(crate) fn file_hex_digest(path: &Path, algo: &str) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    bytes_hex_digest(&bytes, algo)
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

pub(crate) fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    portage_util::read_dir_paths(dir).map_err(|e| format!("{}: {e}", dir.display()))
}
