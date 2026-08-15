//! md5-cache reading, the aux-metadata provider and pregen/regen switches.

use crate::*;

/// Reads `metadata/md5-cache/<category>/<pf>` (`pf` = "package-version",
/// e.g. "foo-1.2.3-r1") as a plain `KEY=value` map.
///
/// Memoised per file path and returned as a shared `Arc`: the resolver
/// reads the same entry many times per run. The tree's md5-cache is
/// immutable for the process lifetime; `--regen` is a separate process.
/// No caller mutates the map.
pub(crate) fn read_md5_cache(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> Result<std::sync::Arc<HashMap<String, String>>, Error> {
    type Md5CacheMap = HashMap<PathBuf, std::sync::Arc<HashMap<String, String>>>;
    static CACHE: OnceLock<RwLock<Md5CacheMap>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));

    let path = repo_location
        .join("metadata")
        .join("md5-cache")
        .join(category)
        .join(pf);

    if let Ok(guard) = cache.read()
        && let Some(map) = guard.get(&path)
    {
        return Ok(std::sync::Arc::clone(map));
    }

    let text = fs::read_to_string(&path).map_err(|e| Error::ReadFile {
        path: path.display().to_string(),
        source: e,
    })?;
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some(eq) = line.find('=') {
            map.insert(line[..eq].to_string(), line[eq + 1..].to_string());
        }
    }
    // `profiles/updates/` moves apply here on read: a sync bakes them
    // into the cache, but portuale reads a static cache.
    // Portage: update_dbentry over each `*DEPEND` string.
    for key in ["DEPEND", "RDEPEND", "PDEPEND", "BDEPEND", "IDEPEND"] {
        if let Some(dep) = map.get(key)
            && let Some(updated) = apply_updates_to_dep_string(dep)
        {
            map.insert(key.to_string(), updated);
        }
    }

    let map = std::sync::Arc::new(map);
    if let Ok(mut guard) = cache.write() {
        guard.insert(path, std::sync::Arc::clone(&map));
    }
    Ok(map)
}

/// The hook `repo_aux_metadata` asks on a cache miss: the aux dict a
/// `depend` phase produced. A plain `fn` pointer, because the phase
/// runner lives in the binary crate, which cannot be referenced from here.
/// Portage: porttree.py's ebuild fallback.
pub type AuxMetadataProvider = fn(&Path, &str, &str) -> Result<HashMap<String, String>, String>;

pub(crate) static AUX_METADATA_PROVIDER: OnceLock<AuxMetadataProvider> = OnceLock::new();

/// The resolved [`RepoConfig`] for one repo location, memoised per path.
/// `None` when the process's own config does not describe that location
/// at all -- direct fixture/unit-test callers keep local fallbacks then.
pub(crate) fn repo_config_for_location(repo_location: &Path) -> Option<RepoConfig> {
    type Memo = HashMap<PathBuf, Option<RepoConfig>>;
    static MEMO: OnceLock<RwLock<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| RwLock::new(HashMap::new()));

    if let Ok(guard) = memo.read()
        && let Some(hit) = guard.get(repo_location)
    {
        return hit.clone();
    }
    let found = find_repos_impl(&config_root_from_env(), false)
        .ok()
        .and_then(|repos| repos.into_iter().find(|r| r.location == repo_location));
    if let Ok(mut guard) = memo.write() {
        guard.insert(repo_location.to_path_buf(), found.clone());
    }
    found
}

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

/// Whether the on-disk entry passes Portage's `validate_entry` for the
/// `md5_database` (pairs) format, memoised per path. Validates the raw
/// text, not the `read_md5_cache` map (which rewrites `*DEPEND` values).
///
/// `true` when the ebuild is missing (trust the entry) and when `pf` is
/// not a `package-version` name at all.
pub(crate) fn md5_cache_entry_is_valid(repo_location: &Path, category: &str, pf: &str) -> bool {
    let cache_path = repo_location
        .join("metadata")
        .join("md5-cache")
        .join(category)
        .join(pf);
    type Validity = HashMap<PathBuf, bool>;
    static VALID: OnceLock<RwLock<Validity>> = OnceLock::new();
    let validity = VALID.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(guard) = validity.read()
        && let Some(valid) = guard.get(&cache_path)
    {
        return *valid;
    }
    let is_valid = (|| {
        let Some((package, _version)) = split_pf(pf) else {
            return true;
        };
        let ebuild = repo_location
            .join(category)
            .join(&package)
            .join(format!("{pf}.ebuild"));
        if !ebuild.is_file() {
            return true;
        }
        let Ok(text) = fs::read_to_string(&cache_path) else {
            // `read_md5_cache` already failed; keep trusting the entry.
            return true;
        };
        // Only `_eclasses_` validation needs the masters chain, so keep the
        // common eclass-less entry off the repo-config path.
        let needs_eclasses = text.lines().any(|line| {
            line.strip_prefix("_eclasses_=")
                .is_some_and(|value| !value.trim().is_empty())
        });
        let masters = if needs_eclasses {
            md5_dict::repo_masters_for_location(repo_location)
        } else {
            Vec::new()
        };
        md5_dict::entry_is_valid(&text, &ebuild, repo_location, &masters, false)
    })();
    if let Ok(mut guard) = validity.write() {
        guard.insert(cache_path, is_valid);
    }
    is_valid
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

/// Whether the process's resolved `FEATURES` names `metadata-transfer`
/// (which removes the pregen repo cache rung entirely), resolved once per
/// process. `false` whenever the config cannot be resolved (fail open).
/// Portage: porttree.py's `_create_pregen_cache` gate.
pub(crate) fn metadata_transfer_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| metadata_transfer_in_config(&config_root_from_env(), &root_from_env()))
}

/// The resolved-config half of [`metadata_transfer_enabled`], split out
/// so a unit test can point it at a fixture config root without touching
/// the process environment.
pub(crate) fn metadata_transfer_in_config(config_root: &Path, root: &Path) -> bool {
    let Ok(repos) = find_repos_impl(config_root, false) else {
        return false;
    };
    let Some(main_repo) = repos.iter().find(|r| r.is_main) else {
        return false;
    };
    let overlay_repos: Vec<(String, PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_masters: HashMap<String, Vec<PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    let repo_aliases: Vec<(String, PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot: root,
    })
    .map(|config| {
        config
            .resolved_incremental("FEATURES")
            .or_else(|| {
                config
                    .other_vars
                    .get("FEATURES")
                    .cloned()
                    .map(|f| f.split_whitespace().map(String::from).collect::<Vec<_>>())
            })
            .unwrap_or_default()
            .iter()
            .any(|f| f == "metadata-transfer")
    })
    .unwrap_or(false)
}

/// Whether `repo_location` ships a usable `metadata/md5-cache`, memoised
/// per repo path: the rung-selection predicate every md5-cache reader uses.
///
/// `true` means aux metadata can be read from the md5-cache dir; `false`
/// means it must come from the provider path. A location the process
/// config does not describe keeps the "the directory exists" fallback for
/// direct fixture/unit-test callers. This flag only says the rung is in
/// use -- a written directory may still hold entries the validator rejects.
pub fn has_usable_md5_cache(repo_location: &Path) -> bool {
    type UsableFlags = HashMap<PathBuf, bool>;
    static FLAGS: OnceLock<RwLock<UsableFlags>> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| RwLock::new(HashMap::new()));

    if let Ok(guard) = flags.read()
        && let Some(usable) = guard.get(repo_location)
    {
        return *usable;
    }

    let usable = match repo_config_for_location(repo_location) {
        // The md5-cache gate first so a `pms`-first repo skips the
        // (memoised, but full-config) FEATURES resolution.
        Some(repo) => pregen_md5_cache_enabled(&repo) && !metadata_transfer_enabled(),
        None => repo_location.join("metadata").join("md5-cache").is_dir(),
    };
    if let Ok(mut guard) = flags.write() {
        guard.insert(repo_location.to_path_buf(), usable);
    }
    usable
}
