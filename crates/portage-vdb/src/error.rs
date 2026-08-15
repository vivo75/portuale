//! The crate's error type.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Every failure of an [`InstalledDb`](crate::InstalledDb) or [`WriteTxn`](crate::WriteTxn).
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An I/O error on `path`; displays as `"<path>: <error>"`.
    Io { path: PathBuf, source: io::Error },
    /// The backend does not implement this operation (yet).
    Unsupported(String),
    /// A bad argument, such as an `aux_get` key outside the 23 fields.
    Invalid(String),
    /// The stored data cannot be read as the backend's format.
    Corrupt(String),
    /// A database engine failure; the text carries the file and message.
    Backend(String),
    /// The database file is held open by another handle. Only redb has
    /// this: one process (one handle) at a time, so a second user waits.
    Busy { path: PathBuf },
}

impl Error {
    /// [`Error::Io`] for `path`.
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::Invalid(what) => write!(f, "invalid argument: {what}"),
            Error::Corrupt(what) => write!(f, "corrupt VDB: {what}"),
            Error::Backend(what) => write!(f, "database error: {what}"),
            Error::Busy { path } => write!(
                f,
                "{}: database is already open (redb allows only one process to use \
                 the file at a time; wait for the other user to finish, or stop it)",
                path.display()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
