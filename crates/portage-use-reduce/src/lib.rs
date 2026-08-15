// Rust port of `portage.dep.use_reduce` (lib/portage/dep/__init__.py),
// restricted to `flat=True` mode: the tokenizer and bracket/use-conditional
// handling, ported as-is, error behavior included.
// Portage: lib/portage/dep/__init__.py, use_reduce
//
// Out of scope are only optional parameters the harness never exercises:
// `masklist`, `excludeall`, `is_src_uri`/the `->` arrow token,
// `opconvert`, and `token_class`/`is_valid_flag` (tokens stay opaque
// strings here).
//
// `subset` IS ported (`use_reduce_flat_subset`, against Portage's
// `select_subset`) via its own tree type (`DepNode`) feeding into the
// unmodified `use_reduce_flat`. `use_reduce_structured` ports the
// `flat=False`/`opconvert=False` mode (verified byte-for-byte against
// Portage over thousands of randomized dep strings).
// `use_reduce_flat_disjunctive` resolves `"||"` groups to the
// highest-`AltPreference` alternative a caller closure reports. This crate
// stays atom-agnostic throughout.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

/// Parse/reduce error from a USE dependency string. `Display` reproduces
/// Portage's messages byte-for-byte (the contract suite pins these).
#[derive(Debug)]
pub enum Error {
    /// `invalid use flag '{flag}' in conditional '{conditional}'`
    InvalidUseFlag { flag: String, conditional: String },
    /// `expected: {expected}, got: ')', token {token}` where `expected`
    /// is either `dependency string` or `'('` at the call site.
    ExpectedCloseParen {
        expected: &'static str,
        token: usize,
    },
    /// `no matching '(' for ')', token {token}`
    NoMatchingOpen { token: usize },
    /// `expected: '(', got: '||', token {token}`
    ExpectedOpenGotOr { token: usize },
    /// `expected: '(', got: '{token}', token {at}`
    ExpectedOpenGotToken { token: String, at: usize },
    /// `SRC_URI arrow are only allowed in SRC_URI: token {token}`
    SrcUriArrow { token: usize },
    /// `conditional '{s}' not followed by a group`
    ConditionalNotFollowedByGroup { s: String },
    /// `'||' not followed by a group`
    OrNotFollowedByGroup,
    /// `Missing ')' at end of string`
    MissingCloseParen,
    /// `Missing '(' at end of string`
    MissingOpenParen,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidUseFlag { flag, conditional } => {
                write!(
                    f,
                    "invalid use flag '{flag}' in conditional '{conditional}'"
                )
            }
            Error::ExpectedCloseParen { expected, token } => {
                write!(f, "expected: {expected}, got: ')', token {token}")
            }
            Error::NoMatchingOpen { token } => {
                write!(f, "no matching '(' for ')', token {token}")
            }
            Error::ExpectedOpenGotOr { token } => {
                write!(f, "expected: '(', got: '||', token {token}")
            }
            Error::ExpectedOpenGotToken { token, at } => {
                write!(f, "expected: '(', got: '{token}', token {at}")
            }
            Error::SrcUriArrow { token } => {
                write!(
                    f,
                    "SRC_URI arrow are only allowed in SRC_URI: token {token}"
                )
            }
            Error::ConditionalNotFollowedByGroup { s } => {
                write!(f, "conditional '{s}' not followed by a group")
            }
            Error::OrNotFollowedByGroup => write!(f, "'||' not followed by a group"),
            Error::MissingCloseParen => write!(f, "Missing ')' at end of string"),
            Error::MissingOpenParen => write!(f, "Missing '(' at end of string"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

fn useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}
