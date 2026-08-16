// Rust port of Portage's `portage.dep.check_required_use`: the
// REQUIRED_USE (PMS 7.3.4/8.2) evaluator.
//
// A leaf is `flag` or `!flag`; a group is a bare all-of (`( item+ )`,
// the top level itself is one), an any-of (`||`), an exactly-one-of
// (`^^`), an at-most-one-of (`??`), or a use-conditional
// (`[!]flag? ( item+ )`). No `=`/`!=`/`?=` forms exist here (those are
// USE-dep, atom-only forms).
//
// [`check_required_use`] evaluates directly; [`unsatisfied_reduced`]
// additionally builds the still-unsatisfied subtree Portage's depgraph
// prints. Not EAPI-parametrized: `??` and empty-group handling follow
// EAPI 7+ rules unconditionally. A referenced flag missing from IUSE is
// an error.
// Portage: lib/portage/dep/__init__.py, check_required_use

use std::collections::HashSet;

/// Error evaluating a REQUIRED_USE string. `Display` reproduces Portage's
/// messages byte-for-byte.
// Portage: lib/portage/dep/__init__.py, is_active
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// `USE flag '{flag}' is not in IUSE`
    UndeclaredFlag { flag: String },
    /// `malformed syntax: operator/conditional not followed by '('`
    MissingOpenParen,
    /// `malformed syntax: unbalanced parentheses`
    UnbalancedParens,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UndeclaredFlag { flag } => write!(f, "USE flag '{flag}' is not in IUSE"),
            Error::MissingOpenParen => {
                write!(
                    f,
                    "malformed syntax: operator/conditional not followed by '('"
                )
            }
            Error::UnbalancedParens => write!(f, "malformed syntax: unbalanced parentheses"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

/// Whether a REQUIRED_USE leaf token (`flag`, or `!flag` for negation)
/// is satisfied against `enabled` (effective USE) and `iuse` (declared
/// IUSE). `Err` if the flag is not a declared IUSE flag.
fn is_active(
    token: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<bool, Error> {
    let (flag, negated) = match token.strip_prefix('!') {
        Some(rest) => (rest, true),
        None => (token, false),
    };
    if flag.is_empty() || !iuse.contains(flag) {
        return Err(Error::UndeclaredFlag {
            flag: flag.to_string(),
        });
    }
    Ok(enabled.contains(flag) != negated)
}
