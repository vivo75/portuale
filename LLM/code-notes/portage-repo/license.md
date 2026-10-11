# crates/portage-repo/src/license.rs
LICENSE parsing and ACCEPT_LICENSE acceptance.

## enum LicenseNode
- Verified directly against Portage's `portage.dep.use_reduce(..., opconvert=True)`: a `||` group's own members are flat (`['||', 'MIT', 'BSD']`, not double-nested), but a *plain* sub-group (or a conditional's resolved contents) sitting directly inside that same `||`'s member list stays a genuine nested unit (`['||', ['GPL-2', 'MIT'], 'BSD']` for `|| ( ( GPL-2 MIT ) BSD )` -- "GPL-2 AND MIT" is one whole alternative, not two independent ones) -- while the identical sub-group anywhere *else* (top level, or inside another plain/conditional group) flattens into its parent instead (opconvert's "AND of AND is just AND" collapse).
- This is why `use_reduce_flat` (which discards *all* group boundaries, a deliberate, already-documented simplification Portage DEPEND/RDEPEND recursion relies on) can't be reused for LICENSE at all -- the same reasoning that made `portage_required_use` its own separate algorithm rather than a mode of this function.
- PMS 7.3.2 grammar structure; the conditional/bundle distinction only matters once a `flag?` USE-conditional has been resolved.

## enum PendingBracket
- Mirrors `use_reduce_flat`'s `need_bracket`/conditional-pop dance.

## fn parse_license_tree
- Bracket/`need_bracket` handling is a direct structural port of `use_reduce_flat` (`portage-use-reduce`).

## fn tree_has_masked_license
- Whether `nodes` (implicit AND -- the top level, or an `AllOf` bundle) has at least one license that isn't in `acceptable`. Mirrors Portage `LicenseManager._getMaskedLicenses`'s own non-`||` branch, as a bool rather than the full "list every masked license" diagnostic (this portuale has no mask-reason display to feed it -- same simplification `check_required_use` already makes for REQUIRED_USE).
- Same simplification `check_required_use` makes for REQUIRED_USE.

## fn node_has_masked_license
- The `||` arm mirrors `_getMaskedLicenses`'s "if not tmp: return []" the moment one alternative comes back clean.

## fn tree_masked_license_names
- "We don't know which combination the user will unmask": every masked name across every alternative. Portage appends as it walks (order-preserving); the caller sorts + dedups.

## fn has_masked_license
- `use_flags` (this candidate's own resolved USE, only ever consulted if `license_str` actually contains a `?`) and `acceptable` (the fully-resolved, concrete set of accepted license names for this specific candidate -- see `license_accepted`).
- `use_flags` only consulted if `license_str` contains a `?`. An empty `license_str` is never masked, matching Portage's `use_reduce("")` empty result.

## fn resolve_accept_tokens
- This candidate's own effective `ACCEPT_LICENSE`/`ACCEPT_PROPERTIES`/`ACCEPT_RESTRICT`-style symbolic token list: `global_accept`, with every matching `package_accept` entry's own tokens layered on top, in atom-specificity order.
- Mirrors Portage's `_getPkgAcceptLicense` `accept_license.extend(x)` loop over `ordered_by_atom_specificity` matches (and its `_getMissingProperties`/`_getMissingRestrict` siblings for their own accept lists). Unlike `package.use.mask`/`.force` (see `specificity_ordered_flags`), which must stay in profile-chain order so a child `-flag` can cancel a parent mask.

## fn all_mentioned_tokens
- Reuses `portage_use_reduce::use_reduce_flat` with `MatchMode::All` directly, unlike the masking check itself for `LICENSE` (see `has_masked_license`).

## fn resolve_acceptable_tokens
- Portage's `getMissingLicenses`/`_getMissingProperties`/`_getMissingRestrict` all use this identical algorithm, just for a different metadata key.

## fn missing_licenses
- Sorted + deduped, the form `_display_autounmask`'s `license_msg` prints (`' '.join(sorted(missing_licenses))`). Shares every step with `license_accepted`.
