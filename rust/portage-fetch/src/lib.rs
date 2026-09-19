// Real SRC_URI flattening, real Manifest digest parsing, and real
// content-digest verification -- the non-network half of "actually
// fetch a package's real sources" (see portuale's own `fetch.rs` for
// the network half, the real `wget` subprocess invocation this crate
// deliberately has nothing to do with, kept out of this crate so its
// own logic stays 100% testable offline).
//
// SRC_URI grammar supported (PMS 3.1.6, real `mirror://` resolution
// included -- see `resolve_mirror_candidates` below, and `layout` for
// how a mirror root becomes a file URL): a whitespace-
// separated list of plain URIs, each optionally followed by `-> name`
// (real "arrow" rename -- PMS's own local-filename override), grouped
// under `flag? ( ... )` / `!flag? ( ... )` USE-conditional groups,
// recursively nested. A URI token may also carry a real `mirror+` or
// `fetch+` prefix (real `fetch.py:1103-1106` -- a portage extension, not
// PMS): the prefix is stripped from the URI and recorded on the entry as
// `override_mirror` / `override_fetch` (`mirror+` sets both). See
// `SrcUriEntry`'s own doc comment and `portuale/src/fetch.rs` for what
// each override actually relaxes.
// Real SRC_URI explicitly does NOT support `||`
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
// KNOWN, DOCUMENTED GAPS (v1 scope, matching portuale's own
// "narrow v1, document the cut" pattern):
//   - `mirror://` resolution (`resolve_mirror_candidates`) consults both
//     real `profiles/thirdpartymirrors` (the ebuild's own repo's copy,
//     via `ebuild_phases::repo_root_for` at the call site in
//     `portuale/src/fetch.rs`) and real `custommirrors` (an admin-
//     configured `${PORTAGE_CONFIGROOT}/etc/portage/mirrors` file,
//     "user-defined mirrors first" -- real `grabdict(...,
//     recursive=1)`'s own directory-form (`/etc/portage/mirrors/` as a
//     directory of drop-in files) isn't reproduced, only the plain-file
//     form, the same narrowing `profiles/thirdpartymirrors` itself
//     already has). Real `custommirrors["local"]`'s own separate
//     filesystem-path/local-network fast-path lookup (real
//     `fetch.py:1017-1029`) isn't reproduced either -- see
//     `resolve_mirror_candidates`'s own doc comment for why a real
//     `mirror://local/...` token still resolves correctly regardless.
//     Real portage's own `random.shuffle`s the `thirdpartymirrors` half
//     of the resulting candidate list (load-balancing across equally-
//     valid mirrors) -- not replicated here: portuale's own "pinned,
//     reproducible" test philosophy already rules out non-determinism
//     elsewhere, and shuffling only affects *which* mirror is tried
//     first, not correctness (every candidate is still real-digest-
//     verified after fetching regardless).
//   - Mirror roots (`custommirrors["local"]` and public `GENTOO_MIRRORS`)
//     become file URLs through the mirror's own `layout.conf` -- real
//     `async_mirror_url`: `layout` (the `flat`/`filename-hash`/
//     `content-hash` path math) and `mirror_cache` (real
//     `.mirror-cache.json`, URL quoting) here, the download and caching
//     in `portuale/src/fetch.rs::mirror_url`. Flat is NOT a safe default:
//     `distfiles.gentoo.org` publishes only `0=filename-hash BLAKE2B 8`
//     and 404s the flat path (checked 2026-09-14).
//   - Real fetch ordering is `assemble_candidates`' own shape
//     (`portuale/src/fetch.rs`, real `fetch.py:1099-1192`), one list per
//     distfile however many `SRC_URI` entries name it: local mirrors,
//     public `GENTOO_MIRRORS`, inline `mirror://` expansions, then the
//     literals last-listed first plus the third-party expansions
//     (appended, or prepended under `RESTRICT=primaryuri`), each location
//     attempted once (real `tried_locations`); pinned against real
//     `fetch(..., listonly=1)` output. Deliberately still cut: real
//     shuffles the `thirdpartymirrors` half (load-balancing; portuale
//     stays deterministic).
//   - Only `BLAKE2B`/`SHA512` are verified (real `MANIFEST2_HASH_DEFAULTS`
//     exactly) -- any other hash name appearing in a Manifest entry is
//     silently ignored, the same "real, standard hash, not reimplemented
//     from scratch" reasoning `ebuild_merge.rs`'s own real MD5 CONTENTS
//     digest already established for `obj` entries.
//   - No AUX/MISC/EBUILD Manifest line support (`parse_manifest` only
//     reads `DIST` lines) -- portuale never needs to verify anything
//     else a Manifest records.

pub mod layout;
pub mod mirror_cache;

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

/// Error from Manifest/SRC_URI/digest handling. Distinct variants mirror
/// the real `lib/portage/manifest.py` / `fetch.py` error message shapes
/// so `Display` reproduces them byte-for-byte (the contract suite pins
/// the CLI-visible strings).
#[derive(Debug)]
pub enum Error {
    /// `{path}: {source}` -- an `io::Error` while reading a real file.
    Io {
        path: String,
        source: std::io::Error,
    },
    /// `SRC_URI: expected "(" after {tok:?}`
    SrcUriExpectedOpenAfter { tok: String },
    /// `SRC_URI: unterminated {tok:?} group`
    SrcUriUnterminated { tok: String },
    /// `SRC_URI: unexpected {tok:?}`
    SrcUriUnexpected { tok: String },
    /// `SRC_URI: missing filename after "->"`
    SrcUriMissingFilename,
    /// `SRC_URI: unexpected token {tok:?}`
    SrcUriUnexpectedToken { tok: String },
    /// `{path}: size mismatch (expected {expected}, got {got})`
    SizeMismatch {
        path: String,
        expected: u64,
        got: usize,
    },
    /// `{path}: {algo} mismatch (expected {expected}, got {actual})`
    HashMismatch {
        path: String,
        algo: String,
        expected: String,
        actual: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{path}: {source}"),
            Error::SrcUriExpectedOpenAfter { tok } => {
                write!(f, "SRC_URI: expected \"(\" after {tok:?}")
            }
            Error::SrcUriUnterminated { tok } => {
                write!(f, "SRC_URI: unterminated {tok:?} group")
            }
            Error::SrcUriUnexpected { tok } => write!(f, "SRC_URI: unexpected {tok:?}"),
            Error::SrcUriMissingFilename => {
                write!(f, "SRC_URI: missing filename after \"->\"")
            }
            Error::SrcUriUnexpectedToken { tok } => {
                write!(f, "SRC_URI: unexpected token {tok:?}")
            }
            Error::SizeMismatch {
                path,
                expected,
                got,
            } => write!(f, "{path}: size mismatch (expected {expected}, got {got})"),
            Error::HashMismatch {
                path,
                algo,
                expected,
                actual,
            } => write!(
                f,
                "{path}: {algo} mismatch (expected {expected}, got {actual})"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

/// Real `Manifest.parseManifest2`, narrowed to `DIST` lines only (see
/// the module doc comment). A missing `Manifest` file is an empty map,
/// not an error -- same "nothing recorded yet" tolerance
/// `portage_repo::list_candidates` already gives a missing repo
/// directory.
pub fn parse_manifest(manifest_path: &Path) -> Result<HashMap<String, DistfileDigests>, Error> {
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(Error::Io {
                path: manifest_path.display().to_string(),
                source: e,
            });
        }
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
    /// Real `override_mirror` (`fetch.py:1103`): this URI token had a
    /// `mirror+` prefix. Real `file_restrict_mirror = (restrict_fetch or
    /// restrict_mirror) and not override_mirror` -- so a `mirror+` URI
    /// re-permits the public mirror list for its file even
    /// under `RESTRICT=mirror`. Implies `override_fetch` too.
    pub override_mirror: bool,
    /// Real `override_fetch` (`fetch.py:1104`): `mirror+` OR `fetch+`
    /// prefix. Real `if (restrict_fetch and not override_fetch)` skips a
    /// normal URI entirely under `RESTRICT=fetch` -- `fetch+` exempts
    /// this one URI. (Portuale's fetch path doesn't model
    /// `RESTRICT=fetch` yet, so today this only guarantees the `fetch+`
    /// prefix is stripped so the URL is valid -- see
    /// `portuale/src/fetch.rs`.)
    pub override_fetch: bool,
}

fn basename(uri: &str) -> String {
    uri.rsplit('/').next().unwrap_or(uri).to_string()
}

/// Recursive-descent parser for the grammar described in the module doc
/// comment. `active(negated, flag)` decides whether a `flag?`/`!flag?`
/// group's own contents should be collected -- callers pass a real USE
/// membership check for `SRC_URI` itself (portuale's own always-empty
/// USE set, same v1 cut every other real-execution slice already has),
/// or an always-true closure to compute the real `AA` variable (every
/// file `SRC_URI` could ever reference, regardless of USE -- PMS's own
/// definition of `AA`).
fn parse_list(
    tokens: &[&str],
    pos: &mut usize,
    active: &impl Fn(bool, &str) -> bool,
) -> Result<Vec<SrcUriEntry>, Error> {
    let mut out = Vec::new();
    while *pos < tokens.len() && tokens[*pos] != ")" {
        let tok = tokens[*pos];
        if let Some(flag) = tok.strip_suffix('?') {
            *pos += 1;
            if tokens.get(*pos) != Some(&"(") {
                return Err(Error::SrcUriExpectedOpenAfter {
                    tok: tok.to_string(),
                });
            }
            *pos += 1;
            let (negated, flag) = match flag.strip_prefix('!') {
                Some(f) => (true, f),
                None => (false, flag),
            };
            let inner = parse_list(tokens, pos, active)?;
            if tokens.get(*pos) != Some(&")") {
                return Err(Error::SrcUriUnterminated {
                    tok: tok.to_string(),
                });
            }
            *pos += 1;
            if active(negated, flag) {
                out.extend(inner);
            }
        } else if tok == "(" || tok == ")" {
            return Err(Error::SrcUriUnexpected {
                tok: tok.to_string(),
            });
        } else {
            *pos += 1;
            // Real `fetch.py:1103-1106`: strip a `mirror+`/`fetch+`
            // prefix off the URI token and record which restriction(s)
            // it overrides. `mirror+` implies `fetch+` (real
            // `override_fetch = override_mirror or ...`).
            let (uri, override_mirror, override_fetch) =
                if let Some(rest) = tok.strip_prefix("mirror+") {
                    (rest, true, true)
                } else if let Some(rest) = tok.strip_prefix("fetch+") {
                    (rest, false, true)
                } else {
                    (tok, false, false)
                };
            let filename = if tokens.get(*pos) == Some(&"->") {
                *pos += 1;
                let Some(name) = tokens.get(*pos) else {
                    return Err(Error::SrcUriMissingFilename);
                };
                *pos += 1;
                name.to_string()
            } else {
                basename(uri)
            };
            out.push(SrcUriEntry {
                uri: uri.to_string(),
                filename,
                override_mirror,
                override_fetch,
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
) -> Result<Vec<SrcUriEntry>, Error> {
    let tokens: Vec<&str> = src_uri.split_whitespace().collect();
    let mut pos = 0;
    let entries = parse_list(&tokens, &mut pos, &active)?;
    if pos != tokens.len() {
        return Err(Error::SrcUriUnexpectedToken {
            tok: tokens.get(pos).copied().unwrap_or("").to_string(),
        });
    }
    Ok(entries)
}

/// Real `grabdict()` (`lib/portage/util/__init__.py`), narrowed to what
/// portuale needs: real `profiles/thirdpartymirrors`'s own format --
/// one `<name> <url1> [<url2> ...]` entry per line. A whole line
/// starting with `#`, or any token from the first `#`-prefixed one
/// onward, is a comment (real `grabdict`'s own per-token truncation,
/// not just whole-line); a line left with fewer than 2 tokens after
/// that (a bare name with zero URLs, or a blank line) is skipped (real
/// `grabdict`'s own `empty=0` default). A missing file is an empty
/// map, not an error -- the same tolerance `parse_manifest` already
/// gives a missing `Manifest`.
pub fn parse_thirdpartymirrors(path: &Path) -> Result<HashMap<String, Vec<String>>, Error> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(Error::Io {
                path: path.display().to_string(),
                source: e,
            });
        }
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

/// Real `mirror://<name>/<path>` resolution (real `fetch.py:1136-1160`):
/// `<name>` is looked up in `custommirrors` *and* `thirdpartymirrors`
/// (real comment: "Try user-defined mirrors first" -- `custommirrors`'s
/// own roots for the name, if any, are listed before
/// `thirdpartymirrors`'s own, real portage's own exact real order),
/// expanding to `<mirror_root>/<path>` for every root under that name in
/// each map (real `cmirr.rstrip("/") + "/" + path` /
/// `locmirr.rstrip("/") + "/" + path`, string-built identically for
/// both -- real portage `random.shuffle`s the `thirdpartymirrors` half
/// only, deliberately not replicated here, see this module's own doc
/// comment). A `mirror://` token whose name isn't known to *either* map,
/// or that's malformed (`mirror://` with no further `/` at all), yields
/// no candidates -- real portage's own `writemsg` warning, not a hard
/// error; the caller still fails loudly if this leaves a file with no
/// working candidate at all, the same real end result. A non-
/// `mirror://` URI is returned unchanged, as its own single candidate --
/// so every `SrcUriEntry.uri` can be passed through this function
/// uniformly, regardless of whether it's actually a `mirror://` token.
///
/// Real `custommirrors["local"]`'s own *separate* meaning (local mirrors
/// tried before any other candidate, real `fetch.py:1017-1029`'s own
/// `fsmirrors`/`local_mirrors` split) is handled by the fetch loop
/// (`portuale/src/fetch.rs`), not here -- a `mirror://local/...` token
/// still resolves normally through this function (real portage's own
/// `if mirrorname in custommirrors:` check doesn't treat `"local"`
/// specially either).
pub fn resolve_mirror_candidates(
    uri: &str,
    custommirrors: &HashMap<String, Vec<String>>,
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
    let expand = |roots: &HashMap<String, Vec<String>>| -> Vec<String> {
        roots
            .get(name)
            .map(|roots| {
                roots
                    .iter()
                    .map(|root| format!("{}/{}", root.trim_end_matches('/'), path))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut candidates = expand(custommirrors);
    candidates.extend(expand(thirdpartymirrors));
    candidates
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Real `make.globals`'s own default `FETCHCOMMAND` template, verbatim.
pub const DEFAULT_FETCHCOMMAND: &str = concat!(
    "wget -t 3 -T 60 --passive-ftp -U \"Portage (Gentoo, https://www.gentoo.org) ",
    "distfile-fetch\" -O \"${DISTDIR}/${FILE}\" \"${URI}\""
);

/// Real `make.globals`'s `RESUMECOMMAND`, byte-for-byte the
/// `FETCHCOMMAND` template plus `-c`.
pub const DEFAULT_RESUMECOMMAND: &str = concat!(
    "wget -c -t 3 -T 60 --passive-ftp -U \"Portage (Gentoo, https://www.gentoo.org) ",
    "distfile-fetch\" -O \"${DISTDIR}/${FILE}\" \"${URI}\""
);

/// Real `fetch.py:1652-1700`'s fetch-command family, resolved from the
/// config (`FETCHCOMMAND`, `RESUMECOMMAND`, and the per-protocol
/// `FETCHCOMMAND_<PROTO>` / `RESUMECOMMAND_<PROTO>` variants). `None`
/// (and an absent protocol key) means the `make.globals` default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchCommands {
    pub fetchcommand: Option<String>,
    pub resumecommand: Option<String>,
    /// Keys are the upper-cased URI scheme (`HTTP`, `HTTPS`, ...).
    pub fetchcommand_proto: std::collections::HashMap<String, String>,
    pub resumecommand_proto: std::collections::HashMap<String, String>,
}

impl Default for FetchCommands {
    /// The `make.globals` defaults, no overrides.
    fn default() -> Self {
        Self {
            fetchcommand: Some(DEFAULT_FETCHCOMMAND.to_string()),
            resumecommand: Some(DEFAULT_RESUMECOMMAND.to_string()),
            fetchcommand_proto: std::collections::HashMap::new(),
            resumecommand_proto: std::collections::HashMap::new(),
        }
    }
}

/// A process-wide [`FetchCommands`] holding only the `make.globals`
/// defaults, for callers with no resolved config (`WgetFetcher`'s absent
/// override).
pub fn default_commands() -> &'static FetchCommands {
    static DEFAULT: std::sync::LazyLock<FetchCommands> =
        std::sync::LazyLock::new(FetchCommands::default);
    &DEFAULT
}

/// Real `fetch.py:1795-1811`'s non-path substitution inputs: the
/// distfile's `DIGESTS` string and the `PORTAGE_SSH_OPTS` setting. Real
/// inserts each into `variables` only when the settings carry it; an
/// unset key still expands to empty in `varexpand`, so `None` and
/// `Some("")` produce the same command line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchCommandVars<'a> {
    /// `" ".join(f"{k.lower()}:{v}" ...)` over the Manifest hashes, size
    /// excluded -- build it with [`digests_variable`].
    pub digests: Option<&'a str>,
    /// Real `mysettings.get("PORTAGE_SSH_OPTS")` (used by the shipped
    /// `FETCHCOMMAND_SSH`/`_SFTP` templates as `"${PORTAGE_SSH_OPTS}"`).
    pub portage_ssh_opts: Option<&'a str>,
}

/// The `DIGESTS` fetch-command variable for one Manifest entry, real
/// `fetch.py:1797-1803`: `" ".join(f"{k.lower()}:{v}")` over every hash
/// (the `size` entry excluded), e.g.
/// `blake2b:<hex> sha512:<hex>`.
///
/// Real preserves the `Manifest` line's hash order (`portage.manifest`'s
/// parser builds an insertion-ordered dict); portuale's
/// [`DistfileDigests`] carries a `HashMap`, so the names are sorted
/// instead. The shipped tree's `BLAKE2B`-then-`SHA512` order matches the
/// sort (200/200 sampled `DIST` lines, 2026-09-16) -- a hand-written
/// reverse-order Manifest would differ; recorded in `docs/history/02.68-74.md`
/// §6.
pub fn digests_variable(digests: &DistfileDigests) -> String {
    let mut names: Vec<&String> = digests.hashes.keys().collect();
    names.sort();
    names
        .iter()
        .map(|name| format!("{}:{}", name.to_ascii_lowercase(), digests.hashes[*name]))
        .collect::<Vec<_>>()
        .join(" ")
}

impl FetchCommands {
    /// Real's selection (`fetch.py:1652-1662` for fetch, `:1680-1690` for
    /// resume): the protocol variant first, then the plain name.
    fn select(&self, proto: &str, resume: bool) -> Result<(String, String), String> {
        let (plain_var, plain, proto_var, proto_map) = if resume {
            (
                "RESUMECOMMAND",
                &self.resumecommand,
                format!("RESUMECOMMAND_{proto}"),
                &self.resumecommand_proto,
            )
        } else {
            (
                "FETCHCOMMAND",
                &self.fetchcommand,
                format!("FETCHCOMMAND_{proto}"),
                &self.fetchcommand_proto,
            )
        };
        if let Some(cmd) = proto_map.get(proto) {
            return Ok((proto_var, cmd.clone()));
        }
        match plain {
            Some(cmd) => Ok((plain_var.to_string(), cmd.clone())),
            None => Err(format!(
                "!!! {plain_var} is unset. It should have been defined in\n\
                 !!! /usr/share/portage/config/make.globals.\n"
            )),
        }
    }
}

/// Real `varexpand` (`portage/util/__init__.py:885-1015`) over a
/// fetch-command template, with `mydict = variables`
/// (`fetch.py:1795-1820`): `${VARNAME}`/`$VARNAME` expand from `vars`
/// and an **unknown name expands to empty** (the shell's unset-variable
/// default), an escaped `\$` is a literal `$`, `\\` is a literal `\`
/// (plus real's bug-compatible extra character when the next one is a
/// quote or `$`), an escaped newline disappears, any other `\x` keeps
/// both characters, and a surviving single quote suspends expansion.
///
/// Same algorithm as `portage_profile`'s config-value `substitute`
/// (R1); the difference is the lookup: no process-environment fallback
/// here, matching real's fetch-time `mydict`.
fn varexpand(template: &str, vars: &HashMap<String, String>) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    let mut in_single = false;
    let mut in_double = false;
    while pos < chars.len() {
        let current = chars[pos];
        match current {
            '\'' => {
                out.push('\'');
                if !in_double {
                    in_single = !in_single;
                }
                pos += 1;
            }
            '"' => {
                out.push('"');
                if !in_single {
                    in_double = !in_double;
                }
                pos += 1;
            }
            '\\' if !in_single => {
                if pos + 1 >= chars.len() {
                    out.push('\\');
                    break;
                }
                let next = chars[pos + 1];
                pos += 2;
                match next {
                    '$' => out.push('$'),
                    '\\' => {
                        out.push('\\');
                        if pos < chars.len() && matches!(chars[pos], '\'' | '"' | '$') {
                            out.push(chars[pos]);
                            pos += 1;
                        }
                    }
                    '\n' => {}
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            '$' if !in_single => {
                pos += 1;
                if pos == chars.len() {
                    out.push('$');
                    continue;
                }
                let braced = chars[pos] == '{';
                if braced {
                    pos += 1;
                    if pos == chars.len() {
                        return String::new();
                    }
                }
                let start = pos;
                while pos < chars.len() && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_')
                {
                    pos += 1;
                }
                let name: String = chars[start..pos].iter().collect();
                if braced {
                    if pos == chars.len() || chars[pos] != '}' {
                        return String::new();
                    }
                    pos += 1;
                }
                if name.is_empty() {
                    return String::new();
                }
                if let Some(value) = vars.get(&name) {
                    out.push_str(value);
                }
            }
            _ => {
                out.push(current);
                pos += 1;
            }
        }
    }
    out
}

/// Real `varexpand` + `shlex.split` over the selected command
/// (`fetch.py:1813-1820`): the variables are substituted, then the string
/// is split into argv with POSIX quoting rules -- real never runs it
/// through a shell.
fn expand_and_split(command: &str, vars: &HashMap<String, String>) -> Vec<String> {
    shell_split(&varexpand(command, vars))
}

/// POSIX-ish argv splitting, public for callers parsing a config value
/// real runs through `shlex.split` (e.g. `PORTAGE_RO_DISTDIRS`).
pub fn split_shell_words(s: &str) -> Vec<String> {
    shell_split(s)
}

/// Python `shlex.split` (posix) argv splitting: whitespace separates,
/// single quotes are literal, a backslash escapes the next character
/// outside single quotes, and inside double quotes only `\"`/`\\` lose
/// the backslash (`\$`, `` \` ``, `\ ` keep it -- see the in-double
/// branch).
fn shell_split(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut has_cur = false;
    let mut chars = s.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                cur.push(c);
            }
            has_cur = true;
            continue;
        }
        if in_double {
            if c == '"' {
                in_double = false;
            } else if c == '\\'
                && let Some(&next) = chars.peek()
                && matches!(next, '"' | '\\')
            {
                // Python `shlex` (posix) removes the backslash only
                // before the closing quote and before another
                // backslash; `\$`, `` \` `` and `\ ` keep it (real
                // fetch commands run through `shlex.split`, not a
                // shell).
                cur.push(chars.next().unwrap());
            } else {
                cur.push(c);
            }
            has_cur = true;
            continue;
        }
        match c {
            ' ' | '\t' | '\n' => {
                if has_cur {
                    args.push(std::mem::take(&mut cur));
                    has_cur = false;
                }
            }
            '\'' => {
                in_single = true;
                has_cur = true;
            }
            '"' => {
                in_double = true;
                has_cur = true;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    has_cur = true;
                }
            }
            _ => {
                cur.push(c);
                has_cur = true;
            }
        }
    }
    if has_cur {
        args.push(cur);
    }
    args
}

/// Real `fetch.py:1652-1716` through `:1830`: select `FETCHCOMMAND` (or
/// its protocol variant) and `RESUMECOMMAND` (both are selected and
/// checked, regardless of which one this attempt runs), warn about a
/// command without `${FILE}`, substitute
/// `${URI}`/`${FILE}`/`${DIGESTS}`/`${DISTDIR}`/`${PORTAGE_SSH_OPTS}`,
/// and spawn the real subprocess (never an in-process HTTP client). A
/// failed fresh fetch removes whatever partial file it left behind; a
/// failed resume keeps it for the next candidate -- the split
/// `portuale::fetch::fetch_src_uri`'s candidate loop relies on.
///
/// The `${FILE}` refusal is real's, not a blanket one: each command
/// missing the parameter prints its own
/// `!!! <VAR> does not contain the required ${FILE} parameter.` line,
/// then the shared make.conf(5) hint, and the fetch aborts **only when
/// the distfile name differs from the URL basename**
/// (`fetch.py:1713-1715`: `if myfile != os.path.basename(loc): return 0`).
/// When the names match, real falls through and runs the command anyway
/// -- `wget -P "${DISTDIR}" "${URI}"` still lands the right file.
///
/// `FILE` is `dest`'s own basename, **not** real's
/// `basename(download_path)`: real downloads to
/// `<myfile>.__download__` (`_download_suffix`, `fetch.py:63`) and
/// renames after verification, while portuale writes `dest` directly.
/// This slice deliberately does not change portuale's download path --
/// recorded in `docs/history/02.68-74.md` §6.
pub fn download_with_commands(
    uri: &str,
    dest: &Path,
    resume: bool,
    distdir: &Path,
    vars: FetchCommandVars<'_>,
    commands: &FetchCommands,
) -> Result<(), String> {
    let proto = uri
        .split_once("://")
        .map(|(p, _)| p.to_ascii_uppercase())
        .unwrap_or_default();
    let (fetch_var, fetch_command) = commands.select(&proto, false)?;
    let (resume_var, resume_command) = commands.select(&proto, true)?;
    let file = dest
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let missing_file_param: String = [(&fetch_var, &fetch_command), (&resume_var, &resume_command)]
        .iter()
        .filter(|(_, command)| !command.contains("${FILE}"))
        .map(|(var, _)| format!("!!! {var} does not contain the required ${{FILE}} parameter.\n"))
        .collect();
    if !missing_file_param.is_empty() {
        let hint = "!!! Refer to the make.conf(5) man page for information about how to\n\
                    !!! correctly specify FETCHCOMMAND and RESUMECOMMAND.\n";
        let message = format!("{missing_file_param}{hint}");
        let url_basename = uri.rsplit('/').next().unwrap_or("");
        if file != url_basename {
            // Real `return 0`: abort the fetch, never spawn.
            return Err(message);
        }
        // Names match: real still prints the warning before running the
        // command; there is no `Err` to carry it here, so it goes to
        // stderr.
        eprint!("{message}");
    }
    let (var, command) = if resume {
        (resume_var, resume_command)
    } else {
        (fetch_var, fetch_command)
    };
    let mut variables: HashMap<String, String> = HashMap::new();
    variables.insert("URI".to_string(), uri.to_string());
    variables.insert("FILE".to_string(), file.clone());
    variables.insert("DISTDIR".to_string(), distdir.display().to_string());
    if let Some(digests) = vars.digests {
        variables.insert("DIGESTS".to_string(), digests.to_string());
    }
    if let Some(opts) = vars.portage_ssh_opts {
        variables.insert("PORTAGE_SSH_OPTS".to_string(), opts.to_string());
    }
    let argv = expand_and_split(&command, &variables);
    let Some((prog, rest)) = argv.split_first() else {
        return Err(format!("!!! {var} is empty.\n"));
    };
    let status = std::process::Command::new(prog)
        .args(rest)
        .status()
        .map_err(|e| format!("failed to spawn {prog}: {e}"))?;
    if !status.success() {
        if !resume {
            let _ = std::fs::remove_file(dest);
        }
        return Err(format!("{prog} failed to fetch {uri:?} ({status})"));
    }
    Ok(())
}

/// The `make.globals` default transport, for callers with no resolved
/// config in hand: `download_with_commands` over [`FetchCommands`]'s
/// defaults and no extra variables. `resume` selects `RESUMECOMMAND`
/// (byte-for-byte `FETCHCOMMAND` plus `-c`). This is the one `wget`
/// invocation both the `portuale` fetch path and the `mrg-director`
/// `Fetcher` seam run when no override is configured; it lives here so
/// the transport is shared, not duplicated per caller.
pub fn download_via_wget(uri: &str, dest: &Path, resume: bool) -> Result<(), String> {
    let distdir = dest.parent().unwrap_or_else(|| Path::new("."));
    download_with_commands(
        uri,
        dest,
        resume,
        distdir,
        FetchCommandVars::default(),
        &FetchCommands::default(),
    )
}

/// Real digest verification: file size (a cheap, real `_check_distfile`
/// pre-check before ever hashing) plus every `BLAKE2B`/`SHA512` entry
/// `digests` carries -- see the module doc comment for why only those
/// two. Any other hash name present is silently skipped (not
/// mismatched, not verified). An empty `digests.hashes` (a `Manifest`
/// entry with no recognized hash at all) still passes once the size
/// matches, the same "size alone is still something" tolerance real
/// `_check_distfile` gives.
pub fn verify_digests(path: &Path, digests: &DistfileDigests) -> Result<(), Error> {
    // `blake2`/`sha2` both re-export the same underlying `digest::Digest`
    // trait under their own name -- importing it once is enough for
    // both `Blake2b512::digest`/`Sha512::digest` below.
    use blake2::Digest as _;

    let bytes = std::fs::read(path).map_err(|e| Error::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    if bytes.len() as u64 != digests.size {
        return Err(Error::SizeMismatch {
            path: path.display().to_string(),
            expected: digests.size,
            got: bytes.len(),
        });
    }
    for (algo, expected) in &digests.hashes {
        let actual = match algo.as_str() {
            "BLAKE2B" => to_hex(&blake2::Blake2b512::digest(&bytes)),
            "SHA512" => to_hex(&sha2::Sha512::digest(&bytes)),
            _ => continue,
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::HashMismatch {
                path: path.display().to_string(),
                algo: algo.to_string(),
                expected: expected.to_string(),
                actual,
            });
        }
    }
    Ok(())
}

/// The streaming sibling of [`verify_digests`]: the same size-first
/// check and the same BLAKE2B/SHA512 set, but the bytes come from a
/// reader -- the gpkg outer-container member stream (`#58` S5), so a
/// large member is neither buffered in memory nor copied to a scratch
/// file. `name` labels the member in the error messages (the `path`
/// field the file variant fills with the on-disk path).
pub fn verify_digests_reader<R: std::io::Read>(
    name: &str,
    mut reader: R,
    digests: &DistfileDigests,
) -> Result<(), Error> {
    // `blake2`/`sha2` both re-export the same underlying `digest::Digest`
    // trait; see `verify_digests`.
    use blake2::Digest as _;

    let mut blake2b = blake2::Blake2b512::new();
    let mut sha512 = sha2::Sha512::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size: u64 = 0;
    loop {
        let n = reader.read(&mut buffer).map_err(|source| Error::Io {
            path: name.to_string(),
            source,
        })?;
        if n == 0 {
            break;
        }
        size += n as u64;
        blake2b.update(&buffer[..n]);
        sha512.update(&buffer[..n]);
    }
    if size != digests.size {
        return Err(Error::SizeMismatch {
            path: name.to_string(),
            expected: digests.size,
            got: size as usize,
        });
    }
    let blake2b_hex = to_hex(&blake2b.finalize());
    let sha512_hex = to_hex(&sha512.finalize());
    for (algo, expected) in &digests.hashes {
        let actual = match algo.as_str() {
            "BLAKE2B" => blake2b_hex.clone(),
            "SHA512" => sha512_hex.clone(),
            _ => continue,
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::HashMismatch {
                path: name.to_string(),
                algo: algo.to_string(),
                expected: expected.to_string(),
                actual,
            });
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
    fn fetch_commands_select_the_protocol_variant_then_the_plain_name() {
        let mut commands = FetchCommands::default();
        commands
            .fetchcommand_proto
            .insert("HTTP".to_string(), "curl ${URI}".to_string());
        commands.fetchcommand = Some("wget ${FILE}".to_string());
        assert_eq!(
            commands.select("HTTP", false).unwrap(),
            ("FETCHCOMMAND_HTTP".to_string(), "curl ${URI}".to_string())
        );
        assert_eq!(
            commands.select("HTTPS", false).unwrap(),
            ("FETCHCOMMAND".to_string(), "wget ${FILE}".to_string())
        );
        // Resume picks the RESUMECOMMAND family.
        commands
            .resumecommand_proto
            .insert("HTTPS".to_string(), "curl -C - ${URI}".to_string());
        assert_eq!(
            commands.select("HTTPS", true).unwrap(),
            (
                "RESUMECOMMAND_HTTPS".to_string(),
                "curl -C - ${URI}".to_string()
            )
        );
        // A default-constructed family selects the shipped make.globals
        // templates: an absent config key means "default", never "unset"
        // (#70 R2 / T2).
        let defaults = FetchCommands::default();
        assert_eq!(
            defaults.select("HTTPS", false).unwrap(),
            ("FETCHCOMMAND".to_string(), DEFAULT_FETCHCOMMAND.to_string())
        );
        assert_eq!(
            defaults.select("HTTPS", true).unwrap(),
            (
                "RESUMECOMMAND".to_string(),
                DEFAULT_RESUMECOMMAND.to_string()
            )
        );
        // An unset plain command is real's own error, reachable only
        // from a hand-built family.
        let empty = FetchCommands {
            fetchcommand: None,
            resumecommand: None,
            ..FetchCommands::default()
        };
        let err = empty.select("HTTPS", false).unwrap_err();
        assert!(err.contains("FETCHCOMMAND is unset"), "{err}");
    }

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn expand_and_split_substitutes_and_quotes_like_shlex() {
        let argv = expand_and_split(
            "wget -O \"${DISTDIR}/${FILE}\" \"${URI}\" -U 'Mozilla x'",
            &vars(&[
                ("DISTDIR", "/var/cache/distfiles"),
                ("FILE", "foo-1.0.tar.gz"),
                ("URI", "https://example.com/foo-1.0.tar.gz"),
            ]),
        );
        assert_eq!(
            argv,
            vec![
                "wget",
                "-O",
                "/var/cache/distfiles/foo-1.0.tar.gz",
                "https://example.com/foo-1.0.tar.gz",
                "-U",
                "Mozilla x",
            ]
        );
        // The bare `$VAR` form is real `varexpand`'s too.
        let argv = expand_and_split(
            "cp $URI ${DISTDIR}/${FILE}",
            &vars(&[
                ("URI", "file:///src/foo"),
                ("DISTDIR", "/d"),
                ("FILE", "foo"),
            ]),
        );
        assert_eq!(argv, vec!["cp", "file:///src/foo", "/d/foo"]);
    }

    /// #70 R4 oracle: the real `FETCHCOMMAND_SSH`
    /// (`make.globals:70`, resolved by R1) expanded with the fetch
    /// variables, captured 2026-09-16 (portage 3.0.82.2) with
    /// `python3 -c "from portage.util import varexpand; import shlex;
    /// print(shlex.split(varexpand(CMD, mydict=VARS)))"`:
    ///
    /// unset `PORTAGE_SSH_OPTS`:
    /// ['bash', '-c', 'x=${2#ssh://} ; host=${x%%/*} ;
    ///  port=${host##*:} ; host=${host%:*} ; [[ ${host} = ${port} ]] &&
    ///  port= ; exec rsync --rsh="ssh ${port:+-p${port}} ${3}" -avP
    ///  "${host}:/${x#*/}" "$1"', 'rsync',
    ///  '/var/cache/distfiles/foo-1.0.tar.gz',
    ///  'https://example.org/distfiles/foo-1.0.tar.gz', '']
    ///
    /// set to `-o ServerAliveInterval=5 -o User=portage`: same argv with
    /// that string as the last element.
    #[test]
    fn expand_and_split_matches_real_varexpand_on_fetchcommand_ssh() {
        let command = r#"bash -c "x=\${2#ssh://} ; host=\${x%%/*} ; port=\${host##*:} ; host=\${host%:*} ; [[ \${host} = \${port} ]] && port= ; exec rsync --rsh=\"ssh \${port:+-p\${port}} \${3}\" -avP \"\${host}:/\${x#*/}\" \"\$1\"" rsync "${DISTDIR}/${FILE}" "${URI}" "${PORTAGE_SSH_OPTS}""#;
        let base = vars(&[
            ("DISTDIR", "/var/cache/distfiles"),
            ("URI", "https://example.org/distfiles/foo-1.0.tar.gz"),
            ("FILE", "foo-1.0.tar.gz"),
            ("DIGESTS", "blake2b:aa sha512:bb"),
        ]);
        let bash_script = r#"x=${2#ssh://} ; host=${x%%/*} ; port=${host##*:} ; host=${host%:*} ; [[ ${host} = ${port} ]] && port= ; exec rsync --rsh="ssh ${port:+-p${port}} ${3}" -avP "${host}:/${x#*/}" "$1""#;
        let mut unset = base.clone();
        assert_eq!(
            expand_and_split(command, &unset),
            vec![
                "bash",
                "-c",
                bash_script,
                "rsync",
                "/var/cache/distfiles/foo-1.0.tar.gz",
                "https://example.org/distfiles/foo-1.0.tar.gz",
                "",
            ]
        );
        unset.insert(
            "PORTAGE_SSH_OPTS".to_string(),
            "-o ServerAliveInterval=5 -o User=portage".to_string(),
        );
        assert_eq!(
            expand_and_split(command, &unset),
            vec![
                "bash",
                "-c",
                bash_script,
                "rsync",
                "/var/cache/distfiles/foo-1.0.tar.gz",
                "https://example.org/distfiles/foo-1.0.tar.gz",
                "-o ServerAliveInterval=5 -o User=portage",
            ]
        );
    }

    /// The rest of real's fetch-time `varexpand` over a command
    /// template: unknown names empty, `\$` literal, `\\` unescaped,
    /// single-quote suspension (captured with the same python command;
    /// `['cp', 'x']` for the unknown-vars line).
    #[test]
    fn expand_and_split_varexpand_unknown_vars_and_escapes_match_real() {
        assert_eq!(
            expand_and_split("cp $NOSUCH ${ALSO_MISSING} x", &vars(&[("FILE", "f")])),
            vec!["cp", "x"]
        );
        assert_eq!(
            expand_and_split(
                r#"cp "\$FILE" "\${URI}" "\\$FILE""#,
                &vars(&[("FILE", "f"), ("URI", "u")]),
            ),
            vec!["cp", "$FILE", "${URI}", "\\$FILE"]
        );
        assert_eq!(
            expand_and_split("cp '$FILE' x", &vars(&[("FILE", "f")])),
            vec!["cp", "$FILE", "x"]
        );
    }

    #[test]
    fn digests_variable_formats_like_real_and_skips_size() {
        let mut hashes = HashMap::new();
        hashes.insert("SHA512".to_string(), "bb".to_string());
        hashes.insert("BLAKE2B".to_string(), "aa".to_string());
        let digests = DistfileDigests { size: 11, hashes };
        assert_eq!(digests_variable(&digests), "blake2b:aa sha512:bb");
    }

    /// A stub "fetch command": `cp <uri-path> <dest>` (the URI is a
    /// `file://` path in the tests below). Spawned directly, exactly the
    /// way a real `FETCHCOMMAND` is.
    fn stub_fetch_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn download_with_commands_runs_a_stub_command_and_refuses_one_without_file() {
        let dir = tempdir();
        let source = dir.join("source.tar.gz");
        fs::write(&source, b"payload").unwrap();
        let dest = dir.join("dest.tar.gz");
        let script = stub_fetch_script(&dir, "fetch.sh", "cp \"${2#file://}\" \"$1\"");
        let mut commands = FetchCommands {
            fetchcommand: Some(format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            )),
            ..FetchCommands::default()
        };
        let uri = format!("file://{}", source.display());
        download_with_commands(
            &uri,
            &dest,
            false,
            &dir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"payload");

        // A command without ${FILE} is refused with real's message and
        // never spawned.
        commands.fetchcommand = Some(format!("{} \"${{URI}}\"", script.display()));
        let err = download_with_commands(
            &uri,
            &dest,
            false,
            &dir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap_err();
        assert!(
            err.contains("does not contain the required ${FILE} parameter"),
            "{err}"
        );

        // The protocol variant wins for a file:// URI.
        commands.fetchcommand_proto.insert(
            "FILE".to_string(),
            format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            ),
        );
        commands.fetchcommand = Some("false".to_string());
        let dest2 = dir.join("dest2.tar.gz");
        download_with_commands(
            &uri,
            &dest2,
            false,
            &dir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap();
        assert!(dest2.exists());
    }

    /// #70 R3: real checks **both** the fetch and resume commands for
    /// `${FILE}` (`fetch.py:1667-1716`) and aborts only when the distfile
    /// name differs from the URL basename; on matching names it warns and
    /// runs anyway. The abort's messages travel in the `Err` (that is
    /// where this crate's callers surface command diagnostics).
    #[test]
    fn download_with_commands_warns_for_both_commands_and_aborts_only_when_renamed() {
        let srcdir = tempdir();
        let dldir = tempdir();
        let source = srcdir.join("source.tar.gz");
        fs::write(&source, b"payload").unwrap();
        let marker = dldir.join("spawned");
        let script = stub_fetch_script(
            &dldir,
            "wget-p.sh",
            // The stub of `wget -P "${DISTDIR}" "${URI}"`: copy the URI's
            // basename into the directory, no ${FILE} anywhere.
            &format!("touch {}\ncp \"${{2#file://}}\" \"$1/\"", marker.display()),
        );
        let uri = format!("file://{}", source.display());
        let no_file = format!("{} \"${{DISTDIR}}\" \"${{URI}}\"", script.display());

        // (a) the distfile name equals the URL basename: the warning is
        // printed to stderr and the command still runs.
        let dest = dldir.join("source.tar.gz");
        let commands = FetchCommands {
            fetchcommand: Some(no_file.clone()),
            ..FetchCommands::default()
        };
        download_with_commands(
            &uri,
            &dest,
            false,
            &dldir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap();
        assert!(marker.is_file(), "the ${{FILE}}-less command ran");
        assert_eq!(fs::read(&dest).unwrap(), b"payload");
        fs::remove_file(&marker).unwrap();

        // (b) a renamed distfile differs from the URL basename: real
        // aborts before spawning.
        let renamed = dldir.join("renamed.tar.gz");
        let err = download_with_commands(
            &uri,
            &renamed,
            false,
            &dldir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap_err();
        assert!(
            err.contains("FETCHCOMMAND does not contain the required ${FILE} parameter"),
            "{err}"
        );
        assert!(err.contains("make.conf(5)"), "{err}");
        assert!(!marker.exists(), "no spawn after the abort");
        assert!(!renamed.exists());

        // (c) only RESUMECOMMAND lacks ${FILE}: it is selected and named
        // even though this is a fresh fetch, and it aborts the renamed
        // case.
        let commands = FetchCommands {
            resumecommand: Some(no_file),
            ..FetchCommands::default()
        };
        let err = download_with_commands(
            &uri,
            &renamed,
            false,
            &dldir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap_err();
        assert!(
            err.contains("RESUMECOMMAND does not contain the required ${FILE} parameter"),
            "{err}"
        );
        assert!(!marker.exists(), "no spawn after the abort");
    }

    #[test]
    fn download_with_commands_removes_a_fresh_partial_but_keeps_a_resumed_one() {
        let dir = tempdir();
        let dest = dir.join("dest.tar.gz");
        let script = stub_fetch_script(&dir, "fail.sh", "echo partial > \"$1\"; exit 1");
        let mut commands = FetchCommands {
            fetchcommand: Some(format!(
                "{} \"${{DISTDIR}}/${{FILE}}\" \"${{URI}}\"",
                script.display()
            )),
            ..FetchCommands::default()
        };
        commands.resumecommand = commands.fetchcommand.clone();
        download_with_commands(
            "file:///nonexistent",
            &dest,
            false,
            &dir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap_err();
        assert!(!dest.exists(), "a failed fresh fetch removes the partial");
        download_with_commands(
            "file:///nonexistent",
            &dest,
            true,
            &dir,
            FetchCommandVars::default(),
            &commands,
        )
        .unwrap_err();
        assert!(
            dest.exists(),
            "a failed resume keeps it for the next candidate"
        );
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
                override_mirror: false,
                override_fetch: false,
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
    fn flatten_src_uri_strips_a_mirror_prefix_and_records_both_overrides() {
        let entries =
            flatten_src_uri("mirror+https://example.com/a-1.0.tar.gz", |_, _| true).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].uri, "https://example.com/a-1.0.tar.gz");
        assert_eq!(entries[0].filename, "a-1.0.tar.gz");
        assert!(entries[0].override_mirror);
        assert!(entries[0].override_fetch, "mirror+ implies fetch+");
    }

    #[test]
    fn flatten_src_uri_strips_a_fetch_prefix_and_records_only_override_fetch() {
        let entries = flatten_src_uri(
            "fetch+https://example.com/dl?v=1 -> a-1.0.tar.gz",
            |_, _| true,
        )
        .unwrap();
        assert_eq!(entries[0].uri, "https://example.com/dl?v=1");
        assert_eq!(entries[0].filename, "a-1.0.tar.gz");
        assert!(!entries[0].override_mirror);
        assert!(entries[0].override_fetch);
    }

    #[test]
    fn flatten_src_uri_a_plain_uri_has_neither_override() {
        let entries = flatten_src_uri("https://example.com/a-1.0.tar.gz", |_, _| true).unwrap();
        assert!(!entries[0].override_mirror);
        assert!(!entries[0].override_fetch);
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
        // Portuale's own always-empty USE set (see the module doc
        // comment) means every `!flag?` group is always active -- the
        // real, common way an optional dependency's SRC_URI still gets
        // fetched by default. `is_set` here stands in for that
        // always-empty USE set: every flag is unset.
        let is_set = |_flag: &str| false;
        let active = |negated: bool, flag: &str| {
            if negated { !is_set(flag) } else { is_set(flag) }
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
        assert!(err.to_string().contains("size mismatch"), "{err}");
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
        assert!(err.to_string().contains("SHA512 mismatch"), "{err}");
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
    fn verify_digests_reader_matches_verify_digests_on_the_same_bytes() {
        // The gpkg outer-container path (`#58` S5) streams a member
        // through this variant; the bytes and the two digests are the
        // same real "hello world" vectors as the file tests above.
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
        assert!(verify_digests_reader("member", &b"hello world"[..], &digests).is_ok());
        // A size mismatch is reported before hashing.
        let wrong_size = DistfileDigests {
            size: 999,
            hashes: HashMap::new(),
        };
        let err = verify_digests_reader("member", &b"hello world"[..], &wrong_size).unwrap_err();
        assert!(err.to_string().contains("size mismatch"), "{err}");
        // A hash mismatch names the member and the algorithm.
        let mut hashes = HashMap::new();
        hashes.insert("SHA512".to_string(), "0".repeat(128));
        let digests = DistfileDigests { size: 11, hashes };
        let err = verify_digests_reader("member", &b"hello world"[..], &digests).unwrap_err();
        assert!(err.to_string().contains("member: SHA512 mismatch"), "{err}");
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
        let candidates = resolve_mirror_candidates(
            "mirror://gentoo/app-arch/foo-1.0.tar.gz",
            &HashMap::new(),
            &mirrors,
        );
        assert_eq!(
            candidates,
            vec![
                "https://distfiles.gentoo.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
                "https://gentoo.osuosl.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_mirror_candidates_tries_custommirrors_before_thirdpartymirrors() {
        // Real "Try user-defined mirrors first" (fetch.py:1143).
        let mut custommirrors = HashMap::new();
        custommirrors.insert(
            "gentoo".to_string(),
            vec!["https://my-local-mirror.example/distfiles".to_string()],
        );
        let mut thirdpartymirrors = HashMap::new();
        thirdpartymirrors.insert(
            "gentoo".to_string(),
            vec!["https://distfiles.gentoo.org/distfiles".to_string()],
        );
        let candidates = resolve_mirror_candidates(
            "mirror://gentoo/app-arch/foo-1.0.tar.gz",
            &custommirrors,
            &thirdpartymirrors,
        );
        assert_eq!(
            candidates,
            vec![
                "https://my-local-mirror.example/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
                "https://distfiles.gentoo.org/distfiles/app-arch/foo-1.0.tar.gz".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_mirror_candidates_is_empty_for_an_unknown_mirror_name() {
        let candidates = resolve_mirror_candidates(
            "mirror://unknown/foo.tar.gz",
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_is_empty_for_a_malformed_mirror_uri() {
        let candidates =
            resolve_mirror_candidates("mirror://gentoo", &HashMap::new(), &HashMap::new());
        assert!(candidates.is_empty());
    }

    #[test]
    fn resolve_mirror_candidates_returns_a_non_mirror_uri_unchanged() {
        let candidates = resolve_mirror_candidates(
            "https://example.com/foo.tar.gz",
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            candidates,
            vec!["https://example.com/foo.tar.gz".to_string()]
        );
    }
}
