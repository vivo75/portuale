//! The crate's error type.

/// Error from repo/config/metadata/resolver handling. Distinct variants
/// mirror the real `lib/portage` error message shapes so `Display`
/// reproduces them byte-for-byte (the contract suite pins the
/// CLI-visible strings). `Detail` carries an already-fully-formatted
/// message whose assembly spans multiple conditional appends upstream
/// (real resolver conflict/`REQUIRED_USE`-violation reporting).
#[derive(Debug)]
pub enum Error {
    /// `reading {path}: {source}` -- an `io::Error` on a repo/config file.
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    /// `{path}: stale metadata/md5-cache entry` -- an entry real's
    /// `_pull_valid_cache` rejects (stale `_md5_`/`_eclasses_` or an
    /// unsupported EAPI) that no provider could replace: a nested read
    /// from inside the depend phase, or a provider whose phase failed.
    StaleMd5Cache { path: String },
    /// `{path}: no usable pregenerated cache` -- the repo's first known
    /// `cache-formats` entry is not `md5-dict` (#55 L1: no `pms` reader)
    /// or `FEATURES` names `metadata-transfer` (#55 L2), and no
    /// depend-phase provider could supply the metadata the pregen cache
    /// would have. The pregen rung is skipped exactly like real
    /// `_pull_valid_cache` falling through its rungs to the depcachedir
    /// and `doebuild(mydo="depend")`.
    PregenCacheUnavailable { path: String },
    /// `{source}` -- an `io::Error` reading a single directory entry.
    ReadEntry { source: std::io::Error },
    /// `no repos.conf found at {path}`
    NoReposConf { path: String },
    /// `no [DEFAULT] main-repo in repos.conf`
    NoMainRepo,
    /// `no location for repo {repo:?} in repos.conf`
    NoRepoLocation { repo: String },
    /// `invalid atom {atom:?}`
    InvalidAtom { atom: String },
    /// `expected: dependency string, got: ')', token {token}`
    LicenseExpectedDepString { token: usize },
    /// `expected: '(', got: ')', token {token}`
    LicenseExpectedOpen { token: usize },
    /// `no matching '(' for ')', token {token}`
    LicenseNoMatchingOpen { token: usize },
    /// `expected: '(', got: '||', token {token}`
    LicenseExpectedOpenGotOr { token: usize },
    /// `expected: '(', got: '{token}', token {at}`
    LicenseExpectedOpenGotToken { token: String, at: usize },
    /// `Missing ')' at end of string`
    LicenseMissingCloseParen,
    /// `Missing '(' at end of string`
    LicenseMissingOpenParen,
    /// `REQUIRED_USE for {package} is invalid: {source}` where `package`
    /// is the `category/package-version` prefix and `source` the
    /// `portage_required_use::Error` `Display`.
    InvalidRequiredUse {
        package: String,
        source: portage_required_use::Error,
    },
    /// An already-fully-formatted, multi-line message (`Detail`).
    Detail(String),
    /// An error bubbled up from `portage_use_reduce`.
    UseReduce(portage_use_reduce::Error),
    /// An error bubbled up from `portage_fetch`.
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
