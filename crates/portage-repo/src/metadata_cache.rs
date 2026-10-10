//! md5-cache reading, the aux-metadata provider and pregen/regen switches.

use crate::*;

/// Reads `metadata/md5-cache/<category>/<pf>` (`pf` = "package-version",
/// e.g. "foo-1.2.3-r1") as a plain `KEY=value` map.
///
/// Memoised per file path and returned as a shared `Arc`: the resolver
/// reads the same md5-cache entry many times over one `emerge -pu`
/// (`list_candidates`, then every visibility / USE / slot-op /
/// changed-deps check for a candidate), and the first cut of this cache
/// -- which `clone()`d the map on every hit -- still showed ~10% of the
/// run in `HashMap<String,String>` copies. The tree's md5-cache is
/// immutable for the process lifetime (real portage's `portdbapi` keeps
/// the same entries in its own `_aux_cache`); `--regen`, portuale's only
/// md5-cache writer, is a separate process. No caller mutates the map.
/// Validation is a sibling memo (`md5_cache_entry_is_valid`), so this
/// reader stays a reader (#46 S3).
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
    // Real `profiles/updates/` package moves (SCOPE_BACKLOG Part 2 Section
    // H item 2): a sync bakes the `move`/`slotmove` directives into
    // `metadata/md5-cache` when it regenerates ebuild metadata. Portuale
    // reads a static fixture cache, so it applies them here on read
    // instead -- real `update_dbentry` over each `*DEPEND` string.
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

/// The hook `repo_aux_metadata` asks on a cache miss: it receives the
/// repo location plus `category`/`pf` and returns the aux dict a `depend`
/// phase produced (real `porttree.py`'s ebuild fallback). A plain `fn`
/// pointer, because the phase runner lives in the binary crate
/// (`portuale`), which cannot be referenced from here -- the layering
/// constraint C2 documents.
pub type AuxMetadataProvider = fn(&Path, &str, &str) -> Result<HashMap<String, String>, String>;

pub(crate) static AUX_METADATA_PROVIDER: OnceLock<AuxMetadataProvider> = OnceLock::new();

/// Register the process-wide fallback provider (idempotent: only the
/// first registration wins, so a second call is a no-op). Called once at
/// startup by the binary that can run ebuild phases; with no provider
/// registered, `repo_aux_metadata` behaves exactly as before C2.
pub fn register_aux_metadata_provider(provider: AuxMetadataProvider) {
    let _ = AUX_METADATA_PROVIDER.set(provider);
}

/// The single entry point for a repo's aux metadata for `category/pf`
/// ("package-version", e.g. `foo-1.2.3-r1`): every production
/// `read_md5_cache` call site in this crate and in `portuale`/`mrg-director`
/// goes through here.
///
/// This is the C2 hook site, and since #46 S3 the #46 validation site:
/// a per-cp cache **miss** (no `metadata/md5-cache/<category>/<pf>`
/// file) *or an entry real's `validate_entry` rejects* (stale `_md5_`,
/// stale `_eclasses_`, unsupported EAPI -- `md5_cache_entry_is_valid`)
/// asks the registered depend-phase provider for the metadata the cache
/// cannot supply -- real `porttree.py`'s `_pull_valid_cache` falling
/// through its rungs to `doebuild(mydo="depend")` (C0's P8 oracle).
/// With no provider registered (unit tests, `mrg`-only builds) a miss
/// keeps the same `Error::ReadFile` as before and an invalid entry is
/// returned as-is (E3), so cached repos -- every L0/L1 probe -- are
/// untouched. An entry whose ebuild is gone is trusted outright (E4).
/// Provider results are memoised per cp for the process lifetime (the
/// resolver reads one entry many times; C3 layers the cross-process
/// `depcachedir` write-back on top).
///
/// #55 adds the rung-selection gate in front: when
/// [`has_usable_md5_cache`] is false the md5-cache rung is skipped
/// outright -- real's first known `cache-formats` entry is `pms` (no
/// portuale reader, L1) or `FEATURES=metadata-transfer` removed the
/// pregen rung (`porttree.py:322`) -- and the provider path is entered
/// directly, exactly like `_pull_valid_cache` walking on to the
/// depcachedir and `depend` phase (S0 cells c/d/f/g). The entry bytes
/// are then never read: a `pms`-first repo's `metadata/md5-cache` is
/// ignored even when present and valid.
pub fn repo_aux_metadata(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> Result<std::sync::Arc<HashMap<String, String>>, Error> {
    if !has_usable_md5_cache(repo_location) {
        return provide_aux_metadata(
            repo_location,
            category,
            pf,
            Error::PregenCacheUnavailable {
                path: repo_location
                    .join("metadata/md5-cache")
                    .join(category)
                    .join(pf)
                    .display()
                    .to_string(),
            },
        );
    }
    match read_md5_cache(repo_location, category, pf) {
        Ok(map) => {
            if md5_cache_entry_is_valid(repo_location, category, pf) {
                return Ok(map);
            }
            if AUX_METADATA_PROVIDER.get().is_none() {
                return Ok(map);
            }
            let invalid = Error::StaleMd5Cache {
                path: repo_location
                    .join("metadata/md5-cache")
                    .join(category)
                    .join(pf)
                    .display()
                    .to_string(),
            };
            provide_aux_metadata(repo_location, category, pf, invalid)
        }
        Err(read_err) => provide_aux_metadata(repo_location, category, pf, read_err),
    }
}

/// The resolved [`RepoConfig`] for one repo location, memoised per path
/// (config files are immutable for the process lifetime -- `find_repos`
/// is a pure read). `None` when the process's own config
/// (`PORTAGE_CONFIGROOT` + `repos.conf`) does not describe that location
/// at all -- direct fixture/unit-test callers of the location-keyed
/// functions, which then keep their pre-#55 local fallbacks. The same
/// lookup `md5_dict::repo_masters_for_location` used to do inline.
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

/// The C2 fallback shared by a missing entry (`failure` is the
/// `read_md5_cache` error) and an entry real's validator rejects
/// (`failure` is `Error::StaleMd5Cache`): fallback memo, recursion
/// guard, provider. The provider failure always surfaces as `failure`
/// (E5: never fall back to stale data), which reads as an unusable
/// entry for the caller.
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
            // The provider itself runs the depend phase, whose env
            // assembly reads `RESTRICT`/`PROPERTIES` through this same
            // entry point -- a *nested* miss must not recurse into the
            // provider again (it would never terminate). Real has the
            // same shape: the depend phase reads the ebuild, not the
            // cache it is generating.
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
                // The provider already reported its own failure; the
                // caller's contract (an unusable entry) is `failure`.
                Err(_) => Err(failure),
            }
        }
    }
}

/// Whether the on-disk `metadata/md5-cache/<category>/<pf>` entry passes
/// real `validate_entry` for the `md5_database` (pairs) format,
/// memoised per path -- real `_pull_valid_cache` validates each rung
/// once and the resolver reads the same cp many times. Raw text, not the
/// `read_md5_cache` map: `apply_updates_to_dep_string` rewrites the
/// `*DEPEND` values, and while the validator does not look at them the
/// raw bytes are what real reads.
///
/// `true` when the ebuild is missing (E4: trust the entry -- real would
/// report the package as non-existent, but no portuale reader reaches an
/// ebuild-less entry) and when `pf` is not a `package-version` name at
/// all.
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
            // `read_md5_cache` already failed; keep the trust direction.
            return true;
        };
        // Only `_eclasses_` validation needs the masters chain; resolving
        // it loads the repo config, so keep the common eclass-less entry
        // (most of them) off that path.
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

/// Whether real portage would read this repo's pregenerated aux metadata
/// from `metadata/md5-cache` rather than falling through to the
/// writable depcachedir / `depend` phase: the **first known** entry of
/// [`RepoConfig::cache_formats`] is `md5-dict`.
///
/// Real's chain: `parse_layout` resolves `cache-formats` (lowercased +
/// split, auto-detect `md5-dict` then `pms`) into the repo's list
/// (`config.py:1564-1577`); `iter_pregenerated_caches`
/// (`config.py:576-605`) yields one database per known format in list
/// order, skipping unknown names; `get_pregenerated_cache`
/// (`config.py:607-616`) keeps only the first. So `cache-formats = pms
/// md5-dict` resolves to `metadata/cache` even when only
/// `metadata/md5-cache` exists -- every lookup misses and falls through
/// -- and `foo` yields no pregen cache at all. Portuale has no `pms`
/// reader (#55 decision L1, a documented narrowing -- slower, not
/// different: the depcachedir rung and `depend` phase produce the same
/// metadata real reads from a valid `pms` cache), so "the first known
/// format is `md5-dict`" is exactly "portuale may read
/// `metadata/md5-cache`". Pure, so the read-path gate and `--regen`'s
/// writer both resolve the same way; memoised per path only by callers.
pub fn pregen_md5_cache_enabled(repo: &RepoConfig) -> bool {
    repo.cache_formats
        .iter()
        .find(|f| f.as_str() == "md5-dict" || f.as_str() == "pms")
        .is_some_and(|f| f.as_str() == "md5-dict")
}

/// Whether real `egencache --update` would write this repo's
/// `metadata/md5-cache`: `md5-dict` is among the **known** resolved
/// `cache-formats`, in any position, or the list is empty -- egencache
/// always passes `force=True`, which defaults an empty list to
/// `("md5-dict",)`.
///
/// Real `egencache` builds its target list with
/// `conf.iter_pregenerated_caches(force=True, readonly=False)`
/// (`bin/egencache:350-362`), which yields every known format in list
/// order (and `("md5-dict",)` when the list is empty), then writes each
/// -- S0 cell h1/h3: `pms` alone writes `metadata/cache` only, `pms
/// md5-dict` writes both dirs. Portuale's `--regen` is egencache-shaped
/// but has no `pms` writer (#55 L3), so this predicate decides where it
/// may write: the md5-dict half when real would write that dir
/// (including the fresh-tree default), skipping the repo otherwise
/// (never the wrong dir). Note the reader's mirror
/// ([`pregen_md5_cache_enabled`]) is *not* the same for an empty list:
/// `get_pregenerated_cache` uses `force=False`, so a repo with no
/// `cache-formats` and no cache dir reads from the ebuild, while
/// egencache still creates `metadata/md5-cache` for it.
pub fn regen_writes_md5_cache(repo: &RepoConfig) -> bool {
    repo.cache_formats.is_empty() || repo.cache_formats.iter().any(|f| f == "md5-dict")
}

/// Whether the process's resolved `FEATURES` names `metadata-transfer`
/// (real `porttree.py:322`: `if "metadata-transfer" not in
/// self.settings.features: self._create_pregen_cache()` -- the token
/// removes the pregen repo cache rung entirely), resolved once per
/// process. `false` whenever the config cannot be resolved (fail open:
/// the pre-#55 behaviour).
pub(crate) fn metadata_transfer_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| metadata_transfer_in_config(&config_root_from_env(), &root_from_env()))
}

/// The resolved-config half of [`metadata_transfer_enabled`], split out
/// so a unit test can point it at a fixture config root without touching
/// the process environment. Builds the same `repos.conf` + profile /
/// `make.conf` / `env`-layer stack the CLI layers pass to
/// `portage_profile::resolve_config` (the `env` layer includes a
/// `FEATURES=… emerge` override, which real treats as incremental --
/// `const.INCREMENTALS`).
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
/// per repo path: the resolved rung-selection predicate #55 wires into
/// every md5-cache reader.
///
/// Grounding: real portage's auxdb/cache-format selection
/// (`repository/config.py::iter_pregenerated_caches`, driven by
/// `porttree.py:392`'s `_create_pregen_cache`) treats the md5-cache as one
/// *cache format* among several (`pms`'s `metadata/cache`, `md5-dict`'s
/// `metadata/md5-cache`); when the first known format is not `md5-dict`,
/// or `FEATURES=metadata-transfer` removed the rung (`porttree.py:322`),
/// `aux_get` falls back to the writable depcachedir and then to
/// generating metadata from the ebuild via the depend phase. The same
/// split here: `true` means aux metadata can be read from the md5-cache
/// dir; `false` means it must come from the provider path (C2/C3). A
/// location the process config does not describe (direct fixture /
/// unit-test callers) keeps the pre-#55 "the directory exists" fallback.
/// A written directory may still hold entries #46 S3 rejects; this flag
/// only says the rung is in use.
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
        // `pregen_md5_cache_enabled` first so a `pms`-first repo skips
        // the (memoised, but full-config) FEATURES resolution.
        Some(repo) => pregen_md5_cache_enabled(&repo) && !metadata_transfer_enabled(),
        None => repo_location.join("metadata").join("md5-cache").is_dir(),
    };
    if let Ok(mut guard) = flags.write() {
        guard.insert(repo_location.to_path_buf(), usable);
    }
    usable
}
