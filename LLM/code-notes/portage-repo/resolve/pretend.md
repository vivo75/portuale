# crates/portage-repo/src/resolve/pretend.rs
Selecting one atom's candidate (`resolve_pretend`).

## fn dependency_avoid_update_candidate
- Portage's `lib/_emerge/depgraph.py` ~8440 (`if inst_pkg is not None and parent is not None and not self._want_update_pkg(parent, inst_pkg): return inst_pkg`); Portage's `_iter_match_pkgs` vardb-sourced USE-dep check for an installed package.
- The valid-flag domain follows Portage's `dbapi._iuse_implicit_cnstr` for a built package (recorded `IUSE` ∪ profile `IUSE_EFFECTIVE` ∪ the package's own recorded `USE`, bug #640318). On EAPI 5+ (all EAPIs here have `iuse_effective`): `_iuse_effective_match` (`elibc_*`/`kernel_*`/…) via `valid_iuse`, plus `_iuse_implicit_built`'s `flag in use` clause. Portage's `_match_use` recomputes the domain the same way rather than reading a persisted `IUSE_EFFECTIVE` file.
- `cat/pkg-1.0` installed only at slot 0 never satisfies a `cat/pkg:2` dependency even when the repo offers `cat/pkg-1.0` in slot 2.
- Portage `_select_pkg_highest_available_imp`'s own early `avoid_update` return for a DEPENDENCY atom (`lib/_emerge/depgraph.py` ~8440: `if inst_pkg is not None and parent is not None and not self._want_update_pkg(parent, inst_pkg): return inst_pkg`) -- the highest installed version of `category/package` that matches `atom_str` (version/slot/repo, via the FULL `candidates` list, deliberately NOT `is_visible`-filtered) and, if the atom carries a USE-dep (`pkg[flag]`), satisfies it against that version's own real, installed vdb `USE`/`IUSE` -- NOT the current tree's, matching Portage `_iter_match_pkgs`'s own vardb-sourced USE-dep check for an already-installed package.
- Portage `dbapi._iuse_implicit_cnstr` for a *built* package on an EAPI 5+ (all EAPIs here have `iuse_effective`): the `is_valid_flag` domain is the recorded `IUSE`, unioned with the profile's `IUSE_EFFECTIVE` (`_iuse_effective_match` -- `elibc_*`/`kernel_*`/…, via `valid_iuse`) AND every flag the package was actually built with (`_iuse_implicit_built`'s own `flag in use` clause, bug #640318: a built package's own `USE` is authoritative for what counts as a valid flag, independent of the profile's current `IUSE_IMPLICIT` -- e.g. an ebuild that has since dropped a flag from its `IUSE`).
- Original comment citations: Portage: lib/_emerge/depgraph.py, _select_pkg_highest_available_imp ~8440; _want_update_pkg; _iter_match_pkgs; dbapi._iuse_implicit_cnstr (bug #640318) recorded `USE` (bug #640318: a built package's own `USE` is authoritative,

## fn already_installed_or_reinstall
- The `BUILD_TIME` is of the local-or-remote binary candidate.
- Shared by both of `resolve_pretend`'s own `!update` shortcut call sites (the early, dependency-only one above `dependency_avoid_update_candidate` feeds, and the later, `matched`-based one further down `resolve_pretend` itself): once an installed version has been chosen to keep, decides between `AlreadyInstalled` and `Reinstall` exactly the same way -- `newuse`/`changed_use`/`changed_deps`/ `changed_slot`/`rebuilt_binaries`/`newrepo` each independently able to trigger a reinstall even though no Portage version change is happening at all.
- `--emptytree`/`-e` (Portage `create_depgraph_params.py:176` -- `myparams["empty"] = True`, `depgraph.py:7889` -- installed packages are not selected as candidates, so every atom in the deep tree resolves to a merge).
- Portuale's candidate pool is already tree-only, so `empty` just forces the "would be `AlreadyInstalled`" fallback to a bare `Reinstall` instead (Portage `output.py`: `attr_display.replace` still set from `vardb.cpv_exists`, no `[oldver]`, no reason -- exactly the portuale's own reasonless `[ebuild R]`).
- Original comment citations: Portage: create_depgraph_params.py:176; depgraph.py:7889; output.py attr_display

## fn slot_operator_binary_masked_version
- Backlog #212 (v2 `#24c`). `match_from_list` ignores USE-deps, so the marker would otherwise match the same-version ebuild too.

## fn is_binary_only_mask
- Backlog #212 (v2 `#24c`). Portage keys by package object.

## fn is_installed_only_mask
- Backlog #276. Portage's `_add_dep` missing-dependency mask (`depgraph.py:3456-3470`) keys by package object, so masking an installed parent hides only that instance -- the same-version ebuild stays visible for Portage's next backtracking try. `split_cpv` rejects the brackets.

## fn slot_operator_installed_masked_version
- Backlog #276.

## fn is_instance_only_mask
- Backlog #212/#276.

## struct SelectSource / SelectOptions
- 11 args trips `clippy::too_many_arguments`; same reasoning as `resolve_pretend_graph`'s identical allow. Backlog #123 for `local_binpkg`.

## fn collect_binary_candidates
- Portage's `dbs` construction (`if "--usepkgonly" not in myopts: dbs.append(("ebuild" ...)); if myopts.get("--usepkg") is True: dbs.append(("binary" ...))`); `is_visible` only consults source-independent fields (package.mask/license/keywords/properties/restrict), never anything ebuild-specific; `filter_usepkg_exclude_include` matches Portage's `break`-out-of-the-loop rejection.
- `--getbinpkg`: Portage's `bintree.isremote` -> not remote once in `_additional_pkgs`.
- `--binpkg-respect-use` (#69 S1): Portage's gate is `reinstall_use or (not installed and respect_use)` -- `--newuse`/`--reinstall` open it on their own, which is what makes a `newflag`-only IUSE difference reject the binary under `--newuse` even with `--binpkg-respect-use=n`.
- `--binpkg-changed-deps`: an explicit `--binpkg-changed-deps=y|n` forces the check on (even under `--usepkgonly`) or off (even for an ordinary `--getbinpkg`).
- `_equiv_ebuild_visible`: `dbs` order is ebuild-first, so Portage's `matched_packages` is non-empty by the time binaries are weighed. Portage's `(use_ebuild_visibility or matched_packages)` gate; `depgraph.py:8027`'s `not use_ebuild_visibility and (usepkgonly or useoldpkg)` guard.
- binpkg-multi-instance: Portage's `_iter_match_pkgs` yields instances newest-first and the selection loop `break`s on the first acceptable one.
- `slot_operator_mask_built` (`depgraph.py:2383`; `resolver/backtracking.py::_feedback_config:237-242` files it under `runtime_pkg_mask`; `_iter_similar_available` (`:3034`) skips masked packages on the re-pass). Portage masks the binary package object only.
- --usepkg/--usepkgonly (Portage depgraph.py's own `dbs` candidate-pool construction, `if "--usepkgonly" not in myopts: dbs.append(("ebuild" ...)); if myopts.get("--usepkg") is True: dbs.append(("binary" ...))`, confirmed by reading it -- see `list_binary_candidates`'s own doc comment): `--usepkgonly` excludes ebuild candidates from the pool entirely; either flag alone makes binary candidates (`<PKGDIR>/Packages`) eligible alongside them.
- Original comment citations: Portage: depgraph.py:8259-8288; gate reinstall_use or (not installed and respect_use) Portage: create_depgraph_params.py:196-203; depgraph.py:8288 ignored_binaries Portage: depgraph.py:8015-8025,8027

## fn resolve_pretend (avoid_update)
- Confirmed live: `sys-fs/fuse`'s `sys-libs/liburing:=[abi_x86_64(-)?,...]` needs the early return -- the tree's only *visible* `liburing` candidate lacks the USE profile to satisfy the atom while the installed version satisfies it via its own vdb `USE`.
- #57: `dev-libs/paired` staying on installed 1.0 while a sibling's `>=paired-2.0` forces 2.0 reconciles on the backtracking retry instead of re-deriving the same conflict.
- #157: gating on `!empty` keeps portuale from reinstalling a keyword-masked installed dependency that Portage downgrades to the best visible stable.
- The constraints are every *other* parent atom that targeted this `cat/pkg:slot` (Portage `_select_pkg_highest_available` weighs the whole atom set, not just the one being resolved), so an installed version one of them rejects is not a version Portage would "avoid updating" to -- it falls through to the ordinary highest-available search, which is what makes a *solvable* installed-vs-merge slot collision reconcile on the backtracking retry (`dev-libs/paired` staying on installed 1.0 while a sibling's `>=paired-2.0` forces 2.0) instead of re-deriving the same conflict on every attempt.
- #157: `--emptytree` skips installed packages as candidates entirely (Portage `depgraph.py:7888-7899`, `if empty and pkg.installed ...: continue`), so the dependency `avoid_update` early return below (`depgraph.py:8453-8458`, `return inst_pkg`) can never fire -- its `matched_packages` holds no installed instance.
- Original comment citations: Portage: depgraph.py:7888-7899,8453-8458

## fn relax_until_usable (levels)
- Portage's `_AutounmaskLevel` is a single mutable object that's re-yielded (`allow_license_changes` sticks once set). Levels 3 & 6 (`missing keywords`) skipped -- no `**` suggestion. Portage bug #463394 for level 4.
- A `*_masked_only` fallback level is worth trying not just when *nothing* passes `is_visible`, but when nothing visible actually satisfies what this atom (and any slot-conflict constraints folded in with it) asks for: Portage `_select_pkg` weighs the whole request before calling a version unavailable, so `>=dev-libs/foo-2` where `foo-1` is stable but only `foo-2` (`~arch`) is in range autounmasks `foo-2`'s keyword rather than reporting the dep missing.
- Original comment citations: Portage: depgraph.py:7446 4. +masks, keywords still respected (Portage bug #463394).

## fn relax_until_usable (installed fallback)
- `dependency_avoid_update_candidate` only reaches atoms with *some* repo candidate (e.g. keyword-masked-but-present). `--usepkgonly` empties the ebuild pool wholesale, so without this an installed-only satisfier would wrongly go `NoVisibleCandidate` and land on the resume list ("no visible ebuild to merge") in a `-K` merge. Narrowed to the L1 test bed case (L1-a); the non-`--usepkg` path keeps reporting a tree-departed ebuild. Queue loop's #233 rewrite handles runtime-keyed deps.
- Portage `_select_pkg_highest_available_imp`'s installed fallback (`depgraph.py` `_iter_match_pkgs(root_config, "installed", atom)` after the merge-candidate search comes up empty): in `selective` mode (the default -- `--emptytree` clears it) an already-installed version that satisfies the atom is accepted as-is (`operation="nomerge"`).

## fn narrow_matched_candidates (matched pipeline)
- `resolve_pretend_graph`'s unsolvable slot-conflict handler adds `!=cat/pkg-<ver>` entries to hide a specific version (of the conflicted package or of a pulling parent) and force a lower one on retry.
- Slot-conflict reconciliation (Portage `_process_slot_conflicts` / `_select_pkg`'s "all the atoms pulling this slot", not just the first): when `resolve_pretend_graph` re-resolves a package that hit a slot conflict, it passes every *other* parent atom that targeted the same `cat/pkg:slot` here -- the winning candidate must satisfy all of them at once.
- S9 (#341): the `usable` gate and the main `matched` filter pair each candidate string with its binary build-id (`match_from_list_with_build_id`), so an `=`+build-id top-level atom resolves only the equal instance (dep:2994-3008).

## fn narrow_matched_candidates (USE filter)
- Same "can't tell, so exclude" precedent `reinstall_flags_for_use_change` sets. `use_masked_only` re-checks `is_visible` (redundant here) and the same "only-a-flip-away" test.
- USE deps (`dev-libs/foo[bar]`/`[-bar]`, `(+)`/`(-)` defaults -- PMS 8.3.4): a post-filter on top of match_from_list's own version/slot/repo matching, exactly where Portage's own `match_from_list` applies its equivalent USE-dep post-pass too -- see `use_deps_satisfied`'s own doc comment (portage-dep) for the ported algorithm and why match_from_list itself doesn't do this.

## fn narrow_matched_candidates (useoldpkg)
- See `set_useoldpkg_atoms`.
- `--useoldpkg-atoms ATOMS` (Portage `depgraph.py:7936` + `matched_oldpkg`/`visible_matches`): for a package matching one of these atoms, prefer a *built* (binary) candidate over any newer *unbuilt* ebuild.
- Original comment citations: Portage: depgraph.py:7936 matched_oldpkg/visible_matches

## fn select_best_candidate (installed checks)
- `installed` (version-only, all slots) is what `dependency_avoid_update_candidate` consumes -- a separate concern from the slot-aware `installed_pairs`. Portage renders `[ebuild NS]`.
- `installed_pairs` carries each installed version's own main slot, so "is this candidate already installed" can be answered the way Portage `output.py::_get_installed_best` does -- against `vardb.match(pkg.slot_atom)` (the resolved candidate's *own* main slot), not merely "this version exists in some slot".
- An installed version in a different slot never makes this a downgrade/upgrade -- it's a `New` into a fresh slot (the renderer's `[ebuild NS]`, see `GraphEntry::new_slot`).

## fn select_best_candidate (installed-only USE selection)
- #74 S1, selection half. Portage's `_want_installed_pkg` keeps an installed package the user did not name as a nomerge selection. Host probe: `dev-lang/rust:1.94.1[llvm_slot_21]` (installed with the flag; the profile's `LLVM_SLOT` expand turns it off for the ebuild) stays `AlreadyInstalled` under `-puDvN`.
- #74 S1, selection half (Portage `_wrapped_select_pkg_highest_available_imp`'s no-autounmask pass): the ebuild candidate is matched against the profile-effective USE and the installed instance against its own built USE (`_iter_match_pkgs`'s `pkg.with_use(self._pkg_use_enabled(pkg))`, `dbapi._match_use`'s built-package branch).

## fn select_best_candidate (update shortcut)
- Portage's `selective`/`is_top_level` semantics; the top-level `selective` arm is already false under `empty` (`selective = selective && !empty`).
- Portage `create_depgraph_params.py:179`: `--emptytree` does `myparams.pop("selective", None)`.
- Original comment citations: Portage: create_depgraph_params.py:179 (`--emptytree` pops `selective`). Portage: depgraph.py:7888-7899

## fn select_best_candidate (best pick)
- Portage's `_wrapped_select_pkg_highest_available_imp` builds `matched_packages` in `dbs` order. `--usepkgonly` has no ebuilds; without `--usepkg` no binaries -- moot either way.
- --exclude/-X: an excluded candidate is never eligible to become the New/Upgrade "best visible candidate" either -- see this function's own doc comment.
- Original comment citations: Portage: depgraph.py:8492 matched_packages[-1]

## fn classify_best_against_installed
- `policykit` (global flag `systemd`'s ebuild now defaults on) shows `[ebuild R] … policykit*` at rc 0; the child-flip block records an autounmask change only when the *effective* USE still can't satisfy the atom (`mime-types [nginx]` -> rc 1).
- `dependency_avoid_update_candidate` above already made the same vdb-USE check and returned `None`, so a `best` that reaches here really is a same-version installed one the atom's `[flag]` isn't satisfied by.
- The child-flip block records an autounmask change only when the *effective* USE still can't satisfy the atom (`mime-types[nginx]` -> rc 1). Forcing a `Reinstall` here (rather than `AlreadyInstalled`) is what routes the entry through that block at all.

## S2 (#362 _want_update_pkg + _too_deep)
- Portage `lib/_emerge/depgraph.py:7304-7333` (`_want_update_pkg(parent, pkg)`): `False` for `--exclude`d pkgs; else `not _complete_mode and (arg_atoms or update) and not _too_deep(depth)` with `depth = parent.depth + 1`, reset to 0 when the dep package matches an argument whose `reset_depth` is true (`_emerge/DependencyArg.py:13-23`, default true; false only for the slot-operator internal sets at `depgraph.py:5474-5481` and the required-set args at `:8730`, neither of which sits in portuale's `ctx.atoms`).
- Portage `:7359-7375` (`_too_deep`): `deep is True` never; else `depth > deep` (`deep` defaults 0). Ported directly as `update_too_deep` (D1); deliberately not negated `Deep::recurses_at` (installed-dep recursion gate, different default).
- Portage `:8445-8458`: with an installed match and `!_want_update_pkg`, the installed package is returned before any update/visibility logic. Ported as the site-A `!want_update` branch returning `avoid_update_outcome`. Portage `:7972-7979` (masked-installed keep under `--update`) falls out of the same branch: no visibility filter.
- Portage `:8311-8329`: the USE-drift reinstall comparison runs per ebuild gated on `cpv in vardb.match(atom)` -- a masked installed package (no visible same-cpv ebuild) never hides the vardb. Ported as the `use_drift && same_cpv_visible` hide condition. `--reinstall-atoms` is not a hide trigger here (the pass fallback flips the kept instance back afterwards); `--rebuilt-binaries` never hides the vardb (a drifting binary is skipped, the installed kept).
- Portage `:8291-8307`: `--changed-deps`/`--changed-slot` `break` out of the installed collection (the ebuild child is selected). Ported as `deps_changed_fires`/`slot_changed_fires` hide triggers. Staged host probe `s2-probe-changeddeps.txt` (`LLM/evidence/2026-10-16-359-exec/`): `--changed-deps` without `--deep` prints the parent alone (drop), with `--deep` the `R` plus the new dep (keep).
- Portage `:4474` (`recurse_satisfied = deep is True or depth <= deep`) + `:4423-4462` (`_ignore_dependency`, incl. the `myarg is None` argument gate): a hidden-vardb ebuild child is dropped unless an argument names it, the installed satisfier is invisible, or the walk recurses. Ported as the drop arm of `avoid_update_outcome`.
- Installed-only versions (no tree candidate at the installed version): Portage walks the vardb independently (`inst_pkg` from `matched_packages`), so `:8445` still fires. Ported as the `best_installed_matching` fallback returning `AlreadyInstalled` directly (no reinstall comparison can run: `:8311` needs a same-cpv ebuild). Gated on the tree knowing every positively-required USE flag (`tree_knows_use_dep_flags`): otherwise Portage reports `Missing IUSE` (host probe `s2-c-needsbuilt-pu`).
- The S2 `!buildtime_hard` gate on that fallback is lifted by #360 S4 (below): Portage keeps installed-only `pg34-gone` for a `BDEPEND` edge (S0 `s0-e-gbapp-pvuD`).

## S4 (#360 installed-only candidates)
- Portage `_wrapped_select_pkg_highest_available_imp` (`depgraph.py:7799`): the `dbs` list is built with the vardb as `"installed"` (`:825`), and `_iter_match_pkgs_any` (`:7085`) walks all of them, so an installed-only package is an ordinary match for every dep kind. Ported as: the site-A installed-only fallback above drops its `!buildtime_hard` gate (Q4 follows the S0 `pg359-gbapp` verdict: Portage `N gextra + N gbapp` rc 0 with and without `--with-bdeps=y`).
- The same rule at the walk layer (`apply_outcome_fallbacks` + the `initial_outcome.rs` minimize prediction): the slot/slot-operator-atom rule is unchanged (installed match suffices, #233), and bare/version-range atoms join it under the "no repo or binary candidate exists for the cp at all" condition (`cp_has_tree_or_binary_candidate`, `graph_lookup.rs`), so a package whose ebuilds are all masked keeps its masked-package diagnostics. `!from_disjunction` and `!empty` stay.
- Top-level arguments under `--update --selective` with no tree/binary candidate but an installed satisfier settle `AlreadyInstalled` (`Total: 0`, S0-backed `360-gone-pvu`); without `--update` the miss stands and the similar-names block is suppressed when the cp is installed (`_show_unsatisfied_dep` `cp_exists` via the installed db, `:7018-7040`; S0-backed `360-gone-pv`).
- The deep walk of a kept installed-only package reads the vdb-recorded snapshot (`installed_walk_metadata`, `installed_meta.rs`): `live_metadata_for_installed` is `None` with no ebuild, and the old early-return stranded the walk (no `N gextra` row). The ` for <root>` suffix on the miss stays out of scope (#395).
- Simplifications vs Portage, all noted: no higher-slot guard in `_iter_atoms_for_pkg` (S3 greedy territory); `--exclude`d packages keep the pre-existing `excluded.is_empty()` gate (no S0 cell distinguishes); `changed_deps`/`slot_changed`/`rebuilt` post-selection handling on a kept installed child is not modelled (flags off by default; no S2 cell exercises them); `satisfied.visible` is proxied by same-cpv ebuild visibility; the `_package_tracker` no-non-installed-match and slot-operator-rebuild provisos of `_ignore_dependency` are not modelled.

## fn resolve_pretend (shared ebuild pool, backlog #441 S1 / #457)
- Keeps `list_candidates`' `Arc` alongside the mutable pool and hands it to the pool memo as the starting-pool identity (pointer comparison instead of deep `Vec<Candidate>` equality). The owned `to_vec` copy stays: binaries are extended and retained in place.

## struct SelectSource (gate, #571)
- New `gate` field: `resolve_pretend`'s ebuild pool (`ebuilds_shared`), `classify_best_against_installed`'s reinstall probes and `collect_binary_candidates`' `changed_deps`/`slot_changed` checks all read through it. Test `SelectSource` literals use `crate::test_gate`.

## fn collect_binary_candidates user-patches retain (bug #917047)
- `--binpkg-respect-user-patches` (default on, also with `--usepkgonly`): rejects a binary whose recorded `USER_PATCHES` differs from the local patch-set digest, so a stale build is rebuilt from source. `=n` skips the comparison outright. Portage: `depgraph.py`, the `binpkg_respect_user_patches` gate.
