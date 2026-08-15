//! binrepos.conf entries for `--getbinpkg` resolution.

use crate::*;

/// One `--getbinpkg` binary-package repository (only what `--pretend`
/// resolution needs; no fetch or download-gating config).
// Portage: binrepo/config.py, BinRepoConfig
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinRepo {
    /// The `[section]` name, or the `md5(uri)` hex-digest fallback for an
    /// implicit `PORTAGE_BINHOST` entry.
    pub name: String,
    /// `sync-uri` with any trailing `/` stripped.
    pub sync_uri: String,
    /// `priority` (`int(priority)`, `0` on parse failure or absence).
    pub priority: i32,
    /// `location`: explicit value or `None`. Display-only (`emerge --info`).
    pub location: Option<String>,
    /// `verify-signature` (default `true`).
    pub verify_signature: bool,
    /// `frozen` (default `false`): cached `Packages` used as-is, never
    /// refreshed.
    pub frozen: bool,
}

impl BinRepo {
    /// This binrepo's on-disk `Packages` index dir under `root`. A
    /// missing index means "no candidates" (`--pretend` never fetches).
    // Portage: bintree.py, _populate_remote
    pub fn packages_dir(&self, root: &std::path::Path) -> std::path::PathBuf {
        let uri = &self.sync_uri;
        if let Some(rest) = uri.strip_prefix("file://") {
            return std::path::PathBuf::from(rest);
        }
        for scheme in ["https://", "http://", "ssh://"] {
            if let Some(rest) = uri.strip_prefix(scheme) {
                let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
                let host = hostport.split(':').next().unwrap_or(hostport);
                return root
                    .join("var/cache/edb/binhost")
                    .join(host)
                    .join(path.trim_matches('/'));
            }
        }
        std::path::PathBuf::from(uri)
    }
}
