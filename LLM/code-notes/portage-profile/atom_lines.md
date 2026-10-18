# crates/portage-profile/src/atom_lines.rs
Profile-file atom validation (`grabfile_package` / `grabdict_package` port, #341 S5).

## struct NodeAtomRules
- Per-profile-node parse flags: the node's `eapi` string plus `allow_repo` / `allow_build_id` resolved from the containing repo's `profile-formats`, never wildcards (repo profiles pass no `allow_wildcard`; Portage `MaskManager.py:171-193`, `KeywordsManager.py:25-60`, `UseManager.py:277-288` with `user_config=False`). `eapi: Option<String>` stays optional for the user-profile node (`eapi=None` when the file is absent/malformed, LocationsManager.py:169-189) and the user files.
- `allow_profile_repo_deps`: Portage `repository/config.py:1476-1485` is `eapi_has_repo_deps(eapi) or "profile-repo-deps" in formats`; `eapi_has_repo_deps` (eapi.py:111-112) is the `repo_deps` attr, False for every supported EAPI 0-9, True for `None`/unsupported. Computed through S1's `eapi_attrs` so the table stays single-sourced. The user node reuses it over its hard-coded `("profile-bashrcs", "profile-set")` formats (#346 S1).

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

## fn user_atom_rules (#341 S6, narrowed by #346 S1)
- User-config (`/etc/portage/package.*`) rules: Portage reads them with `eapi=None` (`verify_eapi=False`) and `allow_repo=True`; only wildcards and build-ids vary per file (oracle Q2e, re-verified live on host Portage 3.0.82.2, 2026-10-12, over the S0 run6 eight lines):
  | file | wildcard | build-id | use deps | Portage reader |
  |---|---|---|---|---|
  | `package.mask`, `package.unmask` | on | on | kept | MaskManager.py:214-232 |
  | `package.keywords`, `package.accept_keywords` (user) | on | on | dropped | KeywordsManager.py:73-104 |
  | `package.use` (user) | on | on | dropped | UseManager.py:337-348 |
  | `package.license` (user) | on | OFF | kept | LicenseManager.py:37-42 |
  | `package.properties`, `package.accept_restrict`, `package.env` (user) | on | on | kept | config.py:857-902 |
- The user-profile rows moved to `user_node_rules` (#346 S1): the node's EAPI drives the grammar, wildcards only for `package.use*` with no EAPI, `allow_repo` from the node's formats.
- The user profile is Portage's last profile node (`_profile_node(custom_prof, True, True, ("profile-bashrcs", "profile-set"), <eapi file or None>, True, False)`, LocationsManager.py:169-189): its `package.mask`/`.unmask`, `package.use*` and `packages` stack after the chain, before the user files. Its `package.unmask` IS read (`portage1_directories=True`); its `package.license` is NOT (LicenseManager only re-reads `profile-license`-format profiles).
- `package.env` use-dep crash NOT ported: Portage's reader accepts `dev-libs/foo[x] file.conf` and then dies with `AssertionError: setcpv recursion detected` on the next resolve (oracle Q2e, S0 cell C-userenv-crash). portuale keeps the line (valid for the reader) and matches it normally; no contract pin covers that cell.
- Residues (no S0 capture covers them; each needs its own probe + pin before implementing):
  - (none left from this table: `package.keywords` shipped in #346 S2, see below).
- Shipped by #346 S1 (was: residues above): the user profile's `package.accept_keywords` (profile reader, no wildcards/build-ids, node's EAPI), the `profile/eapi` file (wildcards off unless absent/malformed), the user node's `packages` feeding `@profile` (`profile-set` hard-coded in the node's formats), and the `use_mask_force_levels` user-profile level.
- Shipped by #346 S2 (was: the `package.keywords` residue above): the user file feeds the accept dict first with the message-only deprecation warning (KeywordsManager.py:73-104); chain and user-node `package.keywords` extend each candidate's own KEYWORDS (the profile loop, node's EAPI, no wildcards; user-node build-ids off, chain per formats).

## enum Error::UnsupportedProfileEapi
- `Display` renders Portage's `!!! ParseError: …` line plus the 4-line `Your current profile is invalid…` abort (B-prof-eapi10). The `Unable to parse profile` and not-a-symlink framing lines depend on container layout and are not reproduced (S5 summary). The `!!!` prefix is the CLI protocol: `pretend/mod.rs` prints `!!!`-leading config errors verbatim, everything else with the `emerge: ` prefix.

## struct Config::config_warnings
- Rank-sorted, deduped texts; the CLI prints each once to stderr at config-load position (before `Calculating dependencies`, owner Q2+Q3). Running-root config warnings (`load_running_config`) are still dropped, not printed — same residue class as the user-profile re-read (S6).

## struct UserProfileNode (#346 S1)
- The appended user-profile node: `path` plus the node's `eapi` file content (`None` when absent or malformed — never the repo default, never an abort). Portage: `_profile_node(custom_prof, True, True, ("profile-bashrcs", "profile-set"), read_corresponding_eapi_file(custom_prof + os.sep, default=None), True, False)` (LocationsManager.py:169-189); the namedtuple shape is `(location, portage1_directories, user_config, profile_formats, eapi, allow_build_id, show_deprecated_warning)` (repository/config.py:40-51).
- The node's own `parent` file is never walked: the node is appended after `load_profiles`, not visited by `_addProfile`. Verified by S0 u16 (Portage ignores it, `USE="-x"`).

## fn resolve_user_profile_node (#346 S1)
- Gate: `self._user_config and self.profiles` (LocationsManager.py:169) plus `os.path.exists(custom_prof)` — ported as non-empty chain plus `path.exists()`. Without `make.profile` Portage aborts the whole run as invalid-profile (S0 u15: rc 1, different reason than portuale's masked block); portuale resolves with an empty chain and ignores the user files.
- The `eapi` read is `read_corresponding_eapi_file` with `default=None` (util/__init__.py:441-479): exactly-one-line else warn `--- Invalid 'eapi' file …` and default; missing is silent; no support check. Shared with the repo-level reader (`read_adjacent_eapi`); the single call site in `load_scalar_layers` warns once.

## enum UserNodeFile (#346 S1)
- Per-file rule selector for the user node: `Mask` (`package.mask`, `package.unmask`, `packages`), `PackageUse` (`package.use*`), `AcceptKeywords` (`package.accept_keywords`), `PackageKeywords` (node `package.keywords`, #346 S2).

## fn user_node_rules (#346 S1)
- Rules for one user-profile file: the node's EAPI drives the atom grammar (`None` parses permissively, like the user files); wildcards only for `PackageUse` with no EAPI (`extended_syntax = eapi is None and user_config`, UseManager.py:277-288, which also forces `allow_repo` and skips `verify_eapi` there); `allow_repo` from `allow_profile_repo_deps` over the hard-coded user formats (repository/config.py:1476-1485); `allow_build_id` from the node's `True` (repository/config.py:40-51) except `AcceptKeywords`, whose profile reader never passes it (KeywordsManager.py:49-60), and `PackageKeywords` (#346 S2: the user node's formats never carry `build-id`; chain levels keep their own node rules instead).
- Design note: the plan suggested `Vec<ProfileNode { path, kind }>` for the chain; S1 threads one `UserProfileNode` alongside the unchanged `Vec<PathBuf>` chain instead. Every fold receives it with per-file rules from this helper (one mechanism, one gate), while `resolve_profile_chain`'s return type and `chain.last()` (`pkgindex_profile`, the selected repo profile) stay untouched.
