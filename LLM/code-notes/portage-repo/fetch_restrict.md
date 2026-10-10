# crates/portage-repo/src/fetch_restrict.rs
SRC_URI and RESTRICT=fetch helpers.

## fn use_flags_if_conditional
- Sits on the lazy-USE helper but documents the `license_accepted` algorithm it feeds: `config.accept_license` (global, already `@group`-expanded but still symbolic -- see that field's doc, portage-profile) is layered with every matching `package.license` entry's tokens, in atom-specificity order -- Portage's `_getPkgAcceptLicense` `accept_license.extend(x)` loop over `ordered_by_atom_specificity` matches, ported the same way `package.use.mask`/`.force` order multiple matches (see `effective_use_flags`). The symbolic list resolves into a concrete per-candidate set via the same `*`/`-*`/`-license`/`license` algorithm Portage's `getMissingLicenses`/`get_pruned_accept_license` use -- `*` needs "every license LICENSE could possibly mention" (Portage's `matchall=1`: every USE-conditional forced active, ported by reusing `portage_use_reduce::use_reduce_flat` with `MatchMode::All`, since group boundaries don't matter for this flat question the way they do for the masking check).
- A `LICENSE` string portuale's parser can't make sense of is treated as masked rather than accepted -- the "can't tell, so exclude" precedent `reinstall_flags_for_use_change`/`candidate_iuse_and_use` establish for an unreadable candidate, rather than Portage's differently-plumbed `InvalidDependString` handling, which routes malformed `LICENSE` to a separate "invalid metadata" masking reason portuale has no pathway for.
- The laziness is Portage's `use_reduce` "if '?' in license_str" optimization, shared by every metadata key needing "resolve USE, but only when it could matter".
- Owned (not the shared `Rc`) clone: cold path, one clone per call -- the same cost as before #103.

## fn evaluated_metadata_tokens
- Mirrors Portage's `_PackageMetadataWrapper.__getitem__` `use_reduce(...)` pass over a `_use_conditional_keys` value (`local_config and "?" in v`), which is what `pkg.properties`/`pkg.restrict` then `.split()`. Used for the display-only `interactive` bracket-column check (and available for the `fetch` one, if that lands later).
- `PROPERTIES`/`RESTRICT` have no `||`-group semantics (Portage's `config.py`: "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without groups") -- same reasoning `metadata_key_accepted` documents. An unparsable value yields an empty set (the token simply won't be found), the "can't tell, so don't claim it" precedent this crate sets elsewhere.

## fn fetch_restrict_files_all_present
- Mirrors Portage's `output.py:636` `not getfetchsizes(cpv, useflags=pkg_info.use, only_restricted=True)`, flattened against the candidate's effective USE (exactly the `useflags=pkg_info.use` Portage passes).
- `RESTRICT=fetch` is package-wide, so every `SRC_URI` file is "restricted" -- no per-file `fetch+` syntax -- making `only_restricted=True` a no-op filter here. Counts as *not* satisfied on unparsable input: the loud `F` column, same "can't tell, so don't claim it's fine" precedent as `evaluated_metadata_tokens`.

## fn flatten_src_uri_with_use
- Passes `useflags=pkg.use` semantics into `getfetchsizes`/`getFetchMap`.

## fn fetch_bytes_to_download
- Mirrors Portage's `output.py:300-332` per-package `_calc_size` contribution to `counters.totalsize` -- `db.getfetchsizes(cpv, useflags=pkg.use, myrepo=…)` (no `only_restricted`). Caller dedups a shared distfile across the graph exactly as Portage's `myfetchlist` does. Portage's `getfetchsizes` returns `None` for an incomplete digest map and `_calc_size` then adds nothing (`"[empty/missing/bad digest]"`).
