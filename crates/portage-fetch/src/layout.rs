// Portage's distfiles mirror layouts (`lib/portage/package/ebuild/fetch.py`:
// `FlatLayout`, `FilenameHashLayout`, `ContentHashLayout`,
// `MirrorLayoutConfig`): the path a file lives at under a mirror's
// `distfiles/` directory, as described by that mirror's own
// `distfiles/layout.conf`. This matters for correctness: the Gentoo
// mirrors publish only the hashed layout path and 404 the flat one (see notes).
//
// Scope: the pure path math and `layout.conf` parsing. Fetching a
// mirror's `layout.conf` and the `.mirror-cache.json` cache live with
// the fetch loop (`portuale/src/fetch.rs`).
//
// Documented divergences (see notes for detail):
//   - `filename-hash` with `RMD160`/`SHA3_256`/`SHA3_512`/`WHIRLPOOL`
//     is unsupported here, so the next structure entry is used instead.
//   - `layout.conf` is parsed by a small `configparser` subset.

use std::collections::HashMap;

use blake2::Digest;

/// One Portage mirror layout (see `MirrorLayoutConfig::best_supported`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorLayout {
    /// Portage `FlatLayout`: `<file>`.
    Flat,
    /// Portage `FilenameHashLayout`: hex digest of the UTF-8 filename, cut
    /// into directory levels by `cutoffs` (bits, multiples of 4).
    FilenameHash { algo: String, cutoffs: Vec<i64> },
    /// Portage `ContentHashLayout`: the file's own Manifest digest, cut the
    /// same way; the last path element is the full digest.
    ContentHash { algo: String, cutoffs: Vec<i64> },
}

impl MirrorLayout {
    /// Portage `get_path(filename)`. `digests` is the file's Manifest
    /// digest map (algorithm name -> lowercase hex); only `ContentHash`
    /// reads it, returning `None` when the digest is missing.
    pub fn get_path(&self, filename: &str, digests: &HashMap<String, String>) -> Option<String> {
        match self {
            Self::Flat => Some(filename.to_string()),
            Self::FilenameHash { algo, cutoffs } => {
                let fnhash = hash_hex(algo, filename.as_bytes())?;
                Some(cut_path(&fnhash, cutoffs) + filename)
            }
            Self::ContentHash { algo, cutoffs } => {
                let digest = digests.get(&algo.to_uppercase())?;
                Some(cut_path(digest, cutoffs) + digest)
            }
        }
    }
}

// Portage's cutoff loop, with Python slice semantics for negative/zero cutoffs.
fn cut_path(hex: &str, cutoffs: &[i64]) -> String {
    let mut remaining: Vec<char> = hex.chars().collect();
    let mut ret = String::new();
    for &c in cutoffs {
        let split = python_slice_index(c.div_euclid(4), remaining.len());
        ret.extend(&remaining[..split]);
        ret.push('/');
        remaining = remaining[split..].to_vec();
    }
    ret
}

/// Python's `s[:i]` / `s[i:]` boundary for an integer `i` on a sequence
/// of length `len`.
fn python_slice_index(i: i64, len: usize) -> usize {
    let len_i = len as i64;
    let idx = if i < 0 {
        (len_i + i).max(0)
    } else {
        i.min(len_i)
    };
    idx as usize
}

/// Portage `checksum_str(data, hashname)` for the algorithms this crate
/// carries; `None` for any other name (see notes for the full list).
fn hash_hex(algo: &str, data: &[u8]) -> Option<String> {
    let bytes: Vec<u8> = match algo {
        "BLAKE2B" => blake2::Blake2b512::digest(data).to_vec(),
        "BLAKE2S" => blake2::Blake2s256::digest(data).to_vec(),
        "MD5" => md5::Md5::digest(data).to_vec(),
        "SHA1" => sha1::Sha1::digest(data).to_vec(),
        "SHA256" => sha2::Sha256::digest(data).to_vec(),
        "SHA512" => sha2::Sha512::digest(data).to_vec(),
        _ => return None,
    };
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Portage `get_valid_checksum_keys()` on a stock install, minus its pseudo-key `size`.
const PORTAGE_CHECKSUM_KEYS: &[&str] = &[
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

fn supports_algo(algo: &str) -> bool {
    hash_hex(algo, b"").is_some()
}

/// Portage `FilenameHashLayout.verify_args`: exactly `(name, algo,
/// cutoffs)` with a known (case-sensitive) hash and `:`-separated cutoffs
/// that are integers divisible by 4.
fn cutoffs_valid(cutoffs: &str) -> Option<Vec<i64>> {
    cutoffs
        .split(':')
        .map(|c| c.parse::<i64>().ok().filter(|c| c % 4 == 0))
        .collect()
}

/// Portage `MirrorLayoutConfig`: the ordered `[structure]` entries of a
/// mirror's `layout.conf`, each already whitespace-split.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorLayoutConfig {
    pub structure: Vec<Vec<String>>,
}

impl MirrorLayoutConfig {
    /// Portage `read_from_file`: `[structure]` keys `0`, `1`, ... in
    /// order until the first missing one, each value `.split()`.
    ///
    /// The `configparser` subset: blank lines and `#`/`;` comments are
    /// skipped; `[name]` opens a section; `key = value` / `key: value`
    /// sets an option (key stripped and lower-cased); an indented line
    /// continues the previous value. Malformed input is an error (see notes).
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current: Option<String> = None;
        let mut last_option: Option<String> = None;
        for (lineno, raw) in text.lines().enumerate() {
            let stripped = raw.trim();
            if stripped.is_empty() || stripped.starts_with('#') || stripped.starts_with(';') {
                continue;
            }
            let indented = raw.starts_with([' ', '\t']);
            if indented && let (Some(section), Some(option)) = (&current, &last_option) {
                let value = sections
                    .get_mut(section)
                    .and_then(|s| s.get_mut(option))
                    .expect("continued option exists");
                value.push('\n');
                value.push_str(stripped);
                continue;
            }
            if let Some(name) = stripped
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .filter(|name| !name.is_empty())
            {
                if sections.contains_key(name) {
                    return Err(format!("line {}: duplicate section {name:?}", lineno + 1));
                }
                sections.insert(name.to_string(), HashMap::new());
                current = Some(name.to_string());
                last_option = None;
                continue;
            }
            let Some(section) = &current else {
                return Err(format!("line {}: no section header", lineno + 1));
            };
            let Some(delim) = stripped.find(['=', ':']) else {
                return Err(format!("line {}: not a key = value line", lineno + 1));
            };
            let key = stripped[..delim].trim().to_lowercase();
            let value = stripped[delim + 1..].trim().to_string();
            let options = sections.get_mut(section).expect("current section exists");
            if options.contains_key(&key) {
                return Err(format!("line {}: duplicate option {key:?}", lineno + 1));
            }
            options.insert(key.clone(), value);
            last_option = Some(key);
        }
        let mut structure = Vec::new();
        if let Some(options) = sections.get("structure") {
            for i in 0.. {
                let Some(value) = options.get(&i.to_string()) else {
                    break;
                };
                structure.push(value.split_whitespace().map(str::to_string).collect());
            }
        }
        Ok(Self { structure })
    }

    fn layout_for(
        val: &[String],
        digests: Option<&HashMap<String, String>>,
    ) -> Option<MirrorLayout> {
        match val.first().map(String::as_str)? {
            "flat" if val.len() == 1 => Some(MirrorLayout::Flat),
            "filename-hash" if val.len() == 3 && supports_algo(&val[1]) => {
                Some(MirrorLayout::FilenameHash {
                    algo: val[1].clone(),
                    cutoffs: cutoffs_valid(&val[2])?,
                })
            }
            "content-hash" if val.len() == 3 => {
                // Portage `ContentHashLayout.verify_args`: the upper-cased
                // name must be a known hash AND (filename-less aside) one
                // the file has a digest for; the as-written name must also
                // pass the case-sensitive known-hash check. No hashing
                // happens here, so every Portage hash name counts.
                let algo = val[1].to_uppercase();
                let supported = PORTAGE_CHECKSUM_KEYS.contains(&val[1].as_str())
                    && match digests {
                        Some(digests) => digests.contains_key(&algo),
                        None => true,
                    };
                if !supported {
                    return None;
                }
                Some(MirrorLayout::ContentHash {
                    algo: val[1].clone(),
                    cutoffs: cutoffs_valid(&val[2])?,
                })
            }
            _ => None,
        }
    }

    /// Portage `get_best_supported_layout(filename)`: the first valid
    /// structure entry, else flat.
    pub fn best_supported(&self, digests: Option<&HashMap<String, String>>) -> MirrorLayout {
        self.structure
            .iter()
            .find_map(|val| Self::layout_for(val, digests))
            .unwrap_or(MirrorLayout::Flat)
    }
}
