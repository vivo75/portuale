// Real distfiles mirror layouts (`lib/portage/package/ebuild/fetch.py`:
// `FlatLayout`, `FilenameHashLayout`, `ContentHashLayout`,
// `MirrorLayoutConfig`): the path a file lives at under a mirror's
// `distfiles/` directory, as described by that mirror's own
// `distfiles/layout.conf`.
//
// This matters for correctness, not just cosmetics: the real Gentoo
// mirrors (`distfiles.gentoo.org` and the mirrors real portage has
// cached in `.mirror-cache.json`) publish only `0=filename-hash BLAKE2B
// 8` -- the flat `<mirror>/distfiles/<file>` path 404s there, the
// hashed `<mirror>/distfiles/<2 hex>/<file>` one is served.
//
// Scope: the pure path math and `layout.conf` parsing. Fetching a
// mirror's `layout.conf` and the `.mirror-cache.json` cache live with
// the fetch loop (`portuale/src/fetch.rs`).
//
// Documented divergences:
//   - `filename-hash` hashes with `BLAKE2B`, `BLAKE2S`, `MD5`, `SHA1`,
//     `SHA256`, `SHA512`. Real also hashes with `RMD160`/`SHA3_256`/
//     `SHA3_512`/`WHIRLPOOL`; a `filename-hash` structure naming one of
//     those is treated as unsupported here, so the next structure entry
//     (ultimately flat) is used instead. (`content-hash` needs no
//     hashing, so it accepts every real name.)
//   - `layout.conf` is parsed by a small `configparser` subset (see
//     `MirrorLayoutConfig::parse`), not the full INI grammar.

use std::collections::HashMap;

use blake2::Digest;

/// One real mirror layout (`get_best_supported_layout`'s return value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorLayout {
    /// Real `FlatLayout`: `<file>`.
    Flat,
    /// Real `FilenameHashLayout`: hex digest of the UTF-8 filename, cut
    /// into directory levels by `cutoffs` (bits, multiples of 4).
    FilenameHash { algo: String, cutoffs: Vec<i64> },
    /// Real `ContentHashLayout`: the file's own Manifest digest, cut the
    /// same way; the last path element is the full digest.
    ContentHash { algo: String, cutoffs: Vec<i64> },
}

impl MirrorLayout {
    /// Real `get_path(filename)`. `digests` is the file's Manifest
    /// digest map (algorithm name -> lowercase hex); only
    /// `ContentHash` reads it, and returns `None` when the digest is
    /// missing (unreachable after `MirrorLayoutConfig::best_supported`,
    /// which only picks a content-hash layout whose digest exists).
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

/// The `for c in self.cutoffs: ret += fnhash[:c//4] + "/"; fnhash =
/// fnhash[c//4:]` loop, with Python's slice semantics (a negative or
/// zero cutoff is accepted by real `verify_args` and sliced as Python
/// would).
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

/// Real `checksum_str(data, hashname)` for the algorithms this crate
/// carries (see the module doc); `None` for any other name.
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

/// Real `get_valid_checksum_keys()` on a stock install (portage
/// 3.0.82.2), minus its pseudo-key `size`.
const REAL_CHECKSUM_KEYS: &[&str] = &[
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

/// Real `FilenameHashLayout.verify_args`: exactly `(name, algo,
/// cutoffs)`, `algo` a (case-sensitive) known hash, and every
/// `:`-separated cutoff an integer divisible by 4.
fn cutoffs_valid(cutoffs: &str) -> Option<Vec<i64>> {
    cutoffs
        .split(':')
        .map(|c| c.parse::<i64>().ok().filter(|c| c % 4 == 0))
        .collect()
}

/// Real `MirrorLayoutConfig`: the ordered `[structure]` entries of a
/// mirror's `layout.conf`, each already whitespace-split.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorLayoutConfig {
    pub structure: Vec<Vec<String>>,
}

impl MirrorLayoutConfig {
    /// Real `read_from_file`: `[structure]` keys `0`, `1`, ... read in
    /// order until the first missing one, each value `.split()`.
    ///
    /// The `configparser` subset: blank lines and lines whose first
    /// non-blank character is `#` or `;` are skipped; `[name]` opens a
    /// section; `key = value` / `key: value` sets an option (key
    /// stripped and lower-cased, value stripped); an indented line
    /// after an option continues its value. Errors real
    /// `configparser` raises (and real `async_mirror_url` turns into
    /// "use flat, don't cache") are errors here too: an option before
    /// any section header, a duplicate section or option, and a
    /// non-blank line that is neither a header nor `key=value`.
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

    /// Real `validate_structure(val, filename)`: with digests given,
    /// a content-hash layout is only valid for an algorithm the file
    /// actually has a digest for.
    pub fn validate_structure(val: &[String], digests: Option<&HashMap<String, String>>) -> bool {
        Self::layout_for(val, digests).is_some()
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
                // Real `ContentHashLayout.verify_args`: the upper-cased
                // name must be a digest the file has (or, filename-less,
                // a known hash) AND the as-written name must pass
                // `FilenameHashLayout.verify_args`'s case-sensitive
                // known-hash check -- so a lower-case name is never valid.
                // No hashing happens here, so every real hash name counts.
                let algo = val[1].to_uppercase();
                let supported = REAL_CHECKSUM_KEYS.contains(&val[1].as_str())
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

    /// Real `get_best_supported_layout(filename)`: the first valid
    /// structure entry, else flat.
    pub fn best_supported(&self, digests: Option<&HashMap<String, String>>) -> MirrorLayout {
        self.structure
            .iter()
            .find_map(|val| Self::layout_for(val, digests))
            .unwrap_or(MirrorLayout::Flat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn fhash(algo: &str, cutoffs: &str) -> MirrorLayout {
        MirrorLayoutConfig {
            structure: vec![val(&format!("filename-hash {algo} {cutoffs}"))],
        }
        .best_supported(None)
    }

    /// Expected paths are real portage's own output
    /// (`FilenameHashLayout(algo, cutoffs).get_path(name)`, portage
    /// 3.0.82.2), not derived from reading the code.
    #[test]
    fn filename_hash_paths_match_real_portage() {
        let none = HashMap::new();
        for (algo, cutoffs, name, expected) in [
            ("BLAKE2B", "8", "which-2.23.tar.gz", "80/which-2.23.tar.gz"),
            (
                "BLAKE2B",
                "8:8",
                "which-2.23.tar.gz",
                "80/0b/which-2.23.tar.gz",
            ),
            ("SHA512", "16", "foo-1.0.tar.gz", "e7e2/foo-1.0.tar.gz"),
            ("SHA256", "4:12", "a b+c.tar.xz", "7/42d/a b+c.tar.xz"),
            ("BLAKE2B", "8", "über-1.0.tgz", "2e/über-1.0.tgz"),
            ("MD5", "8", "x", "9d/x"),
            ("SHA1", "8", "x", "11/x"),
            ("BLAKE2S", "8", "x", "ec/x"),
            ("BLAKE2B", "0", "which-2.23.tar.gz", "/which-2.23.tar.gz"),
            (
                "BLAKE2B",
                "8:0",
                "which-2.23.tar.gz",
                "80//which-2.23.tar.gz",
            ),
        ] {
            assert_eq!(
                fhash(algo, cutoffs).get_path(name, &none).as_deref(),
                Some(expected),
                "{algo} {cutoffs} {name}"
            );
        }
        // Negative cutoffs slice like Python (`fnhash[:-1]`).
        let neg = fhash("BLAKE2B", "-8:4")
            .get_path("which-2.23.tar.gz", &none)
            .unwrap();
        assert_eq!(
            neg,
            "800bac16a98c5ba03da25cef5996250a4541cf5796f800c511fecaa89bcf6dee\
             53036e6e44a0a074db50978380d9c19f11c997c5af809b7ad890838ef0d360/a/which-2.23.tar.gz"
        );
    }

    /// Real `MirrorLayoutConfig.validate_structure` verdicts (portage
    /// 3.0.82.2), filename-less form.
    #[test]
    fn validate_structure_matches_real_portage() {
        for (entry, expected) in [
            ("flat", true),
            ("flat x", false),
            ("filename-hash BLAKE2B 8", true),
            ("filename-hash blake2b 8", false),
            ("filename-hash BLAKE2B 6", false),
            ("filename-hash BLAKE2B 8:x", false),
            ("filename-hash BLAKE2B x:6", false),
            ("filename-hash BLAKE2B -4", true),
            ("filename-hash BLAKE2B 0", true),
            ("content-hash SHA512 8", true),
            ("content-hash blake2b 8", false),
            ("content-hash WHIRLPOOL 8", true),
            ("bogus", false),
            ("", false),
        ] {
            assert_eq!(
                MirrorLayoutConfig::validate_structure(&val(entry), None),
                expected,
                "{entry:?}"
            );
        }
        // With the file's digests (`filename.digests = {"SHA512": ...}`).
        let sha512: HashMap<String, String> = [("SHA512".to_string(), "x".to_string())].into();
        for (entry, expected) in [
            ("content-hash SHA512 8", true),
            ("content-hash BLAKE2B 8", false),
            ("content-hash sha512 8", false),
            ("content-hash WHIRLPOOL 8", false),
        ] {
            assert_eq!(
                MirrorLayoutConfig::validate_structure(&val(entry), Some(&sha512)),
                expected,
                "{entry:?} with digests"
            );
        }
        // Empty cutoffs: `int("")` fails in real too.
        assert!(!MirrorLayoutConfig::validate_structure(
            &["filename-hash".into(), "BLAKE2B".into(), String::new()],
            None
        ));
    }

    #[test]
    fn content_hash_requires_the_files_own_digest() {
        let config = MirrorLayoutConfig {
            structure: vec![
                val("content-hash SHA512 8:8"),
                val("filename-hash BLAKE2B 8"),
            ],
        };
        let digests: HashMap<String, String> =
            [("SHA512".to_string(), "abcdef0123".to_string())].into();
        let layout = config.best_supported(Some(&digests));
        assert_eq!(
            layout.get_path("f.tar.gz", &digests).as_deref(),
            Some("ab/cd/abcdef0123")
        );
        // Without a SHA512 digest the next entry wins.
        let blake_only: HashMap<String, String> =
            [("BLAKE2B".to_string(), "00".to_string())].into();
        assert_eq!(
            config.best_supported(Some(&blake_only)),
            MirrorLayout::FilenameHash {
                algo: "BLAKE2B".into(),
                cutoffs: vec![8]
            }
        );
    }

    #[test]
    fn best_supported_skips_invalid_entries_and_falls_back_to_flat() {
        let config = MirrorLayoutConfig {
            structure: vec![
                val("filename-hash WHIRLPOOL 8"),
                val("filename-hash BLAKE2B 6"),
            ],
        };
        assert_eq!(config.best_supported(None), MirrorLayout::Flat);
        assert_eq!(
            MirrorLayoutConfig::default().best_supported(None),
            MirrorLayout::Flat
        );
    }

    #[test]
    fn parse_reads_the_real_gentoo_layout_conf() {
        let config = MirrorLayoutConfig::parse("[structure]\n0=filename-hash BLAKE2B 8\n").unwrap();
        assert_eq!(config.structure, vec![val("filename-hash BLAKE2B 8")]);
    }

    #[test]
    fn parse_reads_keys_in_order_until_the_first_gap() {
        let text = "# mirror layout\n[structure]\n1 = flat\n0 : filename-hash BLAKE2B 8:8\n\
                    3=flat\n\n[other]\nx=y\n";
        let config = MirrorLayoutConfig::parse(text).unwrap();
        assert_eq!(
            config.structure,
            vec![val("filename-hash BLAKE2B 8:8"), val("flat")]
        );
    }

    #[test]
    fn parse_joins_continuation_lines_before_splitting() {
        let config =
            MirrorLayoutConfig::parse("[structure]\n0=filename-hash\n  BLAKE2B 8\n").unwrap();
        assert_eq!(config.structure, vec![val("filename-hash BLAKE2B 8")]);
    }

    #[test]
    fn parse_missing_structure_section_is_empty_not_an_error() {
        assert_eq!(
            MirrorLayoutConfig::parse("[other]\n0=flat\n").unwrap(),
            MirrorLayoutConfig::default()
        );
    }

    #[test]
    fn parse_rejects_what_configparser_rejects() {
        for text in [
            "0=flat\n",
            "[structure]\n[structure]\n",
            "[structure]\n0=flat\n0=flat\n",
            "[structure]\njust words\n",
        ] {
            assert!(MirrorLayoutConfig::parse(text).is_err(), "{text:?}");
        }
    }
}
