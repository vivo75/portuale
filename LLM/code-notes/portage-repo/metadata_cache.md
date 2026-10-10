# crates/portage-repo/src/metadata_cache.rs
md5-cache reading, the aux-metadata provider and pregen/regen switches.

## fn read_md5_cache
- The resolver reads the same md5-cache entry many times over one `emerge -pu` (`list_candidates`, then every visibility / USE / slot-op / changed-deps check for a candidate), and the first cut of this cache -- which `clone()`d the map on every hit -- still showed ~10% of the run in `HashMap<String,String>` copies. The tree's md5-cache is immutable for the process lifetime (Portage's `portdbapi` keeps the same entries in its own `_aux_cache`). Validation is a sibling memo (`md5_cache_entry_is_valid`), so this reader stays a reader (backlog #46 S3).
- `profiles/updates/` package moves (SCOPE_BACKLOG Part 2 Section H item 2): a sync bakes the `move`/`slotmove` directives into `metadata/md5-cache` when it regenerates ebuild metadata. Portuale reads a static fixture cache, so it applies them here on read instead -- Portage `update_dbentry` over each `*DEPEND` string.

## type AuxMetadataProvider
- The layering constraint C2: the phase runner lives in the binary crate (`portuale`), which cannot be referenced from here.

## fn register_aux_metadata_provider
- With no provider registered, `repo_aux_metadata` behaves exactly as before C2.

## fn repo_aux_metadata
- This is the C2 hook site, and since backlog #46 S3 the validation site: a per-cp cache miss (no `metadata/md5-cache/<category>/<pf>` file) or an entry Portage's `validate_entry` rejects (stale `_md5_`, stale `_eclasses_`, unsupported EAPI) asks the registered depend-phase provider (C0's P8 oracle). With no provider registered (unit tests, `mrg`-only builds): E3 (an invalid entry is returned as-is), so cached repos -- every L0/L1 probe -- are untouched. E4 (an entry whose ebuild is gone is trusted outright). The resolver reads one entry many times; C3 layers the cross-process `depcachedir` write-back on top.
- Backlog #55 rung-selection gate: when `has_usable_md5_cache` is false the md5-cache rung is skipped outright -- Portage's first known `cache-formats` entry is `pms` (no portuale reader, L1) or `FEATURES=metadata-transfer` removed the pregen rung (`porttree.py:322`) -- exactly like `_pull_valid_cache` walking on to the depcachedir and `depend` phase (S0 cells c/d/f/g).
- The single entry point for a repo's aux metadata for `category/pf` ("package-version", e.g. `foo-1.2.3-r1`): every production `read_md5_cache` call site in this crate and in `portuale`/`mrg-director` goes through here.

## fn repo_config_for_location
- Config files are immutable for the process lifetime (`find_repos` is a pure read). The process's own config is `PORTAGE_CONFIGROOT` + `repos.conf`. The same lookup `md5_dict::repo_masters_for_location` used to do inline. Callers without a described location keep their pre-#55 local fallbacks.

## fn provide_aux_metadata
- E5: never fall back to stale data. Portage has the same recursion-guard shape: the depend phase reads the ebuild, not the cache it is generating. The provider itself runs the depend phase, whose env assembly reads `RESTRICT`/`PROPERTIES` through this same entry point -- a nested miss must not recurse into the provider again (it would never terminate).

## fn md5_cache_entry_is_valid
- Portage `_pull_valid_cache` validates each rung once and the resolver reads the same cp many times. The validator does not look at `*DEPEND` values, but the raw bytes are what Portage reads.
- E4: trust the entry -- Portage would report the package as non-existent, but no portuale reader reaches an ebuild-less entry.

## fn pregen_md5_cache_enabled
- Portage's chain: `parse_layout` resolves `cache-formats` (lowercased + split, auto-detect `md5-dict` then `pms`) into the repo's list (`config.py:1564-1577`); `iter_pregenerated_caches` (`config.py:576-605`) yields one database per known format in list order, skipping unknown names; `get_pregenerated_cache` (`config.py:607-616`) keeps only the first. So `cache-formats = pms md5-dict` resolves to `metadata/cache` even when only `metadata/md5-cache` exists -- every lookup misses and falls through -- and `foo` yields no pregen cache at all. Backlog #55 decision L1. Pure, so the read-path gate and `--regen`'s writer both resolve the same way; memoised per path only by callers.

## fn regen_writes_md5_cache
- Backlog #55 L3: Portage's `--regen` is egencache-shaped but portuale has no `pms` writer, so this predicate decides where it may write: the md5-dict half when Portage would write that dir (including the fresh-tree default), skipping the repo otherwise (never the wrong dir). S0 cells h1/h3: `pms` alone writes `metadata/cache` only, `pms md5-dict` writes both dirs (`bin/egencache:350-362`).
- Portage `egencache` builds its target list with `conf.iter_pregenerated_caches(force=True, readonly=False)` (`bin/egencache:350-362`), which yields every known format in list order (and `("md5-dict",)` when the list is empty), then writes each -- S0 cells h1/h3: `pms` alone writes `metadata/cache` only, `pms md5-dict` writes both dirs. Portuale's `--regen` is egencache-shaped but has no `pms` writer (#55 L3), so this predicate decides where it may write: the md5-dict half when Portage would write that dir.

## fn metadata_transfer_enabled
- Fail open to the pre-#55 behaviour. Portage `porttree.py:322`: `if "metadata-transfer" not in self.settings.features: self._create_pregen_cache()`.

## fn metadata_transfer_in_config
- Builds the same `repos.conf` + profile / `make.conf` / `env`-layer stack the CLI layers pass to `portage_profile::resolve_config` (the `env` layer includes a `FEATURES=… emerge` override, which Portage treats as incremental -- `const.INCREMENTALS`).

## fn has_usable_md5_cache
- Backlog #55. Grounding: Portage's auxdb/cache-format selection (`repository/config.py::iter_pregenerated_caches`, driven by `porttree.py:392`'s `_create_pregen_cache`) treats the md5-cache as one cache format among several (`pms`'s `metadata/cache`, `md5-dict`'s `metadata/md5-cache`); when the first known format is not `md5-dict`, or `FEATURES=metadata-transfer` removed the rung (`porttree.py:322`), `aux_get` falls back to the writable depcachedir and then to generating metadata from the ebuild via the depend phase. Backlog #46 S3 rejects some written entries; this flag only says the rung is in use.
