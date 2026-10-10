# crates/portage-profile/src/package_files.rs
Line readers for `package.mask`/`.unmask`, `package.accept_keywords`, `package.license`/`.properties`/`.accept_restrict`, `package.use`, and `license_groups`.

## fn read_config_lines
- Real `_recursive_file_list` semantics: arbitrary depth, one ascending sort per directory over files and subdirectories together with each subdirectory expanded in place, skipping `VCS_DIRS` directories, dot-prefixed names and `~`-suffixed backups at every level including the root path itself, following symlinks (via `portage_util::recursive_config_files`).
- Unreadable paths report; missing paths stay empty.

## fn scope_repo_mask_lines
- Portage `append_repo`'s "atoms without an explicit repo part get one; atoms that already have one are left alone" rule, applied to an overlay's own repo-level entries so they can never silently mask/unmask a same-named package in a different repo.
- A removal atom is scoped exactly like an addition one, so it can only ever cancel a same-repo-scoped entry.

## fn scope_repo_package_use_lines
- Unlike `package.mask`/`.unmask`, none of these files has `-atom` whole-entry removal syntax: Portage only ever `.extend()`s these, and a leading `-` inside the flag list masks/forces a single flag off, not an atom. So there is no leading-`-` case to preserve here.

## fn stack_mask_lines
- Ports Portage `stack_lists`'s `ignore_repo=True` behavior, the flag Portage `MaskManager.__init__` always passes for its final `[repo_pkgmasklines, profile_pkgmasklines, user_pkgmasklines]` combination, confirmed by reading it directly: "let `-cat/pkg` remove `cat/pkg::repo`". An unscoped removal token (all a profile-level or user-level `-atom` can ever be, since only repo-level entries get `::repo`-scoped) strips `::repo` off every existing entry before comparing; a `::repo`-scoped removal token (rare, only hand-written) keeps exact-match semantics, matching Portage's own `"::" not in token` guard exactly.
- Previously portuale's user-level-only `package.unmask` handling treated a leading `-` as meaningless; meaningful once more than one source contributes.
- Without the unscoped-removal rule, a profile's `-dev-libs/foo` could never cancel the main repo's now-`::reponame`-scoped mask entry: a regression the main-repo-scoping follow-up would otherwise have introduced.

## fn parse_package_accept_keywords_lines
- A bare atom gets Portage `accept_keywords_defaults`' implicit `~arch` meaning at both levels, confirmed by reading `KeywordsManager.__init__` (user-level: a bare `pkgdict` entry's value is replaced with `accept_keywords_defaults` before storage in `self.pkeywordsdict`) and `getPKeywords` (profile-level: the identical substitution at read time per matching entry). Same formula either way: `"~" + keyword` for each plain (non-`~`/`-`-prefixed) token in the current global `ACCEPT_KEYWORDS`. Contrary to portuale's earlier assumption, the user-level source gets the identical substitution too.
- This function only preserves the bare atom so that substitution has something to act on; `parse_package_license_lines` filters bare atoms back out immediately after, since no other file gets this treatment.

## fn parse_package_license_lines
- Same shape as `package.accept_keywords`, reused directly for all three files (unlike `parse_package_use_lines`'s deliberate separateness, which exists only because of `package.use`'s `USE_EXPAND`-shorthand parameter).

## fn parse_license_groups_lines
- `grabdict` format: no `-atom`/removal semantics. Matches Portage `self._license_groups.setdefault(k, []).extend(v)` exactly.

## fn expand_license_token
- `-@EULA` with `EULA = "Eula1 Eula2"` expands to `-Eula1 -Eula2`, matching Portage's tail `if negate: rValue = ["-" + token for token in rValue]`.
- Portage: "Skipping invalid element %s in license group '%s'": a group's member list is never itself allowed to contain a `-`-negated entry.
- Both circular references and undefined group names are deliberately silent here (no stderr warning), unlike Portage's own `writemsg`: portuale has no precedent for emitting warnings to stderr from deep inside config resolution (every other Portage warning-only path is silently skipped the same way).

## fn parse_package_use_lines
- Matches `parse_package_accept_keywords_lines` on bare atoms (documented no-op).
- Portage consumption (`config.py`'s `regenerate`) only ever `.extend()`s a growing token list per source, never removes a previous entry.
- The shorthand resets at the start of every line, confirmed by reading Portage `grabdict_package`'s `newlines=1` marker handling (a fresh `"\n"` token between every physical line; the loop resets its prefix on each one). Only the user-level source (`_parse_user_files_to_extatomdict`) ever applies it; `_parse_repository_files_to_dict_of_dicts`/`_parse_profile_files_to_tuple_of_dicts` go through `_parse_file_to_dict`, which never passes `newlines=1` and has no expansion step, so a `VIDEO_CARDS:` token there is just a literal, almost-certainly-invalid USE token. Genuine Portage behavior, not a portuale-invented restriction; see `resolve_config`'s call sites.
