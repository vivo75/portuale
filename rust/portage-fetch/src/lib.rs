// Real SRC_URI flattening, real Manifest digest parsing, and real
// content-digest verification -- the non-network half of "actually
// fetch a package's real sources" (see multicall's own `fetch.rs` for
// the network half, the real `wget` subprocess invocation this crate
// deliberately has nothing to do with, kept out of this crate so its
// own logic stays 100% testable offline).
//
// SRC_URI grammar supported (PMS 3.1.6, minus mirror:// resolution --
// see this module's own "KNOWN, DOCUMENTED GAPS" below): a whitespace-
// separated list of plain URIs, each optionally followed by `-> name`
// (real "arrow" rename -- PMS's own local-filename override), grouped
// under `flag? ( ... )` / `!flag? ( ... )` USE-conditional groups,
// recursively nested. Real SRC_URI explicitly does NOT support `||`
// (any-of) groups the way DEPEND-family strings do (PMS 8.2.6.5: "any-
// of dependencies (`||`) are not allowed" there) -- this parser doesn't
// implement `||` at all, matching that real grammar restriction, not a
// simplification.
//
// Real Manifest2 format (`lib/portage/manifest.py`'s own `_manifest_re`,
// confirmed by reading it): `DIST <filename> <size> <HASH1> <hex1>
// [<HASH2> <hex2> ...]`, one real, unmodified `blake2b`-hashers
// hex digest, plus a `sha512` one -- `MANIFEST2_HASH_DEFAULTS =
// frozenset(("BLAKE2B", "SHA512"))` (`lib/portage/const.py`) -- verified
// here via the real, standard BLAKE2b-512 and SHA-512 algorithms
// (`blake2`/`sha2` crates), not reimplemented from scratch.
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching this whole pilot's own
// "narrow v1, document the cut" pattern):
//   - No `mirror://` URI resolution (real `thirdpartymirrors`/
//     `GENTOO_MIRRORS` config) -- a `mirror://` token is treated as an
//     ordinary (unfetchable) URI, which will simply fail to fetch. Real
//     multi-mirror fallback/retry is a separately-scoped follow-up.
//   - Only `BLAKE2B`/`SHA512` are verified (real `MANIFEST2_HASH_DEFAULTS`
//     exactly) -- any other hash name appearing in a Manifest entry is
//     silently ignored, the same "real, standard hash, not reimplemented
//     from scratch" reasoning `ebuild_merge.rs`'s own real MD5 CONTENTS
//     digest already established for `obj` entries.
//   - No AUX/MISC/EBUILD Manifest line support (`parse_manifest` only
//     reads `DIST` lines) -- this pilot never needs to verify anything
//     else a Manifest records.

use std::collections::HashMap;
use std::path::Path;

/// One real `Manifest` `DIST` line's own digest record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistfileDigests {
    pub size: u64,
    /// Hash algorithm name (`"BLAKE2B"`/`"SHA512"`, real Manifest2
    /// casing) -> lowercase hex digest, exactly as the real `Manifest`
    /// file records it.
    pub hashes: HashMap<String, String>,
}

/// Real `Manifest.parseManifest2`, narrowed to `DIST` lines only (see
/// the module doc comment). A missing `Manifest` file is an empty map,
/// not an error -- same "nothing recorded yet" tolerance
/// `portage_repo::list_candidates` already gives a missing repo
/// directory.
pub fn parse_manifest(manifest_path: &Path) -> Result<HashMap<String, DistfileDigests>, String> {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(format!("{}: {e}", manifest_path.display())),
    };

    let mut out = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("DIST") {
            continue;
        }
        let Some(name) = parts.next() else { continue };
        let Some(size) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let rest: Vec<&str> = parts.collect();
        let mut hashes = HashMap::new();
        let mut i = 0;
        while i + 1 < rest.len() {
            hashes.insert(rest[i].to_string(), rest[i + 1].to_string());
            i += 2;
        }
        out.insert(name.to_string(), DistfileDigests { size, hashes });
    }
    Ok(out)
}

/// One flattened `SRC_URI` entry: the real remote URI, and the real
/// local filename it should be saved as (either the real "arrow"
/// rename, or the URI's own basename -- PMS's own default when no `->`
/// is given).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrcUriEntry {
    pub uri: String,
    pub filename: String,
}

fn basename(uri: &str) -> String {
    uri.rsplit('/').next().unwrap_or(uri).to_string()
}

/// Recursive-descent parser for the grammar described in the module doc
/// comment. `active(negated, flag)` decides whether a `flag?`/`!flag?`
/// group's own contents should be collected -- callers pass a real USE
/// membership check for `SRC_URI` itself (this pilot's own always-empty
/// USE set, same v1 cut every other real-execution slice already has),
/// or an always-true closure to compute the real `AA` variable (every
/// file `SRC_URI` could ever reference, regardless of USE -- PMS's own
/// definition of `AA`).
fn parse_list(
    tokens: &[&str],
    pos: &mut usize,
    active: &impl Fn(bool, &str) -> bool,
) -> Result<Vec<SrcUriEntry>, String> {
    let mut out = Vec::new();
    while *pos < tokens.len() && tokens[*pos] != ")" {
        let tok = tokens[*pos];
        if let Some(flag) = tok.strip_suffix('?') {
            *pos += 1;
            if tokens.get(*pos) != Some(&"(") {
                return Err(format!("SRC_URI: expected \"(\" after {tok:?}"));
            }
            *pos += 1;
            let (negated, flag) = match flag.strip_prefix('!') {
                Some(f) => (true, f),
                None => (false, flag),
            };
            let inner = parse_list(tokens, pos, active)?;
            if tokens.get(*pos) != Some(&")") {
                return Err(format!("SRC_URI: unterminated {tok:?} group"));
            }
            *pos += 1;
            if active(negated, flag) {
                out.extend(inner);
            }
        } else if tok == "(" || tok == ")" {
            return Err(format!("SRC_URI: unexpected {tok:?}"));
        } else {
            *pos += 1;
            let filename = if tokens.get(*pos) == Some(&"->") {
                *pos += 1;
                let Some(name) = tokens.get(*pos) else {
                    return Err("SRC_URI: missing filename after \"->\"".to_string());
                };
                *pos += 1;
                name.to_string()
            } else {
                basename(tok)
            };
            out.push(SrcUriEntry {
                uri: tok.to_string(),
                filename,
            });
        }
    }
    Ok(out)
}

/// Flattens a real `SRC_URI` string into the real, ordered list of
/// `(uri, filename)` pairs it names -- see the module doc comment for
/// the grammar and `active`'s own meaning.
pub fn flatten_src_uri(
    src_uri: &str,
    active: impl Fn(bool, &str) -> bool,
) -> Result<Vec<SrcUriEntry>, String> {
    let tokens: Vec<&str> = src_uri.split_whitespace().collect();
    let mut pos = 0;
    let entries = parse_list(&tokens, &mut pos, &active)?;
    if pos != tokens.len() {
        return Err(format!(
            "SRC_URI: unexpected token {:?}",
            tokens.get(pos).copied().unwrap_or("")
        ));
    }
    Ok(entries)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Real digest verification: file size (a cheap, real `_check_distfile`
/// pre-check before ever hashing) plus every `BLAKE2B`/`SHA512` entry
/// `digests` carries -- see the module doc comment for why only those
/// two. Any other hash name present is silently skipped (not
/// mismatched, not verified). An empty `digests.hashes` (a `Manifest`
/// entry with no recognized hash at all) still passes once the size
/// matches, the same "size alone is still something" tolerance real
/// `_check_distfile` gives.
pub fn verify_digests(path: &Path, digests: &DistfileDigests) -> Result<(), String> {
    // `blake2`/`sha2` both re-export the same underlying `digest::Digest`
    // trait under their own name -- importing it once is enough for
    // both `Blake2b512::digest`/`Sha512::digest` below.
    use blake2::Digest as _;

    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 != digests.size {
        return Err(format!(
            "{}: size mismatch (expected {}, got {})",
            path.display(),
            digests.size,
            bytes.len()
        ));
    }
    for (algo, expected) in &digests.hashes {
        let actual = match algo.as_str() {
            "BLAKE2B" => to_hex(&blake2::Blake2b512::digest(&bytes)),
            "SHA512" => to_hex(&sha2::Sha512::digest(&bytes)),
            _ => continue,
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "{}: {algo} mismatch (expected {expected}, got {actual})",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portage_fetch_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_manifest_reads_a_real_dist_line_with_both_hashes() {
        let dir = tempdir();
        let manifest = dir.join("Manifest");
        fs::write(
            &manifest,
            "DIST fuse-3.18.2.tar.gz 4933779 BLAKE2B aaaa SHA512 bbbb\n\
             EBUILD fuse-3.18.2.ebuild 123 BLAKE2B cccc SHA512 dddd\n",
        )
        .unwrap();
        let parsed = parse_manifest(&manifest).unwrap();
        assert_eq!(parsed.len(), 1, "EBUILD lines must not be parsed as DIST");
        let entry = &parsed["fuse-3.18.2.tar.gz"];
        assert_eq!(entry.size, 4933779);
        assert_eq!(entry.hashes["BLAKE2B"], "aaaa");
        assert_eq!(entry.hashes["SHA512"], "bbbb");
    }

    #[test]
    fn parse_manifest_is_empty_for_a_missing_file() {
        let dir = tempdir();
        let parsed = parse_manifest(&dir.join("Manifest")).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn flatten_src_uri_handles_plain_uris_with_no_conditionals() {
        let entries = flatten_src_uri("https://example.com/a-1.0.tar.gz", |_, _| true).unwrap();
        assert_eq!(
            entries,
            vec![SrcUriEntry {
                uri: "https://example.com/a-1.0.tar.gz".to_string(),
                filename: "a-1.0.tar.gz".to_string(),
            }]
        );
    }

    #[test]
    fn flatten_src_uri_honors_the_arrow_rename() {
        let entries =
            flatten_src_uri("https://example.com/dl?id=1 -> a-1.0.tar.gz", |_, _| true).unwrap();
        assert_eq!(entries[0].filename, "a-1.0.tar.gz");
        assert_eq!(entries[0].uri, "https://example.com/dl?id=1");
    }

    #[test]
    fn flatten_src_uri_includes_a_positive_conditional_only_when_active() {
        let src = "unconditional-1.0.tar.gz test? ( test-only-1.0.tar.gz )";
        let with_test = flatten_src_uri(src, |negated, flag| !negated && flag == "test").unwrap();
        assert_eq!(with_test.len(), 2);

        let without_test = flatten_src_uri(src, |_, _| false).unwrap();
        assert_eq!(without_test.len(), 1);
        assert_eq!(without_test[0].filename, "unconditional-1.0.tar.gz");
    }

    #[test]
    fn flatten_src_uri_negated_conditional_is_active_when_the_flag_is_unset() {
        // This pilot's own always-empty USE set (see the module doc
        // comment) means every `!flag?` group is always active -- the
        // real, common way an optional dependency's SRC_URI still gets
        // fetched by default. `is_set` here stands in for that
        // always-empty USE set: every flag is unset.
        let is_set = |_flag: &str| false;
        let active = |negated: bool, flag: &str| {
            if negated {
                !is_set(flag)
            } else {
                is_set(flag)
            }
        };
        let entries = flatten_src_uri("!test? ( a-1.0.tar.gz )", active).unwrap();
        assert_eq!(entries.len(), 1);

        let entries = flatten_src_uri("test? ( a-1.0.tar.gz )", active).unwrap();
        assert_eq!(
            entries.len(),
            0,
            "a positive conditional on an unset flag must not fire"
        );
    }

    #[test]
    fn flatten_src_uri_supports_nested_groups() {
        let src = "outer? ( a-1.0.tar.gz inner? ( b-1.0.tar.gz ) )";
        let all_active = flatten_src_uri(src, |_, _| true).unwrap();
        assert_eq!(all_active.len(), 2);

        let outer_only = flatten_src_uri(src, |_, flag| flag == "outer").unwrap();
        assert_eq!(outer_only.len(), 1);
        assert_eq!(outer_only[0].filename, "a-1.0.tar.gz");
    }

    #[test]
    fn flatten_src_uri_rejects_a_double_bar_any_of_group() {
        // Real SRC_URI grammar has no `||` at all (PMS 8.2.6.5) -- "||"
        // is just an ordinary, unfetchable URI token here, not a syntax
        // error, matching how real portage's own use_reduce(is_src_uri=
        // True) would also just leave a bare "||" token alone rather
        // than parsing it as a group opener the way DEPEND strings do.
        let entries = flatten_src_uri("|| ( a-1.0.tar.gz )", |_, _| true);
        assert!(
            entries.is_err(),
            "\"(\" with no preceding \"flag?\" token is a real syntax error"
        );
    }

    #[test]
    fn flatten_src_uri_reports_an_unterminated_group() {
        assert!(flatten_src_uri("test? ( a-1.0.tar.gz", |_, _| true).is_err());
    }

    #[test]
    fn flatten_src_uri_reports_a_dangling_arrow() {
        assert!(flatten_src_uri("https://example.com/a ->", |_, _| true).is_err());
    }

    #[test]
    fn verify_digests_accepts_a_real_matching_file() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        // Real, independently-known BLAKE2b-512 and SHA-512 digests of
        // the literal bytes "hello world" (not invented -- these are
        // the real, standard test-vector values for that exact input).
        let mut hashes = HashMap::new();
        hashes.insert(
            "BLAKE2B".to_string(),
            "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbcc05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0".to_string(),
        );
        hashes.insert(
            "SHA512".to_string(),
            "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f".to_string(),
        );
        let digests = DistfileDigests { size: 11, hashes };
        assert!(verify_digests(&path, &digests).is_ok());
    }

    #[test]
    fn verify_digests_rejects_a_size_mismatch_without_hashing() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        let digests = DistfileDigests {
            size: 999,
            hashes: HashMap::new(),
        };
        let err = verify_digests(&path, &digests).unwrap_err();
        assert!(err.contains("size mismatch"), "{err}");
    }

    #[test]
    fn verify_digests_rejects_a_hash_mismatch() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        let mut hashes = HashMap::new();
        hashes.insert("SHA512".to_string(), "0".repeat(128));
        let digests = DistfileDigests { size: 11, hashes };
        let err = verify_digests(&path, &digests).unwrap_err();
        assert!(err.contains("SHA512 mismatch"), "{err}");
    }

    #[test]
    fn verify_digests_ignores_an_unrecognized_hash_name() {
        let dir = tempdir();
        let path = dir.join("hello.txt");
        fs::write(&path, b"hello world").unwrap();
        let mut hashes = HashMap::new();
        hashes.insert("MD5".to_string(), "not-even-hex".to_string());
        let digests = DistfileDigests { size: 11, hashes };
        assert!(verify_digests(&path, &digests).is_ok());
    }
}
