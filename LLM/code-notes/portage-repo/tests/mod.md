# crates/portage-repo/src/tests/mod.rs
Shared test helpers: fixture roots, synthetic configs, resolve/graph harnesses, and the #164 masking-stack legs.

## fn test_config
- Visibility is pinned to a fixed "amd64"-only, no-overrides config here, matching what the fixture's Portage profile chain also happens to resolve to (see fixtures/repo/profiles and test_emerge_pretend_contract.py for the end-to-end version of this).

## fn multi_instance_entry_1010
- Backlog 2026-10 B1 (`LLM/backlog-tasks-2026-10.md`): binpkg-multi-instance builds of one `cpv` are told apart only by `BUILD_ID`.
- Shared fixture for the three tests below, modelled on the Portage `seed-desk` binhost's `app-text/libspectre-0.2.12` -6 (built against ghostscript `0/10.06=`) and -7 (built against `0/10.08=`), listed oldest first exactly like the live `Packages` index.

## fn in_memory_binhost_1010
- The `Packages` index lives only in the in-memory override table (`set_remote_binary_index_override`).

## fn write_snapshot
- Stamp order mirrors Portage's `_write_metadata_file` + `_stamp_metadata_file` order.
- A committed fixture can never carry a valid stamp (`git checkout` sets the package dir's mtime), so every positive-path test stamps at runtime.
- Returns the `metadata` path for tests that append a custom (stale/garbage) stamp themselves.

## fn resolve_update
- For the `upgrade` test and anywhere else `--update`'s own "search for a better version even when the installed one already satisfies the atom" behavior needs exercising directly.

## fn resolve_as_dependency
- Exercises the avoid_update shortcut's own DEPENDENCY-atom code path (that slice's own fix), distinct from `resolve`'s top-level one.
- `selective`'s own value doesn't matter here: `is_top_level=false` alone already satisfies the `!is_top_level || selective` gate.

## fn resolve_not_selective
- See `resolve_pretend`'s own `selective`/`is_top_level` doc comment paragraph. `is_top_level=true` throughout, matching a directly-requested atom.

## fn resolve_update_excluded
- Exercises `--exclude`'s own "leave an installed package alone regardless of --update" precedence directly.

## fn graph_onlydeps
- Backlog #194: a full `ResolveRequest` (like production's own `run_resolve` builds it), since the legacy marshaller answers with onlydeps off.

## fn graph_update_entries
- Backlog #131 S1: the `graph*` helpers above map to name/outcome pairs only, which cannot drive a display order. Argument list mirrors `graph()` with the `update` slot on.

## fn graph_update
- Proves `update` threads through the whole BFS, not just a top-level atom (see `resolve_pretend_graph`'s own doc comment).

## fn resolve_deep_consumer
- The shared probe for the #132/#133 arm tests below: one place holds the long `resolve_pretend_graph` argument list, copied from the sibling test above.
- `root_deps` passes the fixture root itself as the running root (the unit-level counterpart of the contract suite's `PORTAGE_RUNNING_ROOT=$ROOT` pin).

## fn graph_result_real_atoms
- Backlog #57's collisions need two arguments to disagree about one slot, in both argv orders.

## fn graph_entries_with_retry_seeds
- Backlog #68/#72 B2c: the `@world ∪ @selected ∪ @system` atom list Portage's complete-mode retry walks after an uninstall task was selected (`depgraph.py:10365-10383`).

## fn graph_result_autounmask_backtrack
- The `'backtrack` loop re-drives the whole walk on every accumulator growth (Slices 1/4's full cascade), rather than the default collect-and-display-only behaviour.

## fn graph_result_autounmask_cfg_atoms
- Backlog #244's argument-order cells.

## fn aub0_result
- Backlog #244: the six `aub0` argument orders (upstream `test_autounmask_use_breakage`; `aub0a` needs `aub0d[-foo]`, `aub0b` `aub0d[foo]`, `aub0c` `>=aub0d-1`; the fixture profile enables `foo`). Expected values from Portage 3.0.82.2 (`LLM/evidence/2026-09-28-244/fixture-oracle/real/`).

## fn abi_probe_harness
- Backlog #214 (v2 `#24e`) S0 harness. `with_consumer` controls whether the consumer's same-version ebuild (the reinstall Portage forces) is visible in the tree.

## fn residual_161_setup
- Backlog #161 S3.

## struct CtxOpts161
- Backlog #161 S4 shared harness.
- `--buildpkgonly` blanks the run-time keys for a non-binary candidate (the `dep_keys` second arm, backlog #161 S6p).
- Backlog #286: production builds `top_level` from the request atoms; `set_args` holds request atoms expanded from `@world`/`@selected` (see `ResolveRequest::set_args`).

## fn ds_input_161
- Backlog #161 S5: `s2_input` fixes `excluded` to empty; legs needing an `--exclude` entry build the input literally.

## fn run_161
- Backlog #161 S6: `run_pass` takes the request atoms from the context; each leg below resolves a different installed/tree shape and pins the pass entries it settles.

## fn blocker_161_write_pkg
- Backlog #161 S6.

## fn blocker_161_write_pkg_full
- Backlog #161 S6p: the plain helper can only express `DEPEND`/`RDEPEND`, which is why the first `with_bdeps` binary leg walked an ebuild instead of a binary -- see its doc comment.

## fn graph_result_abk0_worldb_on
- Backlog #205, world=B shape shared by the R3/R4 unit tests: installed `abk0b-1` records `RDEPEND=<dev-libs/abk0a-3`. An `abk0b` line in the fixture world file would perturb every other fixture-ROOT test; the seed is the `FX_WORLD_EXTRA=dev-libs/abk0b` equivalent the bed cell stages, and the same trick the #209 `backtrack_zero_feeds_satisfiable_reverse_dep_pins_in_pass` test uses for its world consumer.

## fn assert_triangle_conflict
- Backlog #57 S1.

## fn euf_flat
- Tests that predate the split and don't care about per-layer `package.use` ordering keep calling this; the `package_use` arg lands in the profile (`defaults`) tier.

## mod tests_164
- Backlog #164: `lib.rs` mutation cluster 4. Self-contained so rebases stay mechanical.

## fn parse_license_tree_positions (S1 legs)
- Backlog #164 S1: Portage signals these malformed shapes with a bare `InvalidDependString` (`3rdparty/portage/lib/portage/exception.py:37`); the per-token positions are portuale's own diagnostic precision, and every `pos + N` site below is a mutation target (`+` -> `-`/`*`), so each arm pins its exact number -- including at position 0, where `-` underflows and `*` collapses to 0.

## fn binary_164
- No md5-cache read, no profile recompute (see `candidate_iuse_and_use`'s own Binary arm).

## fn keyword_masked_only_164_needs_keywords_and_only_keywords
- Backlog #164 S2: Portage's `_getmaskingstatus` (`3rdparty/portage/lib/portage/package/ebuild/getmaskingstatus.py:43`) records one `_MaskReason` per failing category (package.mask, KEYWORDS, LICENSE, PROPERTIES, RESTRICT), so "keyword-masked only" means exactly the KEYWORDS reason fires and no other gate trips first.

## fn keyword/mask/license_masked_only_164_invalid_is_never_*_only
- Backlog #153: an `invalid` IUSE-conditional verdict disqualifies every "masked by X alone" claim.

## fn mask_masked_only_164_needs_a_mask_and_nothing_else
- Backlog #164 S2: Portage `_getmaskingstatus`'s own first reason (`package.mask` via `settings._getMaskAtom`, same file): every other visibility check (KEYWORDS, LICENSE, PROPERTIES, RESTRICT) must pass around it.

## fn license_masked_only_164_needs_a_license_and_only_a_license
- Backlog #164 S2: Portage `LicenseManager.getMissingLicenses` (via `_getMaskedLicenses`, `3rdparty/portage/lib/portage/package/ebuild/_config/LicenseManager.py:169,211`) reports a non-empty missing set while every other `_getmaskingstatus` reason stays absent.

## fn use_masked_only_164_needs_an_unsatisfied_use_dep
- Backlog #164 S2: Portage `_iter_match_pkgs` checks a built package's own `[use]` deps against `pkg.use.enabled` (the baked set). The binary shape below needs no repo staging: `binary_use` is the baked set, `iuse` its domain.

## fn repo_164
- Backlog #164 S3: mirrors `blocker_161_write_pkg_full` (ebuild + consistent md5-cache entry, which the md5-cache validation guard requires).

## fn visible_tree_matches_164_lists_only_visible_matches
- Backlog #164 S3: Portage `_pkg_visibility_check` (`lib/_emerge/depgraph.py:7562`) -- here 1.0 is stable-visible and 2.0 is `~amd64`-masked under an `amd64` profile, so only 1.0 shows.

## fn visible_tree_matches_212_ignores_binary_only_masks
- Backlog #212 (v2 `#24c`): a binary-only `slot_operator_mask_built` negative (`!=cpv[binary]`). Portage keys the mask by package object; `match_from_list` ignores USE-deps, so the marker would otherwise match the ebuild too -- see `is_binary_only_mask`.

## fn entry_164
- Backlog #164 S4.

## fn masked_dep_chain_164_walks_to_the_argument
- Backlog #164 S4: Portage `_show_unsatisfied_dep`'s own dep-chain tail (`lib/_emerge/depgraph.py:6471`). Decoys pin every lookup: a same-category other-package `New` entry must become neither the start nor an ascent step, and a non-`NoVisibleCandidate` first entry must not become the start either.

## fn masked_dep_chain_164_installed_parent_reports_the_vdb_repo
- With no vdb entry that repo is `__unknown__` (see `installed_pkg_repo`).

## fn required_use_dep_chain_164_reports_parent_then_argument
- Backlog #164 S5: Portage `_show_unsatisfied_dep`'s own dep-chain tail (`lib/_emerge/depgraph.py:6471`): "first the failing package's direct parent, then that parent's own command-line argument when it is one".

## fn autounmask_dep_chain_164_top_level_change_names_the_argument
- Backlog #164 S6: Portage `_get_dep_chain_as_comment` (`lib/_emerge/depgraph.py:6457`): one `required by <cpv>::<repo>` row per merge-bound parent, then the argument line.

## fn autounmask_dep_chain_164_self_requiring_parent_stops_after_its_row
- This pins the shape that makes `guard -> true` equivalent (see the #164 closeout): advancing onto itself re-breaks on the visited set with the identical chain.

## fn euf_layers
- `euf` names the full split signature; the args after `iuse` are `use_tokens` (profile make.defaults), `conf_use_tokens` (make.conf), `package_use_repo`, `package_use` (profile), `package_use_user`.
