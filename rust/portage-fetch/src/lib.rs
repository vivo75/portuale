// Real SRC_URI flattening, real Manifest digest parsing, and real
// content-digest verification -- the non-network half of "actually
// fetch a package's real sources" (see portuale's own `fetch.rs` for
// the network half, the real `wget` subprocess invocation this crate
// deliberately has nothing to do with, kept out of this crate so its
// own logic stays 100% testable offline).
//
// SRC_URI grammar supported (PMS 3.1.6, real `mirror://` resolution
// included -- see `resolve_mirror_candidates`/`gentoo_mirror_fallback`
// below): a whitespace-
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
//   - `mirror://` resolution (`resolve_mirror_candidates`) only
//     consults real `profiles/thirdpartymirrors` (the ebuild's own
//     repo's copy, via `ebuild_phases::repo_root_for` at the call site
//     in `portuale/src/fetch.rs`) -- real `custommirrors` (an admin-
//     configured `/etc/portage/mirrors` file this pilot has no
//     `PORTAGE_CONFIGROOT` concept for at all) is never consulted.
//     Real portage's own `random.shuffle`s the resulting candidate
//     list (load-balancing across equally-valid mirrors) -- not
//     replicated here: this pilot's own "pinned, reproducible" test
//     philosophy already rules out non-determinism elsewhere, and
//     shuffling only affects *which* mirror is tried first, not
//     correctness (every candidate is still real-digest-verified after
//     fetching regardless).
//   - `gentoo_mirror_fallback` (real `async_mirror_url`'s own fallback
//     path, applied to *every* file real portage fetches, not just
//     `mirror://` ones) only ever assumes the real "flat" mirror
//     layout (`<mirror>/distfiles/<filename>`, real `FlatLayout.
//     get_path`) -- real portage negotiates a per-mirror `layout.conf`
//     live over the network (itself cached in `.mirror-cache.json`)
//     that can describe a hashed directory layout instead
//     (`filename-hash`/`content-hash`); this pilot never attempts that
//     live negotiation, which matches real `MirrorLayoutConfig.get_
//     best_supported_layout`'s own fallback whenever a mirror's
//     `layout.conf` can't be reached at all, and is what the real,
//     well-known `GENTOO_MIRRORS` entries (`distfiles.gentoo.org` and
//     its mirrors) actually use. Real portage also URL-quotes the
//     flat-layout filename for `ftp`/`http`/`https` mirrors -- not
//     replicated here (no URL-encoding dependency in this crate, and
//     real distfile filenames essentially never contain characters
//     that would need it).
//   - Real fetch ordering interleaves `GENTOO_MIRRORS` fallback,
//     `mirror://`-expanded candidates, and the literal `SRC_URI` URI
//     itself in a specific, somewhat subtle order (real `fetch.py`'s
//     own comment: "Prefer thirdpartymirrors over normal mirrors in
//     cases when the file does not yet exist on the normal mirrors").
//     This pilot instead tries the most-specific candidate first,
//     deterministically: `mirror://`-expanded (or the literal URI for
//     a non-`mirror://` token) first, `gentoo_mirror_fallback` last --
//     a real, deliberate deviation from real portage's own precise
//     ordering, not a bug; every candidate is still tried and real-
//     digest-verified regardless of order, so this only affects which
//     mirror is attempted first, never correctness.
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

/// Real `grabdict()` (`lib/portage/util/__init__.py`), narrowed to what
/// this pilot needs: real `profiles/thirdpartymirrors`'s own format --
/// one `<name> <url1> [<url2> ...]` entry per line. A whole line
/// starting with `#`, or any token from the first `#`-prefixed one
/// onward, is a comment (real `grabdict`'s own per-token truncation,
/// not just whole-line); a line left with fewer than 2 tokens after
/// that (a bare name with zero URLs, or a blank line) is skipped (real
/// `grabdict`'s own `empty=0` default). A missing file is an empty
/// map, not an error -- the same tolerance `parse_manifest` already
/// gives a missing `Manifest`.
pub fn parse_thirdpartymirrors(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };

    let mut out = HashMap::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line
            .split_whitespace()
            .take_while(|t| !t.starts_with('#'))
            .collect();
        if tokens.len() < 2 {
            continue;
        }
        out.insert(
            tokens[0].to_string(),
            tokens[1..].iter().map(|s| (*s).to_string()).collect(),
        );
    }
    Ok(out)
}

/// Real `mirror://<name>/<path>` resolution (real `fetch.py`'s own
/// thirdpartymirrors branch, `custommirrors` deliberately excluded --
/// see this module's own doc comment): `<name>` is looked up in
/// `thirdpartymirrors`, expanding to `<mirror_root>/<path>` for every
/// root under that name (real `locmirr.rstrip("/") + "/" + path`
/// string-built exactly, in the thirdpartymirrors file's own order --
/// real portage `random.shuffle`s this, deliberately not replicated
/// here, see this module's own doc comment). A `mirror://` token whose
/// name isn't known to `thirdpartymirrors` at all, or that's malformed
/// (`mirror://` with no further `/` at all), yields no candidates --
/// real portage's own `writemsg` warning, not a hard error; the caller
/// still fails loudly if this leaves a file with no working candidate
/// at all, the same real end result. A non-`mirror://` URI is returned
/// unchanged, as its own single candidate -- so every `SrcUriEntry.uri`
/// can be passed through this function uniformly, regardless of
/// whether it's actually a `mirror://` token.
pub fn resolve_mirror_candidates(
    uri: &str,
    thirdpartymirrors: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let Some(rest) = uri.strip_prefix("mirror://") else {
        return vec![uri.to_string()];
    };
    let Some(slash) = rest.find('/') else {
        return Vec::new();
    };
    let name = &rest[..slash];
    let path = &rest[slash + 1..];
    thirdpartymirrors
        .get(name)
        .map(|roots| {
            roots
                .iter()
                .map(|root| format!("{}/{}", root.trim_end_matches('/'), path))
                .collect()
        })
        .unwrap_or_default()
}

/// Real `async_mirror_url`'s own flat-layout fallback path, applied to
/// *every* file real portage fetches (not just `mirror://` ones) --
/// see this module's own doc comment for the real `layout.conf`
/// negotiation this pilot doesn't attempt, and why flat is the right
/// default anyway.
pub fn gentoo_mirror_fallback(filename: &str, gentoo_mirrors: &[String]) -> Vec<String> {
    gentoo_mirrors
        .iter()
        .map(|root| format!("{}/distfiles/{filename}", root.trim_end_matches('/')))
        .collect()
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

    #[test]
    fn parse_thirdpartymirrors_reads_a_real_grabdict_style_file() {
        let dir = tempdir();
        let path = dir.join("thirdpartymirrors");
        fs::write(
            &path,
            "# a comment line, skipped entirely\n\
             gentoo\thttps://distfiles.gentoo.org/distfiles https://gentoo.osuosl.org/distfiles\n\
             \n\
             gnu https://ftp.gnu.org/gnu/ # trailing comment truncates the rest\n\
             bare-name-no-urls\n",
        )
        .unwrap();
        let mirrors = parse_thirdpartymirrors(&path).unwrap();
        assert_eq!(
            mirrors.get("gentoo").unwrap(),
            &vec![
                "https://distfiles.gentoo.org/distfiles".to_string(),
                "https://gentoo.osuosl.org/distfiles".to_string(),
            ]
        );
        assert_eq!(
            mirrors.get("gnu").unwrap(),
            &vec!["https://ftp.gnu.org/gnu/".to_string()]
        );
        assert!(
            !mirrors.contains_key("bare-name-no-urls"),
            "a name with zero URLs must be skipped, matching real grabdict's own empty=0 default"
        );
    }

    #[test]
    fn parse_thirdpartymirrors_is_empty_for_a_missing_file() {
        let dir = tempdir();
        let mirrors = parse_thirdpartymirrors(&dir.join("does-not-exist")).unwrap();
        assert!(mirrors.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_expands_every_root_under_the_named_mirror() {
        let mut mirrors = HashMap::new();
        mirrors.insert(
            "gentoo".to_string(),
            vec![
                "https://distfiles.gentoo.org/distfiles".to_string(),
                "https://gentoo.osuosl.org/distfiles/".to_string(),
            ],
        );
        let candidates =
            resolve_mirror_candidates("mirror://gentoo/app-arch/foo-1.0.tar.gz", &mirrors);
        assert_eq!(
            candidates,
            vec![
                "https://distfiles.gentoo.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
                "https://gentoo.osuosl.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_mirror_candidates_is_empty_for_an_unknown_mirror_name() {
        let candidates = resolve_mirror_candidates("mirror://unknown/foo.tar.gz", &HashMap::new());
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_is_empty_for_a_malformed_mirror_uri() {
        let candidates = resolve_mirror_candidates("mirror://gentoo", &HashMap::new());
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_returns_a_non_mirror_uri_unchanged() {
        let candidates =
            resolve_mirror_candidates("https://example.com/foo.tar.gz", &HashMap::new());
        assert_eq!(
            candidates,
            vec!["https://example.com/foo.tar.gz".to_string()]
        );
    }

    #[test]
    fn gentoo_mirror_fallback_builds_the_real_flat_layout_path() {
        let mirrors = vec![
            "http://distfiles.gentoo.org".to_string(),
            "https://gentoo.osuosl.org/".to_string(),
        ];
        let candidates = gentoo_mirror_fallback("foo-1.0.tar.gz", &mirrors);
        assert_eq!(
            candidates,
            vec![
                "http://distfiles.gentoo.org/distfiles/foo-1.0.tar.gz".to_string(),
                "https://gentoo.osuosl.org/distfiles/foo-1.0.tar.gz".to_string(),
            ]
        );
    }
}
