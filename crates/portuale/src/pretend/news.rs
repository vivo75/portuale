//! GLEP 42 news.

use crate::pretend::*;

/// The non-vdb restrictions plus the state-file ownership, built once per
/// run by [`news_context`].
/// Portage: portage/news.py, NewsManager
#[derive(Debug, Clone, Default)]
pub struct NewsContext {
    /// The profile path with the main repo's `<location>/profiles/` prefix
    /// removed (the full path when the profile lives elsewhere); `None`
    /// with no profile at all.
    pub profile: Option<String>,
    /// Whether a `make.profile` directory exists. It can be set while
    /// `profile` is `None` (no main repo).
    pub has_profile: bool,
    /// The `Display-If-Keyword` operand.
    pub arch: String,
    /// `PORTAGE_INST_UID` (default 0).
    pub inst_uid: u32,
    /// The host's `PORTAGE_GRPNAME` group (default `portage`), 0 when that
    /// group does not exist. Read from `/etc/group` only.
    pub portage_gid: u32,
}

/// Chown to `PORTAGE_INST_UID:portage_gid` when either differs (root caller
/// only), then OR the `mode` bits into the existing mode when any is
/// missing. `false` on a failed chown/chmod.
/// Portage: portage/util/__init__.py, apply_permissions
pub(crate) fn apply_news_permissions(path: &Path, context: &NewsContext, mode: u32) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    // SAFETY: `geteuid` has no preconditions.
    let is_root = unsafe { libc::geteuid() } == 0;
    if is_root
        && (meta.uid() != context.inst_uid || meta.gid() != context.portage_gid)
        && std::os::unix::fs::chown(path, Some(context.inst_uid), Some(context.portage_gid))
            .is_err()
    {
        return false;
    }
    let st_mode = meta.mode() & 0o7777;
    if st_mode & mode != mode
        && std::fs::set_permissions(path, std::fs::Permissions::from_mode(st_mode | mode)).is_err()
    {
        return false;
    }
    true
}
