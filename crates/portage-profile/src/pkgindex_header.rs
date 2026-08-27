//! The `<pkgdir>/Packages` index header portuale writes after building or
//! backing up a binpkg: Portage's full settings-derived key set, not just
//! `TIMESTAMP`/`VERSION`.
//!
//! Two snapshots, because Portage writes the header from two different
//! processes. A `--buildpkg`/`package` inject runs in the parent (`bintree.inject`
//! reads `self.settings`), while an `unmerge-backup` `quickpkg` runs in a
//! child that inherits the parent's resolved settings as its environment and
//! stacks them over the files again (`vartree._quickpkg_dblink`). The child
//! re-fold resurrects env-negated tokens (`sandbox` is back in its `FEATURES`)
//! and duplicates pruned ones (`ACCEPT_LICENSE: @FREE @FREE`).
//!
//! Scope cuts: `USE`/`ACCEPT_KEYWORDS`/`IUSE_IMPLICIT`/`USE_EXPAND*` reuse the
//! parent fold for the child (they differ only when the calling env negates
//! one of their tokens, which the per-layer sources below do not track);
//! `REPO_REVISIONS` (needs sync history) is never set; entry sorting stays
//! append order (the bed sorts stanzas before comparing).
// Portage: dbapi/bintree.py, _update_pkgindex_header; getbinpkg.py,
//   PackageIndex.write; package/ebuild/config.py, regenerate;
//   package/ebuild/_config/helper.py, prune_incremental

use crate::*;

/// The header key set for one writer, as `(key, value)` with `None`
/// meaning "ensure absent" (Portage pops falsy settings values on every
/// inject). Sorted by key, like Portage's `keys.sort()` write path.
pub type HeaderSnapshot = Vec<(String, Option<String>)>;

/// The two header snapshots: `parent` for the in-process `--buildpkg`
/// inject, `child` for the `quickpkg`-as-a-subprocess re-fold.
#[derive(Clone, Debug, Default)]
pub struct PkgIndexHeader {
    pub parent: HeaderSnapshot,
    pub child: HeaderSnapshot,
}

fn env_tokens(config: &Config, key: &str) -> Vec<String> {
    config
        .pkgindex_env_raw
        .get(key)
        .map(|v| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}
