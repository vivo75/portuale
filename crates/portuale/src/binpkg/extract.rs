//! Extracting a binary package's image and metadata.

use crate::binpkg::*;

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
