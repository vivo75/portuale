//! Digest checks against the index and checksum-failure renames.

use crate::binpkg::*;

/// Real `os.lstat(...)[stat.ST_MTIME]` -- whole seconds, the same
/// integer real portage's own `_mtime_` index field records
/// (`bintree.py:2306`'s own `d["_mtime_"] = str(st[stat.ST_MTIME])`).
pub(crate) fn file_mtime(st: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    st.mtime()
}

/// A merge-time binpkg size/digest mismatch: real `_emerge/
/// BinpkgVerifier._digest_exception`'s own `(name, value, expected)`
/// triple (`BinpkgVerifier.py`), where `name` is `"size"` or a hash
/// name (`"MD5"`, `"SHA1"`, ...), `got` the live file's value and
/// `expected` the `<pkgdir>/Packages` index's claim.
pub struct BinpkgDigestMismatch {
    /// `"size"` or the hash name, exactly as real reports it.
    pub name: String,
    /// The live file's value (`st_size` / lowercase hex digest).
    pub got: String,
    /// The index record's claim.
    pub expected: String,
}

/// Real `_emerge/BinpkgVerifier` + `bintree._get_digests` (backlog
/// #174): verify a local `$PKGDIR` binpkg at merge time against the
/// `<pkgdir>/Packages` index record that vouches for it. The size
/// (`SIZE`) is checked first via `stat(2)`; then every digest the
/// record carries (`MD5`/`SHA1`, the two real `_pkgindex_entry`
/// writes, plus `SHA256`/`SHA512` when present) is hashed over the
/// file and compared. A record with no `SIZE` verifies nothing --
/// real's own `if "size" not in digests: return OK` (`BinpkgVerifier.
/// _start`). Returns the first mismatch; the caller renames the file
/// (`checksum_failure_rename`) and prints real's exact failure block
/// (`digest_failure_block`). Files the index does not vouch for are
/// already fully parsed at scan and never reach here with a record.
///
/// Fixed digest list, disclosed: real `_get_digests` collects every
/// key in `get_valid_checksum_keys()` (host-dependent; on this host:
/// `BLAKE2B/BLAKE2S/MD5/RMD160/SHA1/SHA256/SHA3_256/SHA3_512/SHA512/
/// WHIRLPOOL/size`) and checks them in frozenset order; portuale
/// checks `MD5`/`SHA1`/`SHA256`/`SHA512` in that fixed order and skips
/// the rest (`RMD160`/`WHIRLPOOL`/`BLAKE2B`/`BLAKE2S`/`SHA3_256`/
/// `SHA3_512` need hashers portuale doesn't vendor -- not small).
/// Real's own local `--regen` writes only `MD5`/`SHA1` (real
/// `_pkgindex_hashes`), so the gap only bites on foreign index stanzas
/// carrying the wider keys.
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
/// `SHA512`), the same whole-file readers real `perform_multiple_
/// checksums` runs for a `Packages` entry. `None` for an unknown name
/// or an unreadable file.
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

/// Real `portage.package.ebuild.fetch._checksum_failure_temp_file`
/// (`fetch.py:293`): rename a digest-failed file to
/// `<basename>._checksum_failure_.<rand>` in its own directory and
/// return the new path. A same-size same-md5
/// `<basename>._checksum_failure_.*` duplicate is reused instead (the
/// corrupt file is unlinked, real's own `os.unlink(filename)`), so
/// repeated failures don't pile up evidence files. `None` when the
/// rename itself fails (the caller still fails the package).
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
    // Real `tempfile.mkstemp("", <basename> + "._checksum_failure_.",
    // dir)` (`fetch.py:692-696`): the trailing random part is 8
    // characters from `tempfile._RandomNameSequence.characters`
    // (`"abcdefghijklmnopqrstuvwxyz0123456789_"`), `os.urandom`-seeded.
    // Match that shape exactly (`<rand>` is `[a-z0-9_]{8}`); the loop
    // retries on the (absurd) collision instead of clobbering.
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

/// Real `tempfile._RandomNameSequence`'s own 8-character name
/// (`tempfile.py`: 8 `choices` over `"abcdefghijklmnopqrstuvwxyz0123456789_"`):
/// 8 bytes from the OS mixed onto the same 37-character alphabet. The
/// `attempt` salts the deterministic fallback (no OS entropy available)
/// so retries still vary; the caller re-checks `exists()` either way.
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

/// Real `_emerge/BinpkgVerifier._digest_exception`'s own failure block
/// (`BinpkgVerifier.py`), byte-shaped: a leading blank line, three
/// `!!!` lines (the path, the `Failed on <name> verification` reason,
/// `Got`/`Expected`), then the bare `File renamed to '<path>'` line
/// (no `!!!` prefix, real's own `f"File renamed to ..."`). Real emits
/// it through the scheduler (stdout + package log); portuale prints it
/// to stdout. `renamed` is the `checksum_failure_rename` result -- when
/// the rename itself failed the caller passes the original path (the
/// file is still there), so the line always names a real location.
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
