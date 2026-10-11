# crates/portage-repo/src/tests/metadata_cache.rs
md5-cache presence, `cache-formats` handling, and `--regen` write-gate tests.

## fn has_usable_md5_cache_is_the_presence_of_the_cache_dir
- `has_usable_md5_cache` is the per-repo "cache format available?" bit Portage's `porttree.py` derives from its auxdb/cache-format selection: `fixtures/repo` ships `metadata/md5-cache`, a fresh ebuild-only tree does not (so C2 must generate its metadata from the ebuild).

## fn stale_md5_cache_entry_is_returned_without_a_provider
- Backlog #46 S3 E3: with no provider registered (unit tests, `mrg`-only builds) a present-but-stale entry is returned as-is -- the pre-C2 contract; the read-path validation must not turn it into an error.

## fn md5_cache_entry_without_an_ebuild_is_returned
- Backlog #46 S3 E4: an entry whose ebuild is gone is trusted outright -- no validation can run, and every resolver reader reaches this through an ebuild walk, so the entry is unreachable from `list_candidates` anyway.

## fn pregen_md5_cache_follows_the_first_known_cache_format
- Backlog #55 S0 cells a-f (`TEST/findings/l2.md` "## #55 S0"): Portage lowercases and splits `cache-formats`, auto-detects `md5-dict` then `pms` from directory presence when it is empty, and `get_pregenerated_cache` keeps only the **first known** format -- an unknown name is skipped, no known name means no pregen cache at all.
- Cells b/e: explicit `md5-dict`, and `md5-dict pms` -> used (`MD5-DICT` proves the value is lowercased first).
- Cells c/d: `pms` first, with or without a following `md5-dict` -> Portage reads `metadata/cache`; portuale (no pms reader, L1) falls through to depcache/depend.

## fn metadata_transfer_is_read_from_the_resolved_features
- Backlog #55 S0 cell g: `FEATURES=metadata-transfer` in the resolved config stack disables the pregen cache rung (Portage `porttree.py:322`), independent of `cache-formats`. The process env is the same incremental layer (`const.INCREMENTALS`), so a `FEATURES=… emerge` override resolves through here too.

## fn regen_writes_md5_cache_when_md5_dict_is_among_the_resolved_formats
- Backlog #55 L3 (S0 cell h): Portage's `egencache --update` writes `metadata/cache` for `pms`, both dirs for `pms md5-dict`, and errors when no known format remains; portuale's egencache-shaped `--regen` may write the md5-cache half exactly when `md5-dict` is among the resolved known formats, in any position.
- No key, no directories -> empty list -> egencache's `force=True` default `("md5-dict",)` still writes the cache (unlike the reader, `get_pregenerated_cache(force=False)`).
