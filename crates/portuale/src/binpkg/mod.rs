// The `$PKGDIR` directory-scan fallback (Portage: `bintree._populate_local`).
// Every other binary-package path here is `<pkgdir>/Packages`-index driven,
// so a `gpkg`/`xpak` *listed in an index* already resolves. When `$PKGDIR`
// holds binpkg *files* but no `Packages`, each file's own embedded metadata
// is read instead, with a per-format reader (`read_gpkg_metadata` for
// `.gpkg.tar`, `read_xpak_metadata` for `.tbz2`/`.xpak`). The scan output
// becomes `portage_profile::Config::scanned_binpkgs`, never written back
// to `Packages`.

use portage_util::TempDir;

mod digest;
mod extract;
mod gpg;

#[cfg(test)]
mod tests;

pub use digest::*;
pub use extract::*;
pub use gpg::*;

use std::collections::HashMap;

use std::fs;

use std::io::Read;

use std::path::{Path, PathBuf};

use std::process::{Command, Stdio};

/// Why reading one binpkg file failed: `Invalid` (not a valid/usable
/// binpkg; the scan warns and skips the file) or `Fatal` (the read
/// itself failed; the scan aborts).
// Portage: `bintree._populate_local`'s caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinpkgError {
    /// The scan warns and drops the file.
    Invalid(String),
    /// The scan stops with the message.
    Fatal(String),
}

impl BinpkgError {
    /// The message, without the class.
    pub fn message(&self) -> &str {
        match self {
            BinpkgError::Invalid(message) | BinpkgError::Fatal(message) => message,
        }
    }
}

impl std::fmt::Display for BinpkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Existing `Result<_, String>` callers (the merge path, `--info`) keep
/// working through `?`; only the populate scan inspects the class.
impl From<BinpkgError> for String {
    fn from(error: BinpkgError) -> String {
        error.to_string()
    }
}

/// A best-effort temp directory, removed when the guard drops. The
/// `nanos` suffix keeps concurrent reads from colliding.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
