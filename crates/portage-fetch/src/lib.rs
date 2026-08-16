// Portage SRC_URI flattening, Manifest digest parsing, and content-digest
// verification -- the non-network half of fetching a package's sources
// (`portuale/src/fetch.rs` holds the network half, the `wget` subprocess
// invocation this crate deliberately has nothing to do with, so its logic
// stays 100% testable offline).
//
// SRC_URI grammar (PMS 3.1.6, plus Portage's `mirror://`, `mirror+` and
// `fetch+` extensions): whitespace-separated URIs with optional `-> name`
// renames, under recursively nested `flag? ( ... )` / `!flag? ( ... )`
// groups. No `||` groups (PMS 8.2.6.5). See `SrcUriEntry` and `layout`
// for how a mirror root becomes a file URL.
//
// Manifest2 `DIST` lines (`lib/portage/manifest.py`), verified via the
// standard BLAKE2b-512 and SHA-512 algorithms. Known v1 scope cuts (see
// notes): deterministic (unshuffled) `mirror://` candidates, no
// `custommirrors["local"]` fast path, only `BLAKE2B`/`SHA512` verified,
// `DIST` lines only.

pub mod mirror_cache;

use std::collections::HashMap;
use std::path::Path;

/// Error from Manifest/SRC_URI/digest handling. The variants mirror
/// Portage `lib/portage/manifest.py` / `fetch.py` message shapes so
/// `Display` reproduces them byte-for-byte (the contract suite pins them).
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

/// One flattened `SRC_URI` entry: the remote URI, and the local filename
/// it should be saved as (the `->` rename, or the URI's own basename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrcUriEntry {
    pub uri: String,
    pub filename: String,
    /// Portage `override_mirror` (`fetch.py`): this URI token had a
    /// `mirror+` prefix, re-permitting the public mirror list even under
    /// `RESTRICT=mirror`. Implies `override_fetch` too.
    pub override_mirror: bool,
    /// Portage `override_fetch` (`fetch.py`): `mirror+` or `fetch+`
    /// prefix, exempting this one URI under `RESTRICT=fetch`. (See notes.)
    pub override_fetch: bool,
}
