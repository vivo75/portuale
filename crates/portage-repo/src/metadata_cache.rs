//! md5-cache reading, the aux-metadata provider and pregen/regen switches.

use crate::*;

/// The hook `repo_aux_metadata` asks on a cache miss: the aux dict a
/// `depend` phase produced. A plain `fn` pointer, because the phase
/// runner lives in the binary crate, which cannot be referenced from here.
/// Portage: porttree.py's ebuild fallback.
pub type AuxMetadataProvider = fn(&Path, &str, &str) -> Result<HashMap<String, String>, String>;

pub(crate) static AUX_METADATA_PROVIDER: OnceLock<AuxMetadataProvider> = OnceLock::new();

/// The fallback shared by a missing entry and an entry the validator
/// rejects: fallback memo, recursion guard, provider. A provider failure
/// always surfaces as the original `failure` (never stale data), which
/// reads as an unusable entry for the caller.
pub(crate) fn provide_aux_metadata(
    repo_location: &Path,
    category: &str,
    pf: &str,
    failure: Error,
) -> Result<std::sync::Arc<HashMap<String, String>>, Error> {
    {
        {
            let Some(provider) = AUX_METADATA_PROVIDER.get() else {
                return Err(failure);
            };
            type FallbackCache =
                HashMap<(PathBuf, String, String), std::sync::Arc<HashMap<String, String>>>;
            static FALLBACK: OnceLock<RwLock<FallbackCache>> = OnceLock::new();
            let cache = FALLBACK.get_or_init(|| RwLock::new(HashMap::new()));
            let key = (
                repo_location.to_path_buf(),
                category.to_string(),
                pf.to_string(),
            );
            // The provider itself runs the depend phase, whose env assembly
            // reads through this same entry point -- a nested miss must not
            // recurse into the provider again (it would never terminate).
            thread_local! {
                static IN_PROVIDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
            }
            if IN_PROVIDER.with(|flag| flag.get()) {
                return Err(failure);
            }
            if let Ok(guard) = cache.read()
                && let Some(map) = guard.get(&key)
            {
                return Ok(std::sync::Arc::clone(map));
            }
            struct InProviderGuard;
            impl Drop for InProviderGuard {
                fn drop(&mut self) {
                    IN_PROVIDER.with(|flag| flag.set(false));
                }
            }
            IN_PROVIDER.with(|flag| flag.set(true));
            let _guard = InProviderGuard;
            match provider(repo_location, category, pf) {
                Ok(map) => {
                    let map = std::sync::Arc::new(map);
                    if let Ok(mut guard) = cache.write() {
                        guard.insert(key, std::sync::Arc::clone(&map));
                    }
                    Ok(map)
                }
                // The provider already reported its own failure; the caller's
                // contract (an unusable entry) is `failure`.
                Err(_) => Err(failure),
            }
        }
    }
}

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
