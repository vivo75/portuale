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
mod inner;
mod manifest;
mod metadata;
mod outer;

#[cfg(test)]
mod tests;

pub use digest::*;
pub use extract::*;
pub use gpg::*;
pub(crate) use inner::*;
pub(crate) use manifest::*;
pub use metadata::*;
pub(crate) use outer::*;

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
    /// `true` for the skip class.
    pub fn is_invalid(&self) -> bool {
        matches!(self, BinpkgError::Invalid(_))
    }

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

/// The `.tar<ext>` suffixes map to a compression method, paired with
/// that method's decompress argv (`{JOBS}` -> `0` means all cores).
// Portage: `gpkg.gpkg.ext_list`, `util/compression_probe._compressors`.
fn gpkg_compressions() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        (".gz", &["gzip", "-dc"]),
        (".bz2", &["bzip2", "-dc"]),
        (".lz4", &["lz4", "-dc"]),
        (".lz", &["lzip", "-dc"]),
        (".lzo", &["lzop", "-dc"]),
        (".xz", &["xz", "-T0", "-dc"]),
        (".zst", &["zstd", "-dc", "--long=31"]),
    ]
}

/// Given an inner member's basename, return `Some(None)` for exactly
/// `<want>.tar`, `Some(Some(decompress_argv))` for `<want>.tar<ext>`
/// with a known `ext`, or `None` if it names something else.
// Portage: `gpkg._extract_filename_compression`.
fn classify_inner_member(
    want: &str,
    member_basename: &str,
) -> Option<Option<&'static [&'static str]>> {
    let plain = format!("{want}.tar");
    if member_basename == plain {
        return Some(None);
    }
    for (ext, argv) in gpkg_compressions() {
        if member_basename == format!("{plain}{ext}") {
            return Some(Some(*argv));
        }
    }
    None
}

/// A best-effort temp directory, removed when the guard drops. The
/// `nanos` suffix keeps concurrent reads from colliding.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(tag: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!(
            "portuale-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
