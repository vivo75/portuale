//! md5-cache reading, the aux-metadata provider and pregen/regen switches.

use crate::*;

/// The hook `repo_aux_metadata` asks on a cache miss: the aux dict a
/// `depend` phase produced. A plain `fn` pointer, because the phase
/// runner lives in the binary crate, which cannot be referenced from here.
/// Portage: porttree.py's ebuild fallback.
pub type AuxMetadataProvider = fn(&Path, &str, &str) -> Result<HashMap<String, String>, String>;

/// Whether Portage would read this repo's pregenerated aux metadata from
/// `metadata/md5-cache`: the **first known** entry of
/// [`RepoConfig::cache_formats`] is `md5-dict`.
/// Portage: config.py, parse_layout + iter_pregenerated_caches +
/// get_pregenerated_cache.
///
/// Portuale has no `pms` reader (a documented narrowing -- slower, not
/// different: the depcachedir rung and `depend` phase produce the same
/// metadata), so "the first known format is `md5-dict`" is exactly
/// "portuale may read `metadata/md5-cache`".
pub fn pregen_md5_cache_enabled(repo: &RepoConfig) -> bool {
    repo.cache_formats
        .iter()
        .find(|f| f.as_str() == "md5-dict" || f.as_str() == "pms")
        .is_some_and(|f| f.as_str() == "md5-dict")
}
