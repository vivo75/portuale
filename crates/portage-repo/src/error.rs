//! The crate's error type.

/// Error from repo/config/metadata/resolver handling. Variants mirror
/// Portage's `lib/portage` message shapes so `Display` reproduces the
/// CLI-visible strings (pinned by the contract suite).
#[derive(Debug)]
pub enum Error {
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    /// `{path}: stale metadata/md5-cache entry` -- rejected like Portage's
    /// `_pull_valid_cache` (stale `_md5_`/`_eclasses_`, unsupported EAPI).
    StaleMd5Cache { path: String },
    /// `{path}: no usable pregenerated cache` -- falls through to the
    /// depcachedir and `doebuild(mydo="depend")` like Portage.
    PregenCacheUnavailable { path: String },
    ReadEntry { source: std::io::Error },
    NoReposConf { path: String },
    NoMainRepo,
    NoRepoLocation { repo: String },
    InvalidAtom { atom: String },
    LicenseExpectedDepString { token: usize },
    LicenseExpectedOpen { token: usize },
    LicenseNoMatchingOpen { token: usize },
    LicenseExpectedOpenGotOr { token: usize },
    LicenseExpectedOpenGotToken { token: String, at: usize },
    LicenseMissingCloseParen,
    LicenseMissingOpenParen,
    /// `REQUIRED_USE for {package} is invalid: {source}`.
    InvalidRequiredUse {
        package: String,
        source: portage_required_use::Error,
    },
    /// An already-fully-formatted, multi-line message (`Detail`).
    Detail(String),
    /// Errors bubbled up from `portage_use_reduce` / `portage_fetch`.
    UseReduce(portage_use_reduce::Error),
    Fetch(portage_fetch::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ReadFile { path, source } => write!(f, "reading {path}: {source}"),
            Error::StaleMd5Cache { path } => {
                write!(f, "{path}: stale metadata/md5-cache entry")
            }
            Error::PregenCacheUnavailable { path } => {
                write!(f, "{path}: no usable pregenerated cache")
            }
            Error::ReadEntry { source } => write!(f, "{source}"),
            Error::NoReposConf { path } => write!(f, "no repos.conf found at {path}"),
            Error::NoMainRepo => write!(f, "no [DEFAULT] main-repo in repos.conf"),
            Error::NoRepoLocation { repo } => {
                write!(f, "no location for repo {repo:?} in repos.conf")
            }
            Error::InvalidAtom { atom } => write!(f, "invalid atom {atom:?}"),
            Error::LicenseExpectedDepString { token } => {
                write!(f, "expected: dependency string, got: ')', token {token}")
            }
            Error::LicenseExpectedOpen { token } => {
                write!(f, "expected: '(', got: ')', token {token}")
            }
            Error::LicenseNoMatchingOpen { token } => {
                write!(f, "no matching '(' for ')', token {token}")
            }
            Error::LicenseExpectedOpenGotOr { token } => {
                write!(f, "expected: '(', got: '||', token {token}")
            }
            Error::LicenseExpectedOpenGotToken { token, at } => {
                write!(f, "expected: '(', got: '{token}', token {at}")
            }
            Error::LicenseMissingCloseParen => write!(f, "Missing ')' at end of string"),
            Error::LicenseMissingOpenParen => write!(f, "Missing '(' at end of string"),
            Error::InvalidRequiredUse { package, source } => {
                write!(f, "REQUIRED_USE for {package} is invalid: {source}")
            }
            Error::Detail(message) => write!(f, "{message}"),
            Error::UseReduce(e) => write!(f, "{e}"),
            Error::Fetch(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

impl From<portage_use_reduce::Error> for Error {
    fn from(e: portage_use_reduce::Error) -> Error {
        Error::UseReduce(e)
    }
}

impl From<portage_fetch::Error> for Error {
    fn from(e: portage_fetch::Error) -> Error {
        Error::Fetch(e)
    }
}
