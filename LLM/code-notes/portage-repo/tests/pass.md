# crates/portage-repo/src/tests/pass.rs
Single-walk (`run_pass`) unit tests: binary instances, blockers, masks, and report channels.

## fn multi_instance_walk_reads_the_selected_instances_own_deps
- B1 bug 1: the dependency strings of a chosen binary instance were read from the first `Packages` entry with the same `CPV`, not from the instance that was actually selected. Here build 2 (newest) wins `dedup_binary_instances`, but the walk followed build 1's `RDEPEND` (`dev-libs/olddep`) -- in the field that is `ghostscript-gpl:0/10.06=`, which forces a downgrade of the installed 10.08.
- Expected value: Portage's `_iter_match_pkgs` + `pkg._metadata` of the selected instance (the `Packages` entry carrying that `BUILD_ID`).

## fn run_pass_reinstall_with_only_repo_drift_carries_oldbest
- Backlog #247: a same-version/slot/sub-slot `Reinstall` whose installed instance's repo differs from the merged candidate's carries that instance as `oldbest` -- Portage `_get_installed_best`'s third disjunct (`not quiet_repo_display and repo differs`, `output.py:721-731`; owner B5 reopened the cut).
- No `repository` file: `installed_pkg_repo` falls back to `__unknown__`, which never equals the `testrepo` candidate. The staged vdb omits the `repository` file (like the hand-staged abk0/samepkg fixture vdbs), so the installed repo is Portage's `__unknown__` sentinel while the candidate merges from `testrepo`.

## fn run_pass_records_the_needer_othermod_slot_conflict
- Backlog #161 S6.

## fn run_pass_files_a_depend_blocker_row
- Backlog #161 S6.
- victim-1.0 stays installed while the parent merges around it under a soft block; the installed victim gains the uninstall-model row beside its installed one.

## fn run_pass_nodeps_skips_the_unwalked_blocker_scan
- Backlog #161 S6: an installed owner's blocker stays invisible even when its target upgrades right beside it.
- Unwalked installed owner blocking `scanvictim`, which this run upgrades 1.0 -> 2.0 (so a scan would find a merge-bound match and file rows).
- The owner rides the required-set closure so a found blocker has a display home (the #73 set-parent arm).
- Unversioned blocker: an op-less versioned blocker is ambiguous and rejected on both sides (Portage `Atom('!dev-libs/scanvictim-1.0')` raises too).

## fn run_pass_sorts_provided_atoms_in_argument_order
- Backlog #161 S6.

## fn run_pass_reports_a_masked_candidate
- Backlog #161 S6: a masked candidate reports through the masked-dep channel instead of resolving.

## fn run_pass_fails_a_required_use_violation
- Backlog #161 S6: its own ebuild documents the trigger: `foo` forced on via `package.use`, `bar` left off.

## fn run_pass_with_test_deps_walks_test_conditionals
- Backlog #161 S6: without the flag the `test?` deps stay out.

## fn run_pass_arms_the_missing_dep_trigger
- Backlog #161 S6: a dependency nothing satisfies -- not even its bare form. A USE-only failure instead stays out of this path -- its bare form still satisfies the probe -- so the shape uses a fully missing package.

## fn run_pass_flags_nvc_under_masks_suppressed
- Backlog #161 S6: the #57 surely-masked shape -- the second visit dedups, and the mask gate marks the pass a dead end; without masks the dep merges and nothing suppresses.

## fn run_pass_reports_a_masked_dependency_once
- Backlog #161 S6.

## fn run_pass_complete_mode_locks_unlisted_dependencies_out
- Backlog #161 S6: the cheap graph-or-installed check instead of a fresh resolve.

## fn run_pass_deep_walk_recurses_into_installed_deps
- Backlog #161 S6: without an unlimited deep walk the installed deps stay out.

## fn run_pass_usepkg_selects_the_staged_binary
- Backlog #161 S6: carrying the index's build identity.

## fn run_pass_binary_hides_buildtime_deps_without_bdeps
- Backlog #161 S6.
- The blanking twin: the recorded merge-order edges skip the build-time dep too.

## fn run_pass_dedups_plain_miss_reports
- Backlog #161 S6.

## fn run_pass_records_skipped_updates_for_solved_conflicts
- Backlog #161 S6: records its skipped update for the withhold notice; `oldconsumer` collides the pulled 2.0 with 1.0.

## fn run_pass_arms_the_trigger_for_an_upgrading_parent
- Backlog #161 S6: the `Upgrade` arm, not just `New`.

## fn run_pass_drops_an_unresolvable_set_member
- Backlog #286: a top-level set member (`@world`/`@selected` expansion, Portage `SetArg` "world"/"selected") that selects no package drops into `missing_args` instead of failing the walk (Portage `_missing_args`, `depgraph.py:5577-5605`); the same atom requested explicitly (no set membership) fails with the bare miss. The drop leaves no entry, so the healed rest of the graph can settle around it.

## fn run_pass_reports_each_plain_miss_once
- Backlog #161 S6: four missing deps -- two sharing dev-libs, two sharing the package name across categories.
- Backtrack disabled: no missing-dep retry arms either.

## fn run_pass_repeated_miss_without_masks_is_not_suppressed
- Backlog #161 S6: suppression needs the mask gate.

## fn run_pass_reports_each_masked_dep_once
- Backlog #161 S6: four masked deps across categories sharing a package name.

## fn run_pass_reports_each_use_unsat_once
- Backlog #161 S6: four unsatisfiable deps across categories sharing a package name.

## fn run_pass_files_one_row_per_blocker_owner
- Backlog #161 S6: the pending dedup keys on the full triple, so a second owner is never swallowed.

## fn run_pass_binary_walks_buildtime_deps_with_bdeps
- Backlog #161 S6.
- NOTE (repaired S6p): the first version of this leg built the tree parent with `DEPEND="dev-libs/buildonlydep"` (the only key the helper could write) while staging the binary with `BDEPEND="dev-libs/buildonlydep"`. Under `--with-bdeps` `--binpkg-changed-deps` compares all five keys, saw the `DEPEND` mismatch, and rejected the binary -- so the leg walked the ebuild and proved nothing about the `Binary && !with_bdeps` gates. The parent now carries the dep in `BDEPEND` on both sides, so the binary survives selection and the walk genuinely takes the all-keys arm.
- The parent's recorded edges name the build-time dep; the queue tags that edge build-time-hard (no installed provider, no run-time alternative), which is what the `buildtime_atoms` gate computes.

## fn run_pass_buildpkgonly_walks_buildtime_keys_only
- Backlog #161 S6p: the `dep_keys` second arm (`candidate_source != Binary`). The scratch parent carries the same dep in `DEPEND` (walked) and a second one in `RDEPEND` (blanked).
- The kept build-time dep reaches the merge-order edges; the queue tags the kept dep build-time-hard (no installed provider, no run-time alternative).

## fn run_pass_plain_miss_ignores_other_cps_reports
- Backlog #161 S6: visits run reverse-RDEPEND-text, so the plain filings land last with every sibling vec populated.

## fn run_pass_respect_use_filters_a_stale_binary
- Backlog #161 S6: the staged binary was built with +soflag; the tree resolves it off.

## fn run_pass_newer_ebuild_beats_older_binary
- Backlog #161 S6.

## fn run_pass_bare_slotop_pull_reuses_the_resolved_slot
- Backlog #161 S6: the satisfaction holds through the operator and binding arms when nothing rebuilt shifts.

## fn run_pass_replace_seed_files_no_argument_puller
- Backlog #161 S6: seeded alone, so no top-level visit dedups it away first.

## fn run_pass_trigger_matches_the_owner_entry_exactly
- Backlog #161 S6: a same-category neighbour walking first never supplies the owner; a neighbour resolving elsewhere walks first, so a loosened owner lookup would build the wrong neg.

## fn run_pass_usepkgonly_takes_a_newer_binary
- Backlog #161 S6.

## fn run_pass_newer_binary_loses_to_the_tree
- Backlog #161 S6.

## fn run_pass_masked_ebuild_is_no_twin
- Backlog #161 S6: the accepted staged binary still merges under `--usepkgonly`. Tree ebuild ~amd64 (masked under the amd64 accept); staged binary amd64 (visible).

## fn run_pass_package_use_mask_hides_test_deps
- Backlog #161 S6.
