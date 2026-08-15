//! Merge options and the xattr policy.

use crate::ebuild_merge::*;

/// A merged file's extended attributes: same-device rename keeps every
/// xattr; cross-device copy carries them only under `FEATURES=xattr`,
/// minus `PORTAGE_XATTR_EXCLUDE`.
/// Portage: lib/portage/util/movefile.py, movefile
#[derive(Debug, Clone, Default)]
pub(crate) struct XattrPolicy {
    /// Whether `FEATURES` carries `xattr`.
    pub enabled: bool,
    /// `PORTAGE_XATTR_EXCLUDE` patterns.
    pub exclude: String,
}
