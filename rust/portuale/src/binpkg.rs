// Reading a real binary package's own embedded metadata -- the piece
// this pilot's `Packages`-index reader (`portage_repo::read_packages_
// index`) deliberately never needed, since the index alone carries every
// field a `--pretend` binary candidate uses. This module is increment 1
// of the "$PKGDIR directory-scan fallback" buildout (see
// `PORTING/PROMPT-next.md`): when `$PKGDIR` has binpkg *files* but no
// `Packages` index at all, real `bintree._populate_local` opens each
// file and rebuilds the index -- which needs a real per-format metadata
// reader. This is the `gpkg` half.
//
// It shells out to `tar` (and the matching decompressor) rather than
// parsing the archive natively or pulling a Rust tar/compression crate:
// consistent with this pilot's own "real, unmodified system tools where
// they exist" stance for every other real-execution path (`wget`,
// `ldconfig`, `scanelf`, `bash`/`brush`, the compressors already invoked
// by `ebuild_package.rs`), and `tar` + these compressors are hard Gentoo
// requirements anyway (real `gpkg.py` is built on Python's `tarfile` +
// the exact same compressor subprocesses).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Real `portage.gpkg.gpkg.get_metadata()` / `unpack_metadata(want=None)`
/// (`lib/portage/gpkg.py:838-870`), narrowed to the local metadata read:
/// a `.gpkg.tar` is a plain (uncompressed) tar whose members are
/// `<basename>/{gpkg-1, metadata.tar[.<comp>], image.tar[.<comp>],
/// Manifest}` (+ optional `.sig` files). Returns the `metadata/<KEY>` ->
/// value map -- real `_strip_metadata_prefix` over the inner
/// `metadata.tar`'s own members -- each value UTF-8 with surrounding
/// whitespace trimmed (the same shape a vdb aux file / `read_md5_cache`
/// entry already has). A member whose bytes aren't valid UTF-8
/// (`environment.bz2`) is skipped, not an error.
///
/// **v1 cuts, documented** (matching this pilot's own `Packages`-index
/// reader, which "trusts the index outright" -- real
/// `FEATURES=pkgdir-index-trusted`): NO `Manifest` digest verification
/// and NO GPG `.sig` signature check (real `gpkg._verify_binpkg`). Those
/// are gpkg's whole reason to exist, but this pilot has no crypto
/// anywhere and its `--pretend` binary path has never verified a
/// binpkg's integrity -- a real, separately-scoped follow-up. The
/// `gpkg-1` version marker's *presence* is still required (real
/// `_get_inner_tarinfo`'s own `InvalidBinaryPackageFormat` guard).
pub fn read_gpkg_metadata(gpkg_path: &Path) -> Result<HashMap<String, String>, String> {
    if !gpkg_path.is_file() {
        return Err(format!("{}: not a file", gpkg_path.display()));
    }
    let scratch = ScratchDir::new("gpkg")?;
    let outer = scratch.path().join("outer");
    fs::create_dir_all(&outer).map_err(|e| format!("{}: {e}", outer.display()))?;

    // 1. Unpack the outer container (plain tar).
    run_tar(&["-xf", &lossy(gpkg_path), "-C", &lossy(&outer)])?;

    // 2. Locate `<basename>/gpkg-1` (real validity guard) and the
    //    `metadata.tar[.<comp>]` member.
    let mut gpkg_marker = false;
    let mut metadata_member: Option<(PathBuf, Option<&'static [&'static str]>)> = None;
    for basename_dir in read_dir_sorted(&outer)? {
        if !basename_dir.is_dir() {
            if basename_dir.file_name().and_then(|n| n.to_str()) == Some("gpkg-1") {
                gpkg_marker = true;
            }
            continue;
        }
        for member in read_dir_sorted(&basename_dir)? {
            let Some(name) = member.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == "gpkg-1" {
                gpkg_marker = true;
            }
            if let Some(comp) = classify_inner_member("metadata", name) {
                metadata_member.get_or_insert((member.clone(), comp));
            }
        }
    }
    if !gpkg_marker {
        return Err(format!(
            "{}: not a gpkg container (no `gpkg-1` version marker)",
            gpkg_path.display()
        ));
    }
    let (metadata_member, comp) = metadata_member.ok_or_else(|| {
        format!(
            "{}: no `metadata.tar` member in the gpkg",
            gpkg_path.display()
        )
    })?;

    // 3. Reduce the inner member to a plain `metadata.tar`.
    let inner_tar = scratch.path().join("metadata.tar");
    match comp {
        None => {
            fs::copy(&metadata_member, &inner_tar)
                .map_err(|e| format!("{}: {e}", metadata_member.display()))?;
        }
        Some(argv) => {
            let out = fs::File::create(&inner_tar)
                .map_err(|e| format!("{}: {e}", inner_tar.display()))?;
            let status = Command::new(argv[0])
                .args(&argv[1..])
                .arg(&metadata_member)
                .stdout(out)
                .status()
                .map_err(|e| format!("failed to spawn {}: {e}", argv[0]))?;
            if !status.success() {
                return Err(format!(
                    "{} failed to decompress {} ({status})",
                    argv[0],
                    metadata_member.display()
                ));
            }
        }
    }

    // 4. Unpack the inner `metadata.tar` (members are `metadata/<KEY>`).
    let md = scratch.path().join("md");
    fs::create_dir_all(&md).map_err(|e| format!("{}: {e}", md.display()))?;
    run_tar(&["-xf", &lossy(&inner_tar), "-C", &lossy(&md)])?;

    // 5. Read every `metadata/<KEY>` scalar file.
    let metadata_dir = md.join("metadata");
    let mut out = HashMap::new();
    for f in read_dir_sorted(&metadata_dir)? {
        if !f.is_file() {
            continue;
        }
        let Some(key) = f.file_name().and_then(|n| n.to_str()).map(String::from) else {
            continue;
        };
        let Ok(bytes) = fs::read(&f) else { continue };
        let Ok(text) = String::from_utf8(bytes) else {
            continue; // e.g. environment.bz2 -- not a scalar value
        };
        out.insert(key, text.trim().to_string());
    }
    Ok(out)
}

fn run_tar(args: &[&str]) -> Result<(), String> {
    let status = Command::new("tar")
        .args(args)
        .status()
        .map_err(|e| format!("failed to spawn tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar {args:?} failed ({status})"));
    }
    Ok(())
}

fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    #[test]
    fn classify_inner_member_matches_real_extract_filename_compression() {
        assert!(matches!(
            classify_inner_member("metadata", "metadata.tar"),
            Some(None)
        ));
        assert!(matches!(
            classify_inner_member("metadata", "metadata.tar.zst"),
            Some(Some(_))
        ));
        assert!(matches!(
            classify_inner_member("metadata", "metadata.tar.xz"),
            Some(Some(_))
        ));
        assert!(classify_inner_member("metadata", "metadata.tar.zst.sig").is_none());
        assert!(classify_inner_member("metadata", "image.tar.zst").is_none());
        assert!(classify_inner_member("metadata", "Manifest").is_none());
    }

    #[test]
    fn read_gpkg_metadata_extracts_the_embedded_scalar_metadata() {
        // A real, hand-built `.gpkg.tar` (real `tar` + real `zstd`) --
        // outer plain-tar container, `gpkg-1` marker, zstd-compressed
        // inner `metadata.tar` with `metadata/<KEY>` files.
        let m = read_gpkg_metadata(&fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"))
            .expect("the fixture gpkg reads");
        assert_eq!(m.get("EAPI").map(String::as_str), Some("8"));
        assert_eq!(m.get("SLOT").map(String::as_str), Some("0"));
        assert_eq!(m.get("KEYWORDS").map(String::as_str), Some("amd64"));
        assert_eq!(m.get("IUSE").map(String::as_str), Some("grfoo"));
        assert_eq!(m.get("USE").map(String::as_str), Some(""));
        assert_eq!(m.get("DEPEND").map(String::as_str), Some("dev-libs/newpkg"));
        assert_eq!(
            m.get("RDEPEND").map(String::as_str),
            Some("dev-libs/newpkg")
        );
        assert_eq!(m.get("CATEGORY").map(String::as_str), Some("dev-libs"));
        assert_eq!(m.get("PF").map(String::as_str), Some("gpkgreadpkg-1.0"));
        assert_eq!(m.get("repository").map(String::as_str), Some("gentoo"));
    }

    #[test]
    fn read_gpkg_metadata_rejects_a_non_gpkg_tar() {
        // A plain tar with no `gpkg-1` marker anywhere.
        let scratch = ScratchDir::new("gpkg-negtest").unwrap();
        let junk = scratch.path().join("a.txt");
        fs::write(&junk, b"x").unwrap();
        let not_gpkg = scratch.path().join("plain.tar");
        run_tar(&[
            "-cf",
            &lossy(&not_gpkg),
            "-C",
            &lossy(scratch.path()),
            "a.txt",
        ])
        .unwrap();
        let err = read_gpkg_metadata(&not_gpkg).unwrap_err();
        assert!(err.contains("gpkg-1"), "{err}");
    }
}
