# crates/portage-profile/src/tests/resolve.rs
`resolve_config` end-to-end and per-file folding tests over hand-built and fixture trees.

## fn envd_use_tokens_follow_eroot_not_config_root
- End-to-end check against fixtures/repo/profiles (base, arch/amd64 -> multi-parent -> default) + fixtures/etc/portage/make.conf (which sources fixtures/etc/make.local). Traced by hand (base USE="foo" etc.); `env.d` comes from `eroot`, not `config_root` (Portage `_get_env_d`).

## fn resolves_fixture_profile_chain_and_make_conf
- Hand-traced accumulation: base `USE="foo"; USE="${USE} bar"` -> {foo, bar}; arch/amd64 `ARCH="amd64"; ACCEPT_KEYWORDS="${ARCH}"` -> keywords {amd64}; default `USE="-bar baz"` -> {foo, baz}; make.local (sourced first from make.conf) `USE="${USE} localflag"`; make.conf `USE="confflag"`. env.d comes from `eroot`, not `config_root` (Portage `_get_env_d`): with the fixture tree as `config_root` (whose own `etc/profile.env` sets `USE='envdusetestflag'`), an `eroot` without `profile.env` yields no `envd_use_tokens`, while an `eroot` equal to the fixture root keeps the flag.
- Fixture detail: the committed tree's default/parent has a cross-repo entry ("overlay:crossrepo-parent"); production (portuale's pretend.rs, built from Portage `find_repos`) passes "overlay" the same way.
- ACCEPT_LICENSE detail: `repo/profiles/license_groups` defines `EULA="SomeEula"`, `overlay/profiles/license_groups` extends it with "CrossRepoNonfree" -- so "@EULA" expands to both members, main repo first then overlay, rather than staying literal. From Portage `LicenseManager` + `LocationsManager.profile_locations`.
- USE_EXPAND cases pinned here: `dev-libs/wildexpandpkg` (`linguas_*` wildcard), `packageuseexpandpkg`/`hiddenexpandpkg` (`emerge -pv` USE_EXPAND grouping), `dev-libs/singletargetpkg` (per-level fold).

## fn reload_env_d_picks_up_a_rewritten_profile_env
- Backlog #332: Portage `_emerge/Scheduler.py:1899-1913` (`_allocate_config`) calls `config.reload()` before every package task (`portage/package/ebuild/config.py:2691-2699`), so a package built after a merge that installed an `/etc/env.d/*` file sees the new variables. Portage `reload()` reuses the init-time blacklist (Portage config.py:724-743). See `resolve_config`'s doc comment for the broot cut.

## fn reload_env_d_never_admits_a_profile_only_variable
- Backlog #332: Portage `_env_d_blacklist` profile half (`profile_only_variables`, `config.py:734-743`).

## fn cross_repo_profile_parent_resolves_a_named_repos_own_profile
- Proves repo-name lookup doesn't depend on the referencing node's own repo membership (`expand_parent_colon`).

## fn cross_repo_profile_parent_resolves_an_aliased_repo_name
- Portage `_expand_parent_colon` -> `get_location_for_name` is keyed on aliases too, so `resolve_config` gets `repo_aliases`.

## fn same_repo_colon_profile_parent_resolves_within_the_current_repo
- Resolved via `repo_containing`'s longest-prefix-match, not necessarily the main repo.

## fn colon_profile_parent_is_not_expanded_when_the_repo_lacks_portage_2
- Portage `_allow_parent_colon` gate.

## fn diamond_profile_inheritance_does_not_double_apply_a_shared_ancestor
- Since flags are a set, double-application isn't observable via membership; the test proves the visited-set/cycle-safety mechanism (which also protects genuine cycles) doesn't block a legitimate multi-path DAG.

## fn package_mask_unmask_accept_keywords_load_correctly
- Fixture sets no global ACCEPT_KEYWORDS, so the bare atom's Portage `accept_keywords_defaults` substitution has nothing to derive a "~arch" set from; see the dedicated accept_keywords_defaults tests for the substitution producing tokens.

## fn package_mask_atom_removal_applies_across_repo_profile_and_user_sources
- Portage `stack_mask_lines` "ignore_repo" behavior: the profile's unscoped "-dev-libs/a" cancels the now-scoped "dev-libs/a::testrepo". Final: b::testrepo (repo, survives) + d (user).

## fn overlay_package_mask_is_scoped_to_its_own_repo_only
- Main repo has no mask at all; the overlay masks dev-libs/a with a bare atom (no "::repo" part) -- resolve_config must auto-scope it to "::overlay" via scope_repo_mask_lines, so it never also masks a same-named package in the main repo.

## fn overlay_package_unmask_gets_the_same_scoping_as_overlay_package_mask
- Both entries get the identical "::overlay" scoping; the mask/unmask cancellation itself happens downstream in portage-repo's is_visible.

## fn overlay_package_mask_inherits_the_main_repos_own_entries_via_implicit_masters
- Covers the overlay-scoping follow-up documented on `fold_package_masks_and_keywords`.

## fn overlay_package_unmask_does_not_inherit_via_masters
- Portage MaskManager.py's own package.unmask loop never consults masters at all -- only package.mask does. Covers the unmask asymmetry documented on `fold_package_masks_and_keywords`.

## fn explicit_repo_masters_overrides_the_implicit_main_repo_default
- Portage explicit "masters =" (`portage_repo::find_repos`, resolved into `repo_masters` by the caller) fully replaces the implicit "masters the main repo alone" default this crate always fell back to before -- an explicit empty masters list means NO package.mask inheritance at all, even from the main repo.

## fn overlay_package_use_is_scoped_to_its_own_repo_only
- Main repo has no package.use at all; the overlay sets a flag for dev-libs/a with a bare atom (no "::repo" part) -- resolve_config must auto-scope it to "::overlay" via scope_repo_package_use_lines, the same way package.mask's own overlay entries are scoped, so it never also applies to a same-named package in the main repo.

## fn system_packages_stack_across_profile_levels_with_atom_removal_and_star_filter
- The bare "dev-libs/hint" line is a Portage "known but not system" hint, never contributing an atom on its own.

## fn profile_packages_are_the_unstarred_lines_of_profile_set_repo_levels
- Fixture: base "*dev-libs/sysbase" + "dev-libs/baseonly"; leaf "-dev-libs/baseonly" + "dev-libs/leafonly" + "*dev-libs/sysleaf". @profile: ["dev-libs/leafonly"]; @system: ["dev-libs/sysbase", "dev-libs/sysleaf"].

## fn profile_packages_require_the_profile_set_format
- Fixture shape mirrors the committed fixture repo (`profile-formats = portage-2` without `profile-set`).

## fn use_mask_and_use_force_stack_across_levels_but_stay_out_of_use_flags
- Fixture: base make.defaults enables "normalflag" and "maskflag"; use.force forces "forceflag" and "bothflag". Leaf (its own parent -> base): use.mask masks "maskflag" (an otherwise-normal USE flag) and "bothflag" (proving mask wins when a flag is both forced AND masked, matching Portage regenerate()'s update-then-difference_update order). Unlike an earlier portuale version, force/mask are not folded into use_flags (Portage `regenerate()` applies them as the literal last step of its USE walk, strictly after package.use); `portage-repo`'s `effective_use_flags` applies them at that later position instead, so `use_flags` here stays exactly what make.defaults/make.conf alone produced. See `use_force`'s doc comment for the full grounding.

## fn archlist_stacks_across_profile_levels_with_removal_semantics
- Fixture: base declares "amd64" and "x86"; leaf removes "x86" and adds "arm64".

## fn use_expand_variable_names_stack_incrementally_across_profile_levels
- Fixture: base declares `USE_EXPAND="VIDEO_CARDS"`, `VIDEO_CARDS="nvidia"`; leaf declares `USE_EXPAND="PYTHON_TARGETS"`, `PYTHON_TARGETS="python3_11"`.

## fn use_expand_variable_value_expands_with_negation_and_plus_stripped
- A single USE_EXPAND variable's own value exercises all three ordinary incremental token forms in one pass (Portage config.py's own early-expand loop keeps a "-" token's own "-" outside the new prefix, and a "+" token has it stripped, same as any ordinary USE token): "nvidia" adds, "+intel" adds (with the "+" stripped, not baked into the flag name), "-nvidia" then removes the flag "nvidia" itself already added earlier in the very same value list -- final: only video_cards_intel remains.

## fn profile_make_defaults_use_expand_is_folded_per_level_before_package_use
- Portage config.py 2849-2889: a USE_EXPAND value set in a profile level's make.defaults is translated to its equivalent USE flag in that level's make_defaults_use, prepended before the raw `USE=`, so the same level's package.use can still cancel it -- and it is NOT also folded into the high-priority conf tier. Portage skips `configdict["defaults"]` in its re-fold (2964). The neovim `^^ ( lua_single_target_* )` case in miniature.

## fn use_expand_value_set_at_two_profile_levels_stacks_incrementally
- Real Gentoo shape: `profiles/default/linux/make.defaults` sets `VIDEO_CARDS="dummy fbdev"`; `profiles/arch/amd64/make.defaults` sets `VIDEO_CARDS="amdgpu fbdev intel ..."`. VIDEO_CARDS is not in INCREMENTALS, but Portage config.py folds each level's value into that level's USE (2853-2866), and USE is incremental.

## fn use_expand_prefix_wildcard_cancels_parent_level_values
- #67 S2 regression pin: a leaf level's `VIDEO_CARDS="-* intel"` must cancel the parent levels' `video_cards_*` flags (Portage config.py:2925's `-<prefix>_*` removal) while keeping the leaf's own `intel`. Pre-fix the expanded token was `-video_cards_*`, which apply_incremental treated as an exact removal of a flag literally named `video_cards_*` -- a no-op -- so the parent's cards survived.

## fn use_expand_unprefixed_variable_contributes_a_bare_flag_no_prefix
- Real Gentoo's own `USE_EXPAND_UNPREFIXED="ARCH"` mechanism; contrasted with the prefixed case covered by `use_expand_variable_names_stack_incrementally_across_profile_levels`.

## fn package_accept_keywords_stacks_profile_chain_then_user_no_repo_source
- Profile-level entry for "a", user-level entry for "b" -- both must appear, in that order (profile-chain first, matching Portage KeywordsManager.getPKeywords), proving there's no repo-level source at all. Only portuale's existing repo/profiles/package.mask convention would exist as a repo-level source if there were one.

## fn accept_keywords_defaults_substitutes_tilde_arch_for_a_bare_atom
- Portage `accept_keywords_defaults`: the same as if the user had written "dev-libs/foo ~amd64" themselves.

## fn accept_keywords_defaults_excludes_already_prefixed_tokens
- Portage `accept_keywords_defaults` derives only from tokens with `keyword[:1] not in "~-"`.

## fn package_use_splits_repo_profile_and_user_into_their_own_fields
- Repo-level entry for "a", profile-level entry for "b", user-level entry for "c" -- each lands in its own `Config` field at its own Portage USE_ORDER position (the "Config depth" slice). Proves no source is silently dropped and no `-atom` removal happens (package.use is purely additive, unlike package.mask). The "Config depth" slice.

## fn env_layer_stacks_onto_the_config_protect_and_env_unset_folds
- Backlog #219 fix round 1: Portage `const.INCREMENTALS` (`const.py:125-138`); Portage `config.regenerate()` stacks the calling env as the final source (`mydbs = configlist[:-1] + [backupenv]`, `config.py:2735-2736`) -- like `FEATURES`, not last-wins. `emerge --info`'s `settings.get(k)` then prints the stacked, sorted union.

## fn user_package_use_star_atom_folds_onto_the_global_use
- Portage `UseManager.extract_global_USE_changes`: only the USER package.use (`configdict["pkg"]`) `_pusedict` entry is popped.

## fn envd_use_tokens_are_read_from_profile_env
- Portage `env-update` output shape: `export KEY='value'` lines, a plain assignment, and unrelated keys that must be ignored.

## fn package_env_resolves_env_file_use_including_source_and_missing_files
- Fixture: `debug` sources `common` (which sets "commonflag"); `debug` itself sets "dbgflag" plus a non-USE var; `shared` sets "sharedflag -commonflag". Non-USE scalar half: `debug`'s CFLAGS only.

## fn package_env_expands_dollar_vars_against_the_global_map_with_chaining
- Portage `_grab_pkg_env`'s per-package expand map: `${SHARED}` from make.conf (already global), `INNER` from the same file's earlier line, `ACROSS` from an earlier file of the same entry.

## fn package_use_expand_shorthand_applies_prefix_to_following_tokens_on_the_same_line
- Fixture: user-level "dev-libs/a VIDEO_CARDS: nvidia -intel plainflag".

## fn package_use_expand_shorthand_is_user_level_only
- In repo/profile files the syntax is Portage behavior's own literal, unexpanded token (see `parse_package_use_lines`'s doc comment).

## fn package_use_mask_and_force_stack_repo_then_profile_only_no_user_source
- Repo-level entry for "a", profile-level entry for "b" -- both must appear, in that order; a user-level package.use.mask/.force file is deliberately written too, and must be completely ignored -- Portage has no such source at all. Confirmed by reading `UseManager.__init__`'s file/variable table; unlike package.use itself.

## fn use_stable_mask_and_force_stack_profile_chain_only_and_stay_out_of_use_flags
- Matches use_mask/use_force's profile-chain-only sourcing; see `use_stable_force`'s doc comment (stability is per-candidate).
