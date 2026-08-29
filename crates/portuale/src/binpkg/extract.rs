//! Extracting a binary package's image and metadata.

use crate::binpkg::*;

/// The xpak `[image tarball]` prefix -> `dest`: the image is everything
/// before the `XPAKPACK…STOP` trailer.
// Portage: `xpak.tbz2.decompose`.
pub(crate) fn extract_xpak_image(
    binpkg_path: &Path,
    dest: &Path,
    xattrs: &crate::ebuild_merge::XattrPolicy,
) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut f =
        fs::File::open(binpkg_path).map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let file_len = f
        .seek(SeekFrom::End(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if file_len < 16 {
        return Err(format!(
            "{}: too small for an xpak binpkg",
            binpkg_path.display()
        ));
    }
    let mut trailer = [0u8; 16];
    f.seek(SeekFrom::End(-16))
        .and_then(|_| f.read_exact(&mut trailer))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    if &trailer[0..8] != b"XPAKSTOP" || &trailer[12..16] != b"STOP" {
        return Err(format!("{}: no XPAKSTOP trailer", binpkg_path.display()));
    }
    let infosize = be32(&trailer[8..12]) as u64;
    let image_len = file_len.checked_sub(infosize + 8).ok_or_else(|| {
        format!(
            "{}: xpak trailer larger than the file",
            binpkg_path.display()
        )
    })?;

    let scratch = ScratchDir::new("xpak-image")?;
    let image_tar = scratch.path().join("image.tar");
    f.seek(SeekFrom::Start(0))
        .map_err(|e| format!("{}: {e}", binpkg_path.display()))?;
    let mut out =
        fs::File::create(&image_tar).map_err(|e| format!("{}: {e}", image_tar.display()))?;
    std::io::copy(&mut f.take(image_len), &mut out)
        .map_err(|e| format!("{}: {e}", image_tar.display()))?;
    drop(out);
    // `tar -x` auto-detects gzip/bzip2/xz/zstd/... on read.
    let (tar, dest) = (lossy(&image_tar), lossy(dest));
    let mut args = vec!["-xpf", tar.as_str(), "-C", dest.as_str()];
    let excludes = xpak_xattr_tar_options(xattrs, tar_supports_xattrs());
    args.extend(excludes.iter().map(String::as_str));
    run_tar(&args)
}

/// Under `FEATURES=xattr`, when tar knows `--xattrs`, extract with
/// `--xattrs --xattrs-include='*'` plus one `--xattrs-exclude=<pat>` per
/// `PORTAGE_XATTR_EXCLUDE` pattern. Only the xpak image takes these (gpkg
/// neither stores nor restores xattrs).
// Portage: `BinpkgExtractorAsync._xpak_start`.
pub(crate) fn xpak_xattr_tar_options(
    xattrs: &crate::ebuild_merge::XattrPolicy,
    tar_knows_xattrs: bool,
) -> Vec<String> {
    if !xattrs.enabled || !tar_knows_xattrs {
        return Vec::new();
    }
    let mut opts = vec!["--xattrs".to_string(), "--xattrs-include=*".to_string()];
    opts.extend(
        xattrs
            .exclude
            .split_whitespace()
            .map(|pat| format!("--xattrs-exclude={pat}")),
    );
    opts
}

/// The `b"--xattrs" in $(gtar --help)` probe, run once per process.
pub(crate) fn tar_supports_xattrs() -> bool {
    use std::sync::OnceLock;
    static KNOWS: OnceLock<bool> = OnceLock::new();
    *KNOWS.get_or_init(|| {
        Command::new("tar")
            .arg("--help")
            .output()
            .map(|o| o.stdout.windows(8).any(|w| w == b"--xattrs"))
            .unwrap_or(false)
    })
}
