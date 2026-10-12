# crates/portage-profile/src/atom_lines.rs
Profile-file atom validation (`grabfile_package` / `grabdict_package` port, #341 S5).

## struct NodeAtomRules
- Per-profile-node parse flags: the node's `eapi` string plus `allow_repo` / `allow_build_id` resolved from the containing repo's `profile-formats`, never wildcards (repo profiles pass no `allow_wildcard`; Portage `MaskManager.py:171-193`, `KeywordsManager.py:25-60`, `UseManager.py:277-288` with `user_config=False`). `eapi: Option<String>` stays optional for the S6 user-profile path (`eapi=None`, permissive + wildcards per file).
- `allow_profile_repo_deps`: Portage `repository/config.py:1476-1485` is `eapi_has_repo_deps(eapi) or "profile-repo-deps" in formats`; `eapi_has_repo_deps` (eapi.py:111-112) is the `repo_deps` attr, False for every supported EAPI 0-9, True for `None`/unsupported. Computed through S1's `eapi_attrs` so the table stays single-sourced.

## fn node_atom_rules
- The node's `eapi` file is `readline().strip()` (first line, whitespace-stripped), else the repo default; an unsupported value raises Portage's `ParseError("Profile contains unsupported EAPI …")` (LocationsManager.py:216-242), which empties the whole profile list — ported as `Error::UnsupportedProfileEapi`, checked for every chain node up front in `load_scalar_layers` (chain order = Portage visit order, first bad node wins) so the abort precedes any file read, exactly like Portage's `_addProfile` raising during `load_profiles`.
- An empty `eapi` file reads as `""`, which `eapi_is_supported` rejects → ParseError (Portage behaves the same: `readline().strip()` is `""`, unsupported). Verified by reading, not probed live (no fixture has one).
- A supported-but-unparseable EAPI (`3_pre1`, padded) makes Portage's `Eapi()` raise inside `Atom()`; here `AtomEapi::parse` errs and `filter_atom_lines` falls back to `Unsupported` (permissive). Deliberate cut per S1's rule (Portage crashes there); unreachable in fixtures (node files are stripped digits).

## fn repo_rules_for
- `repo.eapi` transcription of repository/config.py:524-547: top-level `profiles/eapi` wins only with `profile_eapi_default` (EAPI >= 9, eapi.py:159-160); else the `profile_eapi_when_unspecified` layout key, else `"0"`. The key counts only with `profile-default-eapi` in `profile-formats` and only for a supported EAPI (config.py:1688-1727); otherwise dropped with the `SyntaxWarning` (message text reproduced, without the Python `config.py:1694:` frame and `warnings.warn(` line — container install paths, not pinnable hermetically; S5 summary records this).
- The underscore spelling `profile_eapi_when_unspecified` is normative (S0 matrix §3; config.py:529,547,1689,1727; muse I5 resolved). Do NOT "fix" it to hyphens.
- `file_eapi` (what repo-level files parse with) is the file-adjacent value when well-formed, else `repo.eapi` — one computation per repo, mirroring Portage's `_eapi_cache` (warning fires once).
- NOT ported: the key-vs-top-level-default conflict `SyntaxWarning` (config.py:524-547, "setting that will be ignored…") — needs `profiles/eapi` >= 9 plus a key; no fixture reaches it. Residue for a future slice, not a cut (Portage behaviour recorded here so it isn't forgotten).

## fn read_adjacent_eapi
- Portage `read_corresponding_eapi_file` (util/__init__.py:441-475): the `eapi` file next to the read file must hold exactly one line (only `\n` stripped — a trailing space survives, like Portage), else `--- Invalid 'eapi' file (doesn't contain exactly one line): <path>` and the default; missing is silent. NO support check (unlike the node reader) — an `"10"` file flows into `Atom(eapi="10")` = permissive, exactly like Portage.
- Only repo-level files use this reader (`profiles/package.mask`, `profiles/package.unmask`, `profiles/package.use*`); profile nodes always carry their own `eapi` string (review I-7(d)).

## fn repo_file_rules
- Repo-level files parse with `file_eapi` but take `allow_repo` from the computed `repo.eapi` (Portage passes the whole `RepoConfig` to `allow_profile_repo_deps`, not the file-adjacent value) and `allow_build_id` from `"build-id" in formats`. `package.accept_keywords` is the exception: Portage never passes `allow_build_id` there (KeywordsManager.py:49-60), so its call site forces the flag off even with the `build-id` format (S0 `formats` cell proves it).

## fn filter_atom_lines
- `File`: `-` stripping (with the `packages`-only `-*` passthrough) then `*` stripping for `packages` only (util:572-581); the kept line is the ORIGINAL text (`pkg_orig`, same as Portage returning the prefixed string), so downstream `-` removal and `*` classification in `stack_mask_lines`/`fold_package_sets` are unchanged. Failure warns `--- Invalid atom in <source>: <AtomError.message>` (= `str(InvalidAtom)`, S2 contract).
- `Dict`: validates the first whitespace token, keeps the full raw line; with `allow_use=false` a use-carrying atom warns `--- Atom is not allowed to have USE flag(s) in <source>: <atom>` (the atom token as written; Portage prints `str(Atom)`, identical on canonical inputs).
- Warning ranks (0 repo/layout, 1 packages, 2 use, 3 license/properties/restrict/env, 4 keywords, 5 mask) reproduce Portage's manager read order (config.py, UseManager, LicenseManager, KeywordsManager, MaskManager); `resolve_config` stable-sorts and dedupes to exactly-once (owner Q3) — which also collapses the double read of chain `package.use` (`load_scalar_layers` + `fold_package_use`) and the double read of `package.use.*` (`fold_package_use_force_mask` + `use_mask_force_levels`).
- `package.provided` is deliberately NOT filtered here: Portage validates it with `isvalidatom("=" + line)` (config.py:970-1011), a different (S8-family) check; its EAPI 7+ gate is unported too. Residue, same reasoning as above.

## fn user_atom_rules (#341 S6)
- User-config (`/etc/portage/package.*`) and user-profile (`/etc/portage/profile/*`) rules: Portage reads both with `eapi=None` (`verify_eapi=False`) and `allow_repo=True`; only wildcards and build-ids vary per file (oracle Q2e, re-verified live on host Portage 3.0.82.2, 2026-10-12, over the S0 run6 eight lines):
  | file | wildcard | build-id | use deps | Portage reader |
  |---|---|---|---|---|
  | `package.mask`, `package.unmask` | on | on | kept | MaskManager.py:214-232 |
  | `package.keywords`, `package.accept_keywords` (user) | on | on | dropped | KeywordsManager.py:73-104 |
  | `package.use` (user) | on | on | dropped | UseManager.py:337-348 |
  | `package.license` (user) | on | OFF | kept | LicenseManager.py:37-42 |
  | `package.properties`, `package.accept_restrict`, `package.env` (user) | on | on | kept | config.py:857-902 |
  | user-profile `package.mask`, `package.unmask`, `packages` | OFF | on | kept | MaskManager.py:171-193, config.py:655-666 |
  | user-profile `package.use*` (use, use.force/mask, stable.*) | ON | on | dropped | UseManager.py:277-288 (`extended_syntax = eapi is None and user_config`) |
  | user-profile `package.accept_keywords` | OFF | OFF | dropped | KeywordsManager.py:49-60 (same reader as chain profiles) |
- The user profile is Portage's last profile node (`_profile_node(custom_prof, True, True, …, eapi=None-file-or-None, True, …)`, LocationsManager.py:169-189): its `package.mask`/`.unmask`, `package.use*` and `packages` stack after the chain, before the user files. Its `package.unmask` IS read (`portage1_directories=True`); its `package.license` is NOT (LicenseManager only re-reads `profile-license`-format profiles).
- `package.env` use-dep crash NOT ported: Portage's reader accepts `dev-libs/foo[x] file.conf` and then dies with `AssertionError: setcpv recursion detected` on the next resolve (oracle Q2e, S0 cell C-userenv-crash). portuale keeps the line (valid for the reader) and matches it normally; no contract pin covers that cell.
- Residues (no S0 capture covers them; each needs its own probe + pin before implementing):
  - `package.keywords`, chain-side and user-side: Portage merges it into the accept_keywords dict (KeywordsManager.py:25-37 profiles, :73-104 user, with a deprecation `UserWarning` for the user file); portuale reads neither. Same for the user profile's `package.keywords` / `package.accept_keywords` (profile loop, no wildcards; keywords with build-id on, accept_keywords off).
  - An `/etc/portage/profile/eapi` file: Portage reads it (`read_corresponding_eapi_file`, default None) and the user node then parses with that EAPI (wildcards off unless absent/malformed). S6 always uses `eapi=None` per the plan; no fixture has the file.
  - The user profile carries `profile-set` in its formats tuple: Portage's `ProfilePackageSet` may feed its unstarred `packages` lines to `@profile`. S6 only stacks them into `@system`. No capture either way.
  - `use_mask_force_levels` (the per-level USE-flip suggestion stack) has no user-profile level; only the flat force/mask folds do. No capture.

## enum Error::UnsupportedProfileEapi
- `Display` renders Portage's `!!! ParseError: …` line plus the 4-line `Your current profile is invalid…` abort (B-prof-eapi10). The `Unable to parse profile` and not-a-symlink framing lines depend on container layout and are not reproduced (S5 summary). The `!!!` prefix is the CLI protocol: `pretend/mod.rs` prints `!!!`-leading config errors verbatim, everything else with the `emerge: ` prefix.

## struct Config::config_warnings
- Rank-sorted, deduped texts; the CLI prints each once to stderr at config-load position (before `Calculating dependencies`, owner Q2+Q3). Running-root config warnings (`load_running_config`) are still dropped, not printed — same residue class as the user-profile re-read (S6).
