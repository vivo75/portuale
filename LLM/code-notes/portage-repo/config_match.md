# crates/portage-repo/src/config_match.rs
Per-package index over `package.*` config atoms.

## fn matches_config_entry
- S3 (#341): a single `portage_dep::match_from_list` call, which re-parses with `allow_wildcard: true, allow_repo: Some(true)` like Portage (dep:2901-2906). The old two-tier fallback (bounded `WildcardAtom` for `*/*`, `category/*`, `*/package`) is gone with the API it used.

## struct CpBucketIndex
- Portage's `ExtendedAtomDict` is built once at config load by every `UseManager` / `MaskManager` / `KeywordsManager` / `LicenseManager`. During a deep `emerge -pu` these lists are scanned per package per graph-walk visit -- tens of thousands of times -- but only the handful of entries whose atom names this `cp` (or a `*/*` / `cat/*` / `*/pkg` wildcard) can ever match.

## fn cp_bucket_index
- In practice every bucketed list is a clone of stable profile/user data, so a `(ptr, len)` collision already implies identical content -- the fingerprint just makes that guaranteed.

## trait ConfigAtomEntry
- `(atom, tokens)` for the value-carrying lists (`package.use`, `package.accept_keywords`, ...) and a bare `atom` for `package.mask` / `package.unmask`.

## fn config_entries_matching / any_config_entry_matches
- The bucketed replacements for `entries.iter().filter(|e| matches_config_entry(...))` / `entries.iter().any(|e| matches_config_entry(...))`.
