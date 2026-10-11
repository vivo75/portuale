# crates/portage-repo/src/tests/graph.rs
Graph-level resolution tests over the shared fixture tree.

## fn fixtures_tree_is_the_pmtest_checkout
- Every `#[cfg(test)]` reader in the workspace resolves `fixtures/` through a compile-time `CARGO_MANIFEST_DIR/../..` path, so a missing or half-checked-out sibling would otherwise surface as a pile of unrelated file-not-found failures.
- Markers, not a full inventory: enough to catch a tree that is there but is not the fixture tree (a stale mount, an empty clone, a symlink retargeted by hand).

## fn metadata_file_fields_match_portages_literal_set
- Backlog #109 S2: the format version stays 1 (Portage's `_METADATA_FILE_FORMAT_VERSION`); changing either the set or the version breaks interoperability with Portage's reader/writer on a live vdb in both directions.

## fn in_metadata_file_rejects_contents_and_needed
- Backlog #109 S2: `in_metadata_file` is Portage's `_in_metadata_file()` -- membership in the 23-set (they must never be answered from the snapshot).

## fn emptytree_forces_an_installed_atom_to_a_bare_reinstall
- dev-libs/samepkg is installed at 1.0 and is the highest visible version. Portage `create_depgraph_params.py:176-179` pops "selective" and forces the merge; a bare Reinstall has every field false, exactly Portage's own reasonless `[ebuild R]`.

## fn not_selective_top_level_atom_still_offers_a_newer_visible_version
- dev-libs/upgradepkg is installed at 1.0, 2.0 is visible. The installed version is never even a matched candidate to begin with, so the ordinary "best across everything visible" search proceeds and finds 2.0, exactly as if `update` were true.

## fn selective_true_preserves_the_pre_existing_already_installed_outcome
- Same fixture as the reinstall test above, but selective=true (via the pre-existing `resolve` helper); matching every test written before this slice.

## fn without_update_an_installed_version_that_still_satisfies_the_atom_is_kept
- dev-libs/upgradepkg is installed at 1.0; a newer 2.0 is visible in the tree too. Portage depgraph.py's own `avoid_update` (lines 7814/8448) means plain `emerge dev-libs/upgradepkg`, with no --update, never even looks for a better version -- see resolve_pretend's own doc comment. This was, before this slice, portuale's own (inaccurate) default behavior for `upgrade` below.

## fn upgrade
- Portage depgraph.py's own `dont_miss_updates` branch.

## fn downgrade
- Portage `output.py`'s own in-slot `best()` check (see PretendOutcome::Downgrade's own doc comment) flags this as a genuine downgrade, not an "upgrade" to an older version.
- avoid_update's shortcut doesn't apply here even without --update -- see resolve_pretend's own doc comment on requiring a *visible* candidate.

## fn keyword_masked_but_installed_dependency_is_kept_not_downgraded
- dev-libs/keywordmaskedpkg is installed at 2.0, whose own KEYWORDS ("~amd64") are no longer accepted under the fixture profile's own default ACCEPT_KEYWORDS ("amd64") -- only 1.0 (KEYWORDS="amd64") is currently visible. Reached as a DEPENDENCY atom (is_top_level=false, via `resolve_as_dependency`), Portage depgraph.py's own early `avoid_update` return (`_select_pkg_highest_available_imp`, ~8440: `if inst_pkg is not None and parent is not None and not self._want_update_pkg(...)`) requires NO visibility check at all.
- Confirmed live against a Portage system (`sys-libs/liburing`, installed ~amd64-only, Portage's `emerge --pretend` never even considers downgrading it).
- Before this slice's own fix, portuale incorrectly required the installed version to also be in the visible-filtered candidate set, so this fixture would have (wrongly) resolved to `Downgrade { from: "2.0", to: "1.0" }` instead.

## fn keyword_masked_but_installed_dependency_still_requires_visibility_as_a_top_level_atom
- Portage depgraph.py's own SEPARATE, later `if avoid_update: ... self._pkg_visibility_check(...)` block. `resolve`'s own `is_top_level=true` exercises this, unaffected by this slice's own fix, matching Portage's genuine distinction between the two code paths.

## fn emptytree_downgrades_a_keyword_masked_installed_dependency_to_the_visible_stable
- Backlog #157: dev-libs/needskeywordmasked RDEPENDs on dev-libs/keywordmaskedpkg, installed at 2.0 (`KEYWORDS="~amd64"`, not accepted under the fixture profile's own default `ACCEPT_KEYWORDS="amd64"`) while 1.0 (`KEYWORDS="amd64"`) is the only visible version -- the installed `~arch`-masked version beside a visible stable the filing names.
- Portage drops every installed candidate wholesale (`depgraph.py:7888-7899`: `if empty and pkg.installed ...: continue`), so its dependency `inst_pkg` return (`depgraph.py:8453-8458`) never fires.
- Before #157 the dependency avoid_update shortcut kept the masked 2.0 and reinstalled it, exactly the L3 `sys-apps/portage-3.0.82.2` reinstall / Portage `3.0.81.3` downgrade shape.

## fn changed_use_still_catches_an_enablement_change_on_a_shared_flag
- dev-libs/reinstallpkg's own "foo" flag exists in IUSE on both sides -- only its enablement changed, exactly the shared term both --newuse and --changed-use compute the same way.

## fn changed_use_reinstall_still_recurses_into_its_own_dependencies
- Same proof as newuse_reinstall_still_recurses_into_its_own_dependencies below, but via --changed-use instead of --newuse.

## fn fixture_overlay_only_package_is_found
- dev-libs/overlayonlypkg exists only in the fixture's overlay repo (see fixtures/etc/portage/repos.conf), not the main repo.

## fn fixture_best_version_wins_regardless_of_which_repo_has_it
- dev-libs/overlaynewerpkg-1.0 is in the main repo, -2.0 is in the overlay -- the higher version wins even though it isn't in the main (lower-priority) repo.

## fn fixture_same_version_tie_across_repos_is_broken_toward_higher_priority
- dev-libs/overlaytiepkg-1.0 exists identically-versioned in both the main repo (priority -1000, no deps) and the overlay (priority 10, RDEPENDs on dev-libs/newpkg).

## fn fixture_solvable_slot_conflict_is_reconciled_by_backtracking
- dev-libs/slotconflictparent pulls in slotconflictnewconsumer (bare RDEPEND on slotconflicttarget) and slotconflictoldconsumer (RDEPEND "<dev-libs/slotconflicttarget-2.0"). Backlog #90 (S1).
- The bare dep reuses the already-graphed 1.0 (Portage `_select_package`'s existing-node return).
- The backtracking retry path this test used to exercise (2.0 graphed first, then reconciled to 1.0) no longer triggers here; the unsolvable twin below still covers the recorded-conflict shape.
- Portage `--debug` never prints `Slot conflict handler started.` for this fixture (S0 matrix oracle).

## fn backtrack_zero_feeds_satisfiable_reverse_dep_pins_in_pass
- Backlog #209. Portage `_resolve_conflicts` (`_emerge/depgraph.py:9444`) calls `_complete_graph()` (`:8562`) with no `_allow_backtracking` gate.
- Hermetic r25 shape (#25 S0): installed world consumer `r25consumer` pins `<r25lib-2.0:=` (both bound forms), and `-uD r25target` must withhold `r25lib-2.0` silently -- `backtrack_restarts` 0 (Portage `backtrack: 0/20`) and no skipped-update rows (Portage merges `r25up-2.0 + r25target-1.0` with no warning).
- The fixture world file does not carry the consumer (it would perturb every other fixture-ROOT test), so the seed is set directly -- the `FX_WORLD_EXTRA=dev-libs/r25consumer` equivalent the bed cell stages.

## fn two_atom_old_new_withholds_with_a_skip_notice_at_any_budget
- Walk-level S0 matrix cells (old,new): the direct solve removes 2.0 in-pass, so the retry has nothing to do; the merge list settles on 1.0. Portage's rows + `WARNING`, rc 0 (staged oracle both budgets).

## fn fixture_unsolvable_slot_conflict_is_resolved_by_masking_a_puller_version
- dev-libs/btparent -> btconsumer (resolves -2.0, RDEPEND >=bttarget-2.0) + btpin (RDEPEND <bttarget-2.0). Slice 1's solvability check fails; C3's ranked choices mask bttarget-2.0 on one node; the retry finds btconsumer-2.0's dep unsatisfiable (mask-aware missing-dep probe) and masks its parent -- the same downgrade the old puller pre-masking produced.

## fn fixture_masking_trial_is_reverted_when_it_makes_a_dependency_unsatisfiable
- Slice 3 must revert the trial (Backlog #90 S1: instances now file oldpin's 1.0 first -- Portage's own notice order on the staged oracle).

## fn fixture_slot_conflict_carries_per_instance_pulled_in_by_data
- Portage `slot_collision_handler`'s per-slot_atom (pkg, parent_atoms).
- slotconflicttarget and both pins declare no IUSE; expected rendering grounded in Portage `/usr/sbin/emerge` 3.0.82.2 `--pretend --ignore-default-opts dev-libs/slotconflictunsolvable` on the staged fixture (backlog #264).

## fn fixture_slot_conflict_carries_pkg_use_display
- dev-libs/scuseparent -> scusenewpin (>=scusetarget-2.0) + scuseoldpin (<scusetarget-2.0): unsolvable slot conflict on scusetarget:0. Expected rendering grounded in Portage `/usr/sbin/emerge` 3.0.82.2 `--pretend --ignore-default-opts dev-libs/scuseparent` on the staged fixture (backlog #264).
- scusenewpin IUSE="+scupin" -> that parent line renders `USE="scupin" ELIBC="glibc"`; scuseoldpin has no IUSE -> `USE="" ELIBC="glibc"`.
- Backlog #90 (S1): instance order follows the walk -- the oldpin's `<2.0` resolves first (reversed declaration order, Portage's stack discipline); staged oracle: 1.0 pulled by oldpin, then 2.0 by newpin.

## fn fixture_unsolvable_slot_conflict_survives_backtracking_and_is_reported
- dev-libs/slotconflictunsolvable pulls in slotconflictnewpin (RDEPEND ">=dev-libs/slotconflicttarget-2.0") and slotconflictoldpin (RDEPEND "<dev-libs/slotconflicttarget-2.0"). The backtracking solvability pre-check fails. Backlog #90 S1: oldpin's instance files first -- Portage's notice order -- since the walk resolves it first.

## fn fixture_different_slots_of_the_same_package_coexist_without_conflict
- Portage: different slots of the same package are normal coexistence (like dev-lang/python:3.11 and :3.12), not a conflict; neither is silently dropped by the visited-set the way v1 always did before slot tracking existed.

## fn fixture_new_slot_install_is_new_not_an_upgrade
- dev-libs/newslotpkg-1.0 (SLOT 0) is installed; -2.0 (SLOT 1) is not. Portage's `_get_installed_best` (`not myinslotlist` while `vardb.match(cp)` is non-empty) makes this a `New` with `new_slot=True` (`[ebuild NS]`).
- Before installed matching was slot-aware portuale mis-reported it as `Upgrade { from: "1.0", to: "2.0" }` (both this crate and the Python oracle agreed, and both were wrong).

## fn fixture_in_slot_request_is_not_flagged_new_slot
- Requesting the slot that IS installed (`:0`, holding newslotpkg-1.0) stays an in-slot outcome -- selective (this helper's mode) keeps the installed version untouched, and `new_slot` is never set for anything but a `New`.

## fn fixture_interactive_bracket_column_tracks_evaluated_properties
- Portage output.py:833's `attr_display.interactive`; the conditional gate is `_PackageMetadataWrapper`'s `"?" in v` check.
- The merge-bound `Reinstall` case -- an already-installed interactive package under non-selective resolution rendering `[ebuild Ir]` -- is covered end to end by the contract suite.
- dev-libs/interactivecondpkg's `interactive` token sits behind a `gtk? ( ... )` conditional with `gtk` disabled -> the Portage USE-conditional evaluation (`_PackageMetadataWrapper`, `"?" in v`) gates it out.

## fn fixture_fetch_restrict_column_tracks_distdir_state
- The test helpers pass `fixtures_root().join("distfiles")` as `distdir`. Portage `output.py:633-641`'s `fetch_restrict` / `not getfetchsizes(only_restricted=True)`.

## fn fixture_virtual_resolves_through_ordinary_category_and_any_of_machinery
- virtual/texteditor is shaped exactly like a genuine virtual (e.g. virtual/pager in the genuine Gentoo tree, confirmed by inspection): an ordinary ebuild whose RDEPEND is a "|| ( dev-libs/newpkg dev-libs/samepkg )" any-of group of genuine providers.
- Portage `dep_zapdeps` preference (see `use_reduce_flat_disjunctive`'s `AltPreference`, portage-use-reduce): dev-libs/samepkg is already installed (choice bin 0, `preferred_installed`) so it wins over the first-listed dev-libs/newpkg (bin 1, needs a merge) -- exactly how Portage resolves `virtual/wine` -> the installed `wine-staging` instead of the first-listed `wine-vanilla`.

## fn fixture_provenance_records_the_keyword_entry_that_was_actually_needed
- dev-libs/wildcardkeywordpkg is ~amd64-only, accepted only via the "*/wildcardkeywordpkg" package.accept_keywords entry (see fixtures/etc/portage/package.accept_keywords) -- the global ACCEPT_KEYWORDS alone doesn't accept it.

## fn fixture_provenance_records_no_keyword_entry_when_global_keywords_suffice
- dev-libs/newpkg is a plain stable-amd64 package needing no package.accept_keywords help at all.

## fn keyword_mask_marker_classifies_the_bracket_column
- Plain stable-amd64: no marker (globally keyword-visible). ~amd64 via a package.accept_keywords entry (a testing keyword for our own arch) -> '~'.

## fn fixture_package_mask_minus_atom_removal_is_unaffected
- dev-libs/samepkg is masked then immediately un-masked via "-dev-libs/samepkg" within package.mask itself -- it must resolve completely normally (already installed), not as if it were ever masked.

## fn fixture_unrelated_masked_by_keywords_package_is_still_hidden
- The "*/wildcardkeywordpkg" package.accept_keywords entry is scoped to that package name only (not "dev-libs/*"), specifically so it can't accidentally make dev-libs/maskedpkg visible too.

## fn fixture_eula_style_license_is_masked_by_the_portage_default_accept_license
- Neither the fixture profile chain nor make.conf sets ACCEPT_LICENSE at all; profiles/license_groups defines EULA="SomeEula", so dev-libs/eulapkg's own LICENSE="SomeEula" is masked.
- Portage's own "* -@EULA" default (dev-libs/eulapkg is the fixture pin for it).

## fn fixture_package_license_unmasks_an_otherwise_eula_masked_package
- fixtures/etc/portage/package.license accepts SomeEula for this one package specifically, despite the same global "* -@EULA" default that masks dev-libs/eulapkg above.

## fn fixture_package_properties_narrows_acceptance_for_one_package
- fixtures/etc/portage/package.properties revokes "interactive" for dev-libs/interactivepkg specifically, despite the genuine default ACCEPT_PROPERTIES=* that leaves dev-libs/propertiespkg (above) visible.

## fn fixture_package_accept_restrict_narrows_acceptance_for_one_package
- fixtures/etc/portage/package.accept_restrict revokes "bindist" for dev-libs/restrictedpkg specifically.

## fn sibling_package_prefix_does_not_contaminate_vdb_scan
- Regression test over the vdb category-directory scan. Without the digit-start guard in strip_version_prefix, this would wrongly report an Upgrade from a bogus "bar-2.0" pseudo-version instead of a clean New.

## fn onlydeps_with_rdeps_and_ideps_blank_the_root_dep_classes
- Backlog #194: exactly like Portage `depgraph.py:4186-4193` (sets asserted against the `/tmp/opencode/n194/oracle_odw.log` playground oracle).

## fn onlydeps_eapi7_root_ignores_idepend
- Backlog #194: even the default rdeps-on shape merges no `E`.

## fn disjunction_preference_matches_an_installed_alternatives_vdb_use
- Backlog #74 S1: Portage `dep_check.dep_zapdeps`'s `_dep_check_composite_db` covers the vardb, and `dbapi._match_use` has a separate built-package branch that reads `metadata["USE"]`.
- `fixtures/repo/dev-libs/altconsumer-1.0` BDEPENDs `|| ( dev-libs/altprov:1.1[flip] dev-libs/altprov:1.0 )`; `fixtures/var/db/pkg/dev-libs/altprov-1.1` is installed with `USE=flip` while the fixture profile/environment never enables `flip`.
- Before the fix the installed alternative ranked `UnsatUseInstalled` and the uninstalled slot's cp-level `Installed` won, so portuale merged `dev-libs/altprov-1.0`; Portage's own container capture (findings "#74 S0") keeps the installed `:1.1` (bin 0 plus the `all_installed_slots` promotion).

## fn exclude_threads_through_dependency_recursion_not_just_top_level
- dev-libs/upgradepkg is reached only as a dependency of dev-libs/withdeps here, never a top-level atom (see resolve_pretend_graph's own doc comment).

## fn rebuild_if_unbuilt_reinstalls_a_consumer_of_a_merged_build_dep
- dev-libs/rebuildconsumer (installed) has a vdb DEPEND on dev-libs/rebuildtrigger; `emerge -u rebuildtrigger` upgrades it 1.0 -> 2.0 (it isn't otherwise in the graph at all).

## fn dynamic_deps_picks_ebuild_vs_vdb_deps_for_an_already_installed_deep_walk
- changeddepspkg (installed) has vdb RDEPEND=dev-libs/samepkg but its current ebuild RDEPEND=dev-libs/newpkg. Driven as AlreadyInstalled by NOT requesting a reinstall: resolve_pretend in test_config keeps a directly-named installed atom as AlreadyInstalled (selective=true here), so --deep recurses via enqueue_dependencies.
- dynamic (default): the current ebuild's RDEPEND -> newpkg (New). =n: the vdb RDEPEND -> samepkg (installed) -> newpkg not pulled.

## fn default_dynamic_deps_walks_the_live_ebuild_alone
- dev-libs/builtbindpkg is installed with vdb RDEPEND="dev-libs/builtbindtarget:0/1=" while its current ebuild RDEPEND is "dev-libs/newpkg" (the binding was dropped from the tree).
- Portage reads the installed EAPI as `"0"` (`vartree.py:1054-1055`, backlog #25 S1b). `--ignore-built-slot-operator-deps` changes nothing on this shape. (The EAPI-bearing r25 shape walks both; the ungated append broke both #24 pins, Phase 5 S1 S0.)

## fn deep_walk_evaluates_flag_deps_against_the_installed_vdb_use_not_effective
- dev-libs/deepvdbusepkg is installed with vdb USE="" (no `wantdep`); its IUSE is `wantdep` and fixtures/repo/profiles/base/package.use enables it in the *effective* set. Its current ebuild RDEPENDs `wantdep? ( dev-libs/deepvdbusetarget )`, reached only via the --deep recursion under dev-libs/deepvdbuseconsumer (New).

## fn deep_walk_evaluates_conditional_use_deps_of_installed_parents
- Backlog #139 (#132's missing Rust-side test): the `deepusedep*` arms pin `enqueue_dependencies`' evaluation rule at the graph level. The conditional form never arrives unevaluated (where it imposes no state constraint at all).
- Arm A: the child is present as an entry but never as a satisfied install, so the abort path (pinned at the CLI by the #132 contract test) has something to report.
- Arm B: a fix that only hardened the failure display would pass arm A and fail here.
- Arm C: the graph still records the walked installed nodes as `AlreadyInstalled` entries -- unlike the CLI merge list -- so the assertion is "no merging entry", not single-entry. Guards against an over-broad fix.

## fn root_deps_walk_evaluates_conditional_use_deps_before_the_satisfied_check
- Backlog #133 at the graph level (the contract pin is `test_root_deps_evaluates_conditional_use_deps_before_the_satisfied_check`): `dev-libs/deeprootdepparent` is installed with vdb USE="" (built -flip) and BDEPENDs `~dev-libs/deeprootdepchild-1.0[flip=]`; the running root has the child installed +flip. Otherwise the raw form is vacuously satisfied and the child vanishes (bed `l0-fx-20260922T082944Z`: Portage rebuilds it with flip off).

## fn root_deps_walk_evaluates_conditional_use_deps_before_the_satisfied_check (control)
- Control without `--root-deps`: the producers are a strict no-op (`root_deps_running_root` None) and the ordinary S5-evaluated walk rebuilds the child against the target root instead.

## fn root_deps_build_entry_displays_the_rebuild_use_column
- Backlog #141 (Phase 5b S1): the renderer must print Portage's `USE="-flip*"` column (bed `l0-fx-20260922T192350Z`) instead of skipping the display. The contract pin is `test_root_deps_evaluates_conditional_use_deps_before_the_satisfied_check`; this is the `cargo test` gate over the same constructor (#141 kept in the assertion message below).

## fn emptytree_reinstalls_the_whole_deep_dependency_tree
- Without --deep/--emptytree, `graph("dev-libs/deeppkg")` on an installed deeppkg shows only itself (AlreadyInstalled, or a top-level bare Reinstall) and never walks deeppkg2/newpkg. The whole chain shows: deeppkg + deeppkg2 as Reinstall (both installed), newpkg as New (not installed).

## fn with_bdeps_default_true_walks_depend_and_bdepend_of_an_already_installed_package
- withbdepspkg is already installed, RDEPENDs on newpkg, DEPENDs on builddeponlypkg, BDEPENDs on hostdeponlypkg. Portage's --with-bdeps=auto/y (portuale's own --usepkg-less default); same as before --with-bdeps existed.
- Merge order follows Portage's own discovery order via `portage_discovery_order`: RDEPEND's newpkg is discovered before DEPEND's builddeponlypkg, which is discovered before BDEPEND's hostdeponlypkg (`_add_pkg_dep_string`'s `deps` tuple, `depgraph.py:4253-4289`).

## fn with_bdeps_false_skips_depend_and_bdepend_but_not_rdepend
- Portage depgraph.py's own "if pkg.built and not removal_action": --with-bdeps=n only ever drops DEPEND/BDEPEND for an already-built (here: AlreadyInstalled) package -- RDEPEND is never affected, so newpkg (RDEPEND) still shows up while builddeponlypkg (DEPEND) and hostdeponlypkg (BDEPEND) don't.

## fn root_deps_adds_a_real_build_entry_for_an_unsatisfied_bdepend_with_a_visible_ebuild
- Portage's "recursively pull in and build a new package against the running root" (see `resolve_root_deps_build_entries`'s own doc comment): `rootdepsbuildpkg`'s own `BDEPEND` (`dev-libs/rootdepsbuildtool`) has a genuine, tree-visible ebuild, but no vdb entry anywhere -- so under `--root-deps`, it's neither dropped as already-satisfied (unlike `rootdepsprovider` above) nor reported as `NoVisibleCandidate` (unlike the same atom would be *without* `--root-deps`, since it's genuinely visible in the tree).

## fn root_deps_recursion_walks_a_build_entrys_own_build_and_runtime_deps
- Recursive successor to the test above (see `resolve_root_deps_build_entries`'s own doc comment): `rdrapp` BDEPENDs `rdrtool`, which itself BDEPENDs `rdrtooldep` and RDEPENDs `rdrlib`.

## fn root_deps_recursion_walks_a_build_entrys_own_idepend
- Portage: `IDEPEND` *always* targets `_running_root.root` (`depgraph.py:4247-4252`). `rdriapp` BDEPENDs `rdritool`, whose own `IDEPEND` (`rdrilib`) must be pulled in as its own `targets_running_root` entry.

## fn root_deps_top_level_idepend_resolves_against_the_running_root
- Not just recursed running-root build entries: `IDEPEND` always targets `_running_root.root` for every package. `topidepapp` IDEPENDs `topideplib`; under `--root-deps` `topideplib` becomes a `targets_running_root` entry rather than an ordinary ROOT entry.

## fn cross_root_bdepend_self_branch_merges_in_both_roots
- Track X Slice C (#242): `g216top` RDEPENDs `g216mid` RDEPENDs `g216comp` BDEPENDs `|| ( >=dev-lang/g216comp-1.0 dev-lang/g216boot )`. Portage's pass 1 keeps the in-graph self branch live and dead-ends on the running-root self cycle (`_serialize_tasks`, `depgraph.py:10262`), then re-resolves with the `circular_dependency` map to `g216boot` (`backtrack: 1/20`, rc 0).
- `g216comp` lands TWICE, one row per root (Portage `output.py` `get_display_list`; `LLM/evidence/2026-09-29-242-inventory/probes/default-g216top.txt`).
- Hermetic running root: nothing installed there, so every build dep selects from the tree.
- Portage spends its single backtrack retry on the serialize abort.

## fn root_deps_recursion_terminates_on_a_mutual_bdepend_cycle
- `rdrcyca` BDEPENDs `rdrcycb` BDEPENDs `rdrcyca`.
- The shared `root_deps_build_seen` set (inserted *before* a package's own deps are walked) must cut the recursion so each cycle node appears exactly once, not overflow the stack.

## fn root_deps_recursion_surfaces_an_unbuildable_build_dep
- Per the confirmed judgment call for this slice: `rdrmisstool` BDEPENDs `rdrnothere`, which has no ebuild anywhere -- previously `--root-deps` silently swallowed it.

## fn deep_is_ignored_when_nodeps_disables_the_dependency_walk_entirely
- Portage `create_depgraph_params.py` pops "recurse" out of myparams outright, which the dependency walk itself checks for before `deep` is ever consulted.

## fn recursion_basic_chain
- dev-libs/upgradepkg is installed at 1.0 with a newer 2.0 visible in the tree too -- same as resolve_pretend's own without_update test above, just reached here as a dependency instead of a top-level atom.

## fn recursion_keeps_a_keyword_masked_but_installed_dependency_instead_of_downgrading
- dev-libs/needskeywordmasked (New) RDEPENDs on dev-libs/keywordmaskedpkg, installed at 2.0 (KEYWORDS="~amd64", not accepted under the fixture profile's own default ACCEPT_KEYWORDS="amd64") -- reached only as a dependency, never a top-level atom. The live `sys-fs/fuse` -> `sys-libs/liburing` dependency shape, confirmed live against a genuine system (see the resolve_pretend-level test above for the full citation). Before this slice's own fix, this would have (wrongly) shown a Downgrade entry to 1.0.

## fn recursion_keeps_a_keyword_masked_but_installed_dependency_with_a_satisfied_use_dep
- dev-libs/needskeywordmaskeduse (New) RDEPENDs on dev-libs/keywordmaskedusepkg[flag]. Installed at 2.0 with genuine vdb USE="flag" (see the fixture's own IUSE/USE files) -- checked against that genuine vdb-recorded USE, not the current tree's, via `use_deps_satisfied` -- matching Portage `sys-fs/fuse`'s own genuine `sys-libs/liburing:=[abi_x86_64(-)?,...]` dependency (the actual genuine-world case this slice's own fix was built for, confirmed live: without checking USE-deps against vdb data, an earlier version of this fix would have (wrongly) fallen back to the old, visibility-gated lookup for ANY USE-decorated atom, missing this exact case entirely).

## fn recursion_keeps_an_installed_dependency_whose_use_dep_flag_is_only_in_its_built_use
- dev-libs/needsbuiltusediverge (New) RDEPENDs on dev-libs/builtusedivergedep[divergedflag]. The tree candidate can't satisfy the `[divergedflag]` USE-dep at all (top-level `dev-libs/builtusedivergedep[divergedflag]` resolves to NoVisibleCandidate). Portage `dbapi._iuse_implicit_built` (bug #640318): a built package's own USE is authoritative for what counts as a valid IUSE flag -- no spurious NoVisibleCandidate.

## fn recursion_basic_chain_with_update_upgrades_the_dependency
- dev-libs/upgradepkg reached only as a *dependency* of withdeps, never a top-level atom (see resolve_pretend_graph's own doc comment).

## fn nodeps_resolves_the_top_level_atom_but_never_recurses
- Same dev-libs/withdeps fixture as recursion_basic_chain above (which would otherwise pull in newpkg and upgradepkg).

## fn nodeps_still_computes_use_flags_display_for_the_top_level_atom
- Unrelated to whether its dependencies get walked. dev-libs/useflagpkg's own foo?-gated RDEPEND on dev-libs/newpkg proves the *dependency* walk really is skipped (no second entry), while its own IUSE="foo missingflag" still shows up correctly.

## fn use_expand_hidden_group_is_absent_from_the_pv_display
- cpu_flags_x86_sse2 is a genuine enabled flag.

## fn entries_are_returned_in_dependency_first_merge_order
- `topological_merge_order`: Portage's `mylist` is a genuine topological merge schedule. `dev-libs/withdeps` RDEPENDs `dev-libs/newpkg` + `dev-libs/upgradepkg`; both deps must precede `withdeps`, and among the two unconstrained deps their BFS-discovery (RDEPEND string) order is preserved.
- Diamond: a strict topological order.

## fn required_by_names_the_single_owner_of_a_plain_dependency
- dev-libs/withdeps RDEPENDs on newpkg and upgradepkg.

## fn required_by_lists_every_owner_of_a_diamond_dependency
- dev-libs/common is reached via both shared-a and shared-b.

## fn required_by_is_set_on_every_slot_of_a_multi_slot_dependency
- dev-libs/multislotparent RDEPENDs on both dev-libs/multislotpkg:0 and :1. The merge post-pass previously used a destructive `required_by_map.remove(...)`, handing the owner to whichever slot's entry came first and leaving the other with `[]` (which `--tree` then dropped to its flush-left safety net).

## fn a_slot_qualified_dep_does_not_edge_to_a_sibling_slot
- `slotorderroot` RDEPENDs `dev-libs/slotorderdual:2`; `:1` is pulled only through `slotorderb`. Before per-atom edge matching, the slot-blind `cat/pkg` lookup gave `slotorderroot` a phantom edge to `:1` too (and the `required_by` fallback did the same in reverse), so both `slotorderdual` slots reached parent-count 2 and `_merge_order_bias` promoted them ahead of the plain leaf `slotordera` (parent-count 1, declared first).

## fn virtual_cycle_aborts_like_portage_on_the_upstream_fixtures
- Backlog #193, upstream `test_virtual_cycle.py` (bug 965570) as emitted by pmtest `89d17e0` (`ResolverPlayground` success=False, virtual_cycle={gzip-1} / {A-1,B-1,C-1}; g193 container probe `emerge -p --color=n app-misc/{bar,foo}` on the staged tree: rc 1 with the `!!! virtual cycle detected:` block). Portuale merged both as ordinary rings (rc 0).

## fn conditional_grandparent_with_undeclared_flag_is_masked_invalid_before_any_cycle_analysis
- Backlog #153: the fucyclea/fucycleb USE-gated cycle is still in the tree, but the top-level target dev-libs/fucyclec constrains the solution flag conditionally (`dev-libs/fucyclea[x?]`) with `x` in no IUSE of its own -- Portage masks the whole candidate `invalid` at construction (`Package._validate_deps`), so the resolve fails with the all-masked report before any cycle is ever walked: there is no circular-dep suggestion to keep. (This test previously pinned the followup suggestion; that output described a merge Portage never makes.)

## fn large_cycle_count_and_display_fire_on_a_four_ring
- dev-libs/cyc4a through dev-libs/cyc4d form a four-ring of build-time deps (the cyc4a edge gated behind USE=x, on by default): Portage `get_cycles` records one ring per node, so four records trip `large_cycle_count`, and the stuck remainder re-displays all four members in leaf-drain order.

## fn small_cycles_report_without_large_count_or_display
- Control: the two-ring reports its error block but neither the trailer (two records) nor ... -- well, the re-display still shows (two members); only `large_cycle_count` stays false.

## fn masked_dep_report_carries_the_chain_to_the_argument
- End to end through the driver: the maskneedpkg resolve records one masked_deps report (atom + masked list).

## fn mia0a_cells_report_missing_iuse_for_the_conditional_dep
- End to end through the driver for #265(a): both `mia0a` cells record one `Missing IUSE: foo` disclosure on `mia0b`. Portage's `(dependency required by …)` lines in `LLM/evidence/2026-09-30-265/real/mia0a-{1,2}.txt`.

## fn use_unsat_dep_report_carries_the_chain_to_the_argument
- End to end through the driver: the unsatuseiuse resolve records one Missing IUSE disclosure.

## fn unreachable_installed_pin_does_not_block_an_upgrade
- Backlog #54: dev-libs/keeper-1.0 (installed, outside every target closure, and outside @world/@selected/@system in the checked-in fixture) records `=dev-libs/paired-1.0` -- but `graph_result_real` resolves through a single non-complete-mode pass (no `complete_seed_atoms`, matching Portage never entering complete mode here at all: hermetic Portage 3.0.82.2 never walks to keeper), so the explicit `=dev-libs/paired-2.0` request merges cleanly with no residual conflict (verified live, `TEST/findings/l0-fixture-oracle.md` "#54 S0" hermetic row).
- Before the #54 fix this asserted the opposite: a residual conflict pairing the merge against installed 1.0 with keeper as parent -- portuale's unconditional (unreachable-consumers included) vdb reverse scan applying a pin Portage never sees.

## fn build_time_cycle_between_two_installed_packages_is_not_a_hard_cycle
- instcyclea BDEPENDs instcycleb BDEPENDs instcyclea, both already installed. Portage `DepPriority.satisfied` demotes a build-time edge whose dep an installed package provides, so `_serialize_tasks` can break the cycle. Under --emptytree both are `[ebuild R]`; `circular_deps` must stay empty (contrast `hardcyclea`/`hardcycleb`, neither installed -> reported).

## fn or_dep_prefers_a_branch_already_merge_bound_in_the_graph
- ingraphsvc RDEPENDs `|| ( dev-libs/ingraphkeyring dev-libs/ingraphwallet[keyring] )`. Neither branch is installed. ingraphany pulls ingraphwallet[keyring] directly *and* ingraphsvc.
- Control: with nothing pulling ingraphwallet in independently, the same `||` falls back to the first-listed branch.

## fn recursion_prefers_the_installed_any_of_alternative_over_an_earlier_uninstalled_one
- dev-libs/anyof's own RDEPEND is "|| ( dev-libs/newpkg dev-libs/samepkg )". dev-libs/newpkg (listed first) would need a merge; dev-libs/samepkg (second) is already installed. Portage `dep_zapdeps` files samepkg in choice bin 0 (`preferred_installed`, aliased to `preferred_in_graph`) and newpkg in bin 1 (`preferred_non_installed`), and takes the first entry of the best non-empty bin. See `use_reduce_flat_disjunctive`'s `AltPreference` (portage-use-reduce) and `atom_cp_installed`.

## fn real_resolved_use_flags_drive_dependency_recursion
- End-to-end wiring check using the profile-resolved config (via portage-profile, a dev-dependency only) instead of the fixed test_accept_keywords()/empty-USE sets every other test in this file uses.
- fixtures/repo/profiles resolves "foo" enabled and "missingflag" disabled (see portage-profile's own fixture test), so useflagpkg's `foo? ( dev-libs/newpkg )` must be pulled in and its `missingflag? ( dev-libs/hiddendep )` must not be.

## fn use_expand_flag_drives_dependency_recursion
- profiles/base/make.defaults declares USE_EXPAND="VIDEO_CARDS" and VIDEO_CARDS="nvidia".

## fn package_use_expand_shorthand_drives_dependency_recursion
- fixtures/etc/portage/package.use has "dev-libs/packageuseexpandpkg PYTHON_TARGETS: python3_12" -- expands into "python_targets_python3_12".

## fn use_expand_star_wildcard_enables_every_matching_iuse_flag
- fixtures/etc/portage/package.use has "dev-libs/wildexpandpkg linguas_*" -- Portage config.py setcpv's own _* wildcard: enable every linguas_<x> in this package's OWN IUSE that isn't masked. profiles/base/package.use.mask keeps linguas_en off, so only linguas_de is enabled -> RDEPEND's "linguas_de? ( wildexpanddep )" fires and "linguas_en? ( wildexpandmasked )" does not. This package's OWN IUSE is "linguas_en linguas_de".

## fn use_expand_implicit_flag_is_valid_iuse_even_when_unlisted
- profiles/base/make.defaults: USE_EXPAND_IMPLICIT="ELIBC", USE_EXPAND_VALUES_ELIBC="glibc musl", ELIBC="glibc". dev-libs/implicitiusepkg RDEPENDs implicitiuseprov[elibc_glibc], and implicitiuseprov never lists elibc_glibc in its own IUSE. Exactly what it would be without this slice (elibc_glibc absent from declared IUSE -> `.required` fails -> no matching candidate).

## fn use_expand_implicit_flag_valid_but_not_enabled_still_fails_the_use_dep
- elibc_musl is valid implicit IUSE (in USE_EXPAND_VALUES_ELIBC) but not enabled (ELIBC="glibc"); implicitiusepkgmusl's own dep resolves to NoVisibleCandidate, exactly like any other unsatisfiable USE-dep.

## fn fixture_stable_use_force_and_package_use_stable_mask_apply_when_stable
- dev-libs/stableusepkg's own KEYWORDS="amd64" (no "~"); use.stable.force comes from profiles/base.

## fn fixture_stable_use_force_skips_an_unstable_candidate
- dev-libs/unstableusepkg's own KEYWORDS="~amd64"; its own stableforceflag?-gated RDEPEND is never pulled in.

## fn with_test_deps_does_not_apply_beyond_a_top_level_atom
- dev-libs/withtestdepconsumer RDEPENDs on dev-libs/withtestdeppkg.

## fn with_test_deps_is_a_no_op_for_a_package_with_no_test_iuse_flag_at_all
- Portage's own "pkg.iuse.is_valid_flag(\"test\")" guard.

## fn newuse_reinstall_still_recurses_into_its_own_dependencies
- dev-libs/reinstallpkg RDEPENDs on dev-libs/newpkg; not treated like the AlreadyInstalled dead-end it used to be before --newuse existed.

## fn fixture_package_use_enables_a_flag_not_on_globally
- Neither the profile nor make.conf enables "pkguseflag", but fixtures/etc/portage/package.use has a "*/packageuseenablepkg pkguseflag" wildcard entry, so newpkg's foo?-unrelated, pkguseflag?-gated dependency must be pulled in.

## fn nodeps_skips_recursion_even_when_package_use_would_otherwise_pull_something_in
- Same fixture as above. Dev-libs/newpkg must NOT appear.

## fn fixture_package_use_disables_a_flag_that_is_on_globally
- The fixture profile chain enables "foo" globally (see resolves_fixture_profile_chain_and_make_conf in portage-profile, and dev-libs/useflagpkg's own test above, which -- unlike this package -- DOES pull in its foo?-gated dependency). fixtures/etc/portage/package.use has a "dev-libs/packageusedisablepkg -foo" entry scoped to just this package.

## fn use_dep_enforcement_matches_a_declared_and_enabled_flag
- dev-libs/useflagpkg's own IUSE="foo missingflag", "foo" enabled globally by the fixture profile chain -- see resolve_pretend's own doc comment on use_deps_satisfied.

## fn use_dep_enforcement_rejects_a_dependency_atom_but_does_not_fail_the_graph
- Same "report, don't fail" precedent an unresolvable dependency atom already gets; see resolve_pretend_graph's own doc comment.

## fn required_use_satisfied_resolves_normally
- dev-libs/requireduseokpkg's own REQUIRED_USE is "foo? ( bar )" -- foo enabled globally, bar forced on by this package's own package.use entry.

## fn required_use_violated_top_level_fails_the_whole_call
- dev-libs/requiredusebadpkg's own REQUIRED_USE is "foo? ( bar )" -- foo enabled globally, bar never forced on. Portage depgraph.py's own REQUIRED_USE check happens right after package selection. Portage `_show_unsatisfied_dep`'s REQUIRED_USE block: no "complete expression" line.

## fn required_use_violated_dependency_still_fails_the_whole_call
- dev-libs/requiredusebadparentpkg RDEPENDs on dev-libs/requiredusebadpkg.

## fn required_use_violations_are_collected_across_the_whole_walk_not_just_the_first
- Portage depgraph.py's own _add_pkg sets _dynamic_config._required_use_unsatisfied = True and returns 0 on a violation. The second atom is dev-libs/requiredusebadpkg2's own "baz? ( qux )", unrelated to dev-libs/requiredusebadpkg's own "foo? ( bar )". Confirmed live against both portuale's own Rust and Python implementations (byte-identical joined output) before this test was written.
- Backlog #90 (S1): the walk resolves last-declared-first (Portage's stack discipline). Portage itself stops at the first violation, argv order -- staged oracle shows only badpkg.

## fn fixture_dependency_no_visible_candidate_gets_no_keyword_suggestion_by_default
- dev-libs/autounmaskdepconsumer RDEPENDs on dev-libs/autounmaskkeywordpkg (the same keyword-masked-only fixture the top-level test above uses). Previously always silent.

## fn fixture_dependency_no_visible_candidate_gets_a_keyword_suggestion_once_enabled
- With `--autounmask` explicit the keyword-masked dependency now RESOLVES (was `NoVisibleCandidate` + a `keyword_suggestion` before autounmask keyword *resolution* shipped).

## fn top_level_use_miss_reports_the_use_block_without_chain
- Backlog #265(b) (with #273): Portage `_show_unsatisfied_dep`'s "no ebuilds built with USE flags" block; Portage skips the chain for an `AtomArg` parent (`depgraph.py:7080-7090`). Live Portage 3.0.82.2 staged-fixture probe (`LLM/evidence/2026-09-30-265/real/useflagpkg-autounmask-use-n.txt`): `emerge --pretend --autounmask-use=n 'dev-libs/useflagpkg[-foo]'` prints the block where portuale printed the plain `no ebuilds to satisfy` miss.
- Portage's staging `for <root>.` suffix stays absent (the long-standing `fixture-miss-message-unsuffixed` class).

## fn fixture_dependency_no_visible_candidate_gets_no_use_suggestion_by_default
- dev-libs/usedeprejectedpkg RDEPENDs on "dev-libs/useflagpkg[-foo]" (the same fixture the top-level test above uses).

## fn required_use_referencing_an_implicit_arch_list_flag_is_valid
- dev-libs/archiuseimplicitpkg's own IUSE is empty and its own REQUIRED_USE is "!x86" -- exactly like Portage media-libs/mesa's own REQUIRED_USE referencing "x86" without declaring it in IUSE (PORTAGE_ARCHLIST lives at fixtures/repo/profiles/base/arch.list) -- confirmed live against the genuine, installed system. Before this fix, portuale's own iuse_set never consulted PORTAGE_ARCHLIST at all, so this would fail with "USE flag 'x86' is not in IUSE" instead of resolving (x86 isn't the active profile's own arch).

## fn aub0_backtrack_n_fails_or_stops_early_by_walk_order
- B walked before A: the loop stops on `need_config_change`.
- B first (walked first), then A, whose change broke it.

## fn backtracker_adopt_current_replaces_the_live_node
- Backlog #161 S2: the C2 dead-end accumulator hand-off.

## fn backtracker_get_best_run_prefers_the_deepest_terminal
- Backlog #161 S2: the most config progress with no masks. (The children differ in accumulator state, not just position: `add_node` dedups position-only progress via `params_equal`.)
- Both children are terminal at depth 3; the tie keeps the first, not the last.

## fn backtracker_feedback_slot_conflict_replaces_the_same_mask
- Backlog #161 S2: costing one budget unit.

## fn backtracker_feedback_missing_dep_latches_without_duplicating
- Backlog #161 S2: an already-masked parent is not pushed again; a mask step costs one budget unit.

## fn use_change_chain_starts_at_the_recorded_forcing_parent
- Backlog #205 R4. Portage `_get_dep_chain(pkg, unsatisfied_dependency=True)` (`lib/_emerge/depgraph.py:6257`); here that is `abk0d-1` (whose `C[x]` the flip satisfies, not `abk0a-2`'s plain `C`). The flip site records that owner in `AutounmaskChange::trigger`, and the fill starts the chain there -- asserting the recorded trigger itself, not just the final text, is what distinguishes this from the legacy first-requirer start.

## fn installed_instance_then_merge_records_an_unsolvable_slot_conflict
- Backlog #57 direction 2, #90 (S1). `needer`'s `>=paired-2.0` graphs 2.0 first and `othermod`'s `<paired-2.0` then collides with the installed 1.0 (staged oracle: 2.0 by needer, then 1.0).
- Portage's `_package_tracker` holds both nodes and reports the collision; portuale used to merge all three silently. The two atoms are jointly unsatisfiable, so the backtracker's mask trials all fail and `get_best_run` reports the original shape -- exactly Portage's `backtrack: 4/20` run, whose block is byte-identical to its own `--backtrack=0` one (`TEST/findings/l0-fixture-oracle.md` "#57 S0" cells b/c).
- The upgrade still merges, like Portage's merge list.

## fn merge_then_installed_instance_records_the_same_slot_conflict
- Backlog #57 direction 1, #90 (S1): last-declared-first, so `othermod`'s `<paired-2.0` meets the installed 1.0 (the walker's `AlreadyInstalled` early branch) before `needer`'s `>=paired-2.0` graphs 2.0 (staged oracle: 1.0, then 2.0 by needer).

## fn solvable_installed_instance_collision_records_nothing
- Backlog #57 K2: `plainuser`'s bare `dev-libs/paired` is satisfied by the installed 1.0 *and* by the 2.0 `needer` forces. Portage `_solve_non_slot_operator_slot_conflicts` reconciles it silently; portuale reaches the same place through `collect_feedback`'s existing `slot_want` solvability check (the merge-vs-merge reconciliation reused, not a second solver).

## fn ignore_built_slot_operator_deps_skips_the_rebuild_scan
- A test-local vdb (the shared fixtures keep only the slotbind repo ebuilds/md5-cache): slotbindtarget-1.0 at SLOT="2" plus a stale `:2/2=` consumer. Emerging the target picks slotbindtarget-2.0 (SLOT="2/9") from the shared repo.
- Complete mode: `slotbindconsumer` is in `@world`, so the slot-op scan considers it (see `required_set_reachable_cps`).

## fn ignore_built_slot_operator_deps_skips_the_rebuild_scan
- A test-local vdb (the shared fixtures keep only the slotbind repo ebuilds/md5-cache): slotbindtarget-1.0 at SLOT="2" plus a stale `:2/2=` consumer. Emerging the target picks slotbindtarget-2.0 (SLOT="2/9") from the shared repo.
- selective = false: a directly-named installed atom with a higher version still upgrades (matching Portage `emerge <atom>` without `--noreplace`).
- Backlog #24 S3: the rebuilt consumer's dependency edges (`slotbindtarget:2/2=` re-read from `metadata/md5-cache`). The pre-S3 synthesiser left `deps` empty, so this assertion is the slice.

## fn dependency_avoid_update_candidate_is_slot_aware
- avoidslotpkg-1.0 is installed at SLOT="2" (the repo ebuild says SLOT="1"); avoidslotconsumer RDEPENDs `dev-libs/avoidslotpkg:1`. Slot 1 is genuinely new. The `:1` dependency is a genuine graph entry, not silently AlreadyInstalled.

## fn graph_entry_carries_sub_slot_repo_and_oldbest_for_slot_repo_decoration
- A New from testrepo: sub_slot/repo populated, oldbest empty. A sub-slotted dep: sub_slot != slot is carried through.
- The vdb entry for newslotpkg-1.0 has no `repository` file -> `__unknown__`... but portuale's fixtures add one, so it's testrepo. Its slot ("0") differs from the resolved slot.

## fn package_provided_drops_a_matching_dependency_from_the_walk
- dev-libs/needsprovided RDEPENDs dev-libs/providedpkg (listed in profiles/default/package.provided) + dev-libs/newpkg; only needsprovided + newpkg show.

## fn package_provided_records_a_matching_top_level_target_for_the_warning
- Portage depgraph.py's own _pprovided_args, which pretend.rs turns into the `WARNING: … package.provided:` block.

## fn buildpkgonly_flags_a_new_package_depending_on_another_new_package
- dev-libs/dualdep (New) has both DEPEND and RDEPEND on dev-libs/newpkg (also New) -- Portage --buildpkgonly can't resolve this, since newpkg itself would also need building.

## fn buildpkgonly_does_not_fire_when_the_dependency_is_already_installed
- dev-libs/buildpkgonlysatisfied (New) RDEPENDs on dev-libs/samepkg.

## fn buildpkgonly_is_always_false_when_not_requested
- `graph_result_real` always passes buildpkgonly=false.

## fn fixture_strong_blocker_matches_an_installed_package
- dev-libs/blockerpkg's RDEPEND is "!!dev-libs/samepkg", and dev-libs/samepkg-1.0 is already installed per the fixture vdb. Backlog #68 S3 (container oracle, 2026-09-16). (Pre-S3 this expected `unsolvable: true` from the vdb reverse scan.)
- Backlog #72 B3: resolved by removing the installed match; display of it is B4.

## fn blocker_retry_seeds_make_an_uninstall_resolved_block_unsolvable
- Backlog #68/#72 B2c, B0 cell u4: Portage re-runs `_serialize_tasks` in complete mode when the first pass selected an uninstall task (`depgraph.py:10365-10383`), even though the resolve itself is not complete.
- `dev-libs/blockerpkg` (`!!dev-libs/samepkg`) with installed `samepkg-1.0`; with installed `dev-libs/changeddepspkg` (vdb `RDEPEND=dev-libs/samepkg`) as the world seed.

## fn fixture_weak_blocker_matches_another_new_package_in_the_same_graph
- dev-libs/graphblockerparent pulls in both dev-libs/blockerpartnerpkg and dev-libs/weakblockerpkg (whose RDEPEND is "!dev-libs/blockerpartnerpkg") as New in the same run.
- Backlog #68 S2 (container oracle, 2026-09-16): Portage prints `[blocks B]` for this merge-vs-merge shape.

## fn fixture_unrelated_packages_report_no_blockers
- Regression guard: the diamond fixture has no blockers at all, so none of its entries should gain a spurious one.

## fn exact_version_update_keeps_arg_pinned_provider
- Backlog #285. Same installed/tree shape as the #268 pin (`mmprov-1` `0/1` + `mmprov-2` `1/1` + `mmcons-1` bound `mmprov:0/1=`, tree `mmprov-1/-2/-3` with `mmprov-3` at `1/2`), but the tree here is the shared fixture repo (resolved through `config_root`) while the vdb is ad-hoc under `root` -- the `_b1_root` split.
- Full `--update --deep` walks, Portage 3.0.82.2 bytes from the R3 S0 capture (host staged-fixture probe 2026-10-02, `--ignore-default-opts --pretend --color=n --backtrack=3 --update --deep <atoms>`, rc 0 throughout):
- `=app-misc/mmprov-2 app-misc/mmcons` -> `[ebuild rR] app-misc/mmcons-1` alone (`backtrack: 1/3`): the argument pins installed `mmprov-2` in the graph and the rebuilt consumer's `app-misc/mmprov:=` reuses it (Portage `_wrapped_select_pkg_highest_available_imp`'s `find_existing_node` pass) instead of upgrading to `mmprov-3`.
- `~app-misc/mmprov-2 app-misc/mmcons` -> the same single row.
- Control `app-misc/mmprov:1/2 app-misc/mmcons` -> `[ebuild U] app-misc/mmprov-3 [2]` + `[ebuild rR] app-misc/mmcons-1`: the slot atom's own pick is the upgrade, so the dep binds that.

## fn fixture_use_conditional_license_is_visible_when_the_flag_is_off
- No comment kept; resolves New with the flag off (contrast the forced-on leg below).

## fn fixture_use_conditional_license_is_masked_once_package_use_forces_the_flag_on
- fixtures/etc/portage/package.use forces "nonfreeflag" on for this specific package, activating its own LICENSE's "nonfreeflag? ( SomeEula )" conditional.

## fn recursion_keeps_an_installed_dependency_whose_use_dep_flag_is_only_in_its_built_use (bug ref)
- Portage `dbapi._iuse_implicit_built` is upstream bug #640318.
