# crates/portage-repo/src/config_match.rs
Per-package index over `package.*` config atoms.

## fn matches_config_entry
- S3 (#341): a single `portage_dep::match_from_list` call, which re-parses with `allow_wildcard: true, allow_repo: Some(true)` like Portage (dep:2901-2906). The old two-tier fallback (bounded `WildcardAtom` for `*/*`, `category/*`, `*/package`) is gone with the API it used.
- S9 (#341): `matches_config_entry_with_build_id` (via `match_from_list_with_build_id`) pairs the string with the candidate's binary build-id; the `package.mask`/`unmask` mask paths (`visibility.rs`, `masked_deps.rs`, `provenance.rs`) pass `candidate.build_id` (S3 (#346) widens this to the accept-keywords and package.use paths; `package.license` keeps `None`).

- S3 (#346): the build-id passing widens beyond mask/unmask: the accept-keywords and package.use match paths (visibility, use_flags, binpkg_use, autounmask writers) now pass `candidate.build_id` through `config_entries_matching_with_build_id`/`specificity_ordered_flags`. `package.license` deliberately keeps `None` (Portage `LicenseManager.py:37-42` turns build-ids off there, so such a line is dropped).
## fn matches_cli_option_atom
- S7 (#341): Portage `WildcardPackageSet._implicitWildcarding` (`lib/portage/_sets/base.py`): a category-less CLI option atom (`--exclude`, `--reinstall-atoms`, `--rebuild-exclude`/`--rebuild-ignore`, `--usepkg-exclude`/`--usepkg-include`, `--useoldpkg-atoms`, `--buildpkg-exclude`, `--getbinpkg-exclude`/`--getbinpkg-include`) is retried as `*/entry`, so `--exclude t341-dep` excludes every category's `t341-dep`. The plain parse runs first and a categorised entry never falls through (Portage prefixes `*/` only on `InvalidAtom`). Config files (`package.*`) never do this — a category-less line there is invalid (S5/S6) — so they keep using `matches_config_entry` directly; every other `matches_config_entry` caller whose list is a CLI option list was switched to this.

## struct CpBucketIndex
- Portage's `ExtendedAtomDict` is built once at config load by every `UseManager` / `MaskManager` / `KeywordsManager` / `LicenseManager`. During a deep `emerge -pu` these lists are scanned per package per graph-walk visit -- tens of thousands of times -- but only the handful of entries whose atom names this `cp` (or a `*/*` / `cat/*` / `*/pkg` wildcard) can ever match.

## fn cp_bucket_index
- In practice every bucketed list is a clone of stable profile/user data, so a `(ptr, len)` collision already implies identical content -- the fingerprint just makes that guaranteed.

## trait ConfigAtomEntry
- `(atom, tokens)` for the value-carrying lists (`package.use`, `package.accept_keywords`, ...) and a bare `atom` for `package.mask` / `package.unmask`.

## fn config_entries_matching / any_config_entry_matches
- The bucketed replacements for `entries.iter().filter(|e| matches_config_entry(...))` / `entries.iter().any(|e| matches_config_entry(...))`.
