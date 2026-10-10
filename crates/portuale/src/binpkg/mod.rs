// The `$PKGDIR` directory-scan fallback -- real `bintree._populate_local`
// (see `LLM/agent-context.md`). Every other binary-package path in this
// portuale is `<pkgdir>/Packages`-index driven and format-agnostic, so a
// `gpkg`/`xpak` *listed in an index* already resolves for `--pretend`.
// What the index reader can't do is the "no trusted index" branch: when
// `$PKGDIR` holds binpkg *files* but no `Packages`, open each file, read
// its own embedded metadata, and build the pool from that. That needs a
// real per-format reader -- this module has both:
//
//   - `read_gpkg_metadata`: real `gpkg.get_metadata()` -- a `.gpkg.tar`
//     is a plain tar container; find/decompress the inner `metadata.tar`
//     and read each `metadata/<KEY>` member. The inner tar is read **in
//     process** with the `tar` crate (`#58`, K0): a member is judged
//     from its own header -- regular/unknown type -> bytes, symlink/
//     hardlink -> the target member resolved inside the archive, and
//     anything else is an error -- exactly real `tarfile.extractfile`,
//     so no symlink is followed and no FIFO/device is opened before the
//     member is accepted. The outer container and the `image.tar` unpack
//     still shell out to `tar` (the compressors stay subprocesses), the
//     same "shell out to the real tool" stance every other real-
//     execution path here takes (`wget`/`ldconfig`/`scanelf`/`bash`/
//     `brush`/the compressors `ebuild_package.rs` already runs); `tar`
//     and these compressors are hard Gentoo requirements anyway.
//   - `read_xpak_metadata`: real `xpak.tbz2.scan` -- the self-describing
//     `XPAKPACK…XPAKSTOP…STOP` trailer appended after the image tarball.
//     Pure Rust, no subprocess, reads only the bounded file tail.
//   - `populate_local_pkgdir`: walks `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}`
//     and synthesizes one `Packages`-style entry per file, fast-pathed
//     against any already-parsed `<pkgdir>/Packages` entry whose own
//     `_mtime_`/`SIZE` still agree with the live file (real's own
//     mtime-staleness revalidation -- see its own doc comment). Its
//     output becomes `portage_profile::Config::scanned_binpkgs` (NOT
//     written back to `Packages` -- portuale recomputes each run, so
//     `--pretend` still writes nothing).

use portage_util::TempDir;

mod digest;
mod extract;
mod gpg;
mod inner;
mod local_index;
mod manifest;
mod metadata;
mod outer;

#[cfg(test)]
mod tests;

pub use digest::*;
pub use extract::*;
pub use gpg::*;
pub(crate) use inner::*;
pub use local_index::*;
pub(crate) use manifest::*;
pub use metadata::*;
pub(crate) use outer::*;

use std::collections::HashMap;

use std::fs;

use std::io::Read;

use std::path::{Path, PathBuf};

use std::process::{Command, Stdio};

/// Why reading one binpkg file failed, in the two classes real's
/// `bintree._populate_local` caller distinguishes (`bintree.py:1185`):
///
/// - `Invalid`: the file is readable but is not a valid/usable binpkg --
///   real's `(PortagePackageException, SignatureException)` catch. The
///   `$PKGDIR` scan prints real's own
///   `!!! Invalid binary package: '<path>', <msg>` and skips the file
///   (`populate_local_pkgdir`).
/// - `Fatal`: the read itself failed -- real lets these propagate (a
///   `KeyError`/`AttributeError`/`RecursionError` from `tarfile`, an
///   inner `tarfile.ReadError`, an `OSError`). The scan aborts.
///
/// The classification mirrors real's exception families site by site;
/// the per-site table is `LLM/07.60-gpkg-populate-read-errors.opus.md`
/// section 4. Every `.xpak` read error is `Fatal`: real's
/// `tbz2.get_data()` returns `None` on any scan failure and the caller
/// then raises `AttributeError` on `None.get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinpkgError {
    /// Real's skip class: warn (real's own message) and drop the file.
    Invalid(String),
    /// Real's abort class: the scan stops with the message.
    Fatal(String),
}

impl BinpkgError {
    /// `true` for real's skip class.
    pub fn is_invalid(&self) -> bool {
        matches!(self, BinpkgError::Invalid(_))
    }

    /// `true` for real's abort class.
    pub fn is_fatal(&self) -> bool {
        matches!(self, BinpkgError::Fatal(_))
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

/// The `.tar<ext>` suffixes real `gpkg.gpkg.ext_list`
/// (`lib/portage/gpkg.py:821-829`) maps to a compression method, paired
/// with that method's own real `_compressors` decompress argv
/// (`lib/portage/util/compression_probe.py:10-53`; `{JOBS}` -> `0` =
/// "all cores", real's own substitution).
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

/// Real `gpkg._extract_filename_compression` (`gpkg.py:2176`): given an
/// inner member's basename, return `Some(None)` if it is exactly
/// `<want>.tar`, `Some(Some(decompress_argv))` if it is
/// `<want>.tar<ext>` for a known `ext`, or `None` if it names something
/// else.
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
/// `nanos` suffix keeps concurrent reads (e.g. a `$PKGDIR` scan over
/// many files) from colliding -- same shape `ebuild_phases`/`fetch`
/// already use for their own scratch dirs.
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
