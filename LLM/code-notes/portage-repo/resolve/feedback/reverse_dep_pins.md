# crates/portage-repo/src/resolve/feedback/reverse_dep_pins.rs
Reverse-dependency pins: an upgrade breaking an installed consumer's recorded dep is pinned or dropped.

## fn step_reverse_dep_pins
- Portage `_complete_graph`'s installed-universe walk reaching `_slot_operator_check_reverse_dependencies`: an upgrade this pass picked breaks a dependency an *installed* package -- one nowhere near the requested atom's own tree -- already records against it. Portage sees such an atom because complete mode (auto-enabled by any installed-package version/USE change) deep-walks `@world`/`@selected`/`@system` and pulls every *reachable* installed package into the graph as a nomerge node (#54: not the whole installed universe -- see `reverse_dependency_constraints`'s own notes), contributing its recorded atoms to `_parent_atoms`; portuale finds them with a direct vdb reverse scan instead and feeds them back here as positive enforcement atoms (C1: they stay in the positives bucket -- they are not masks), which is precisely what parent atoms are. The re-resolve then picks the highest candidate satisfying them -- normally the installed version, so the entry settles as `AlreadyInstalled` -- and drops whatever the rejected version had dragged into the graph. `reverse_dep_masked` latches every constraint already added, so a pass that finds nothing new falls through and the loop terminates.
- A pin jointly unsatisfiable with this pass's hard requirements (an explicit versioned request, or a dependency only a newer version satisfies) is dropped instead of enforced: Portage merges the hard-required version anyway and reports the broken consumer against the installed instance. Dropped pins accumulate in `dropped_pins` for that residual report (see `build_residual_slot_conflicts`); they never enter `slot_constraints`, so the hard-required version is never masked into invisibility.
- #24 S4: rule 4 of `_eliminate_rebuilds` checks every parent atom of the rebuilt pkg, and Portage's complete-graph nomerge consumers are part of `_parent_atoms`; these are the pins carrying their atoms. Cloned before the loops consume the vectors.
- Backlog #90 (S2) + #92: retain every enforced pin (deduped like `dropped_pins`) so `assemble_result` can report the upgrades they withhold -- no conflict ever forms on that path, so without this the withhold would stay silent.
- Backlog #205: Portage's "Record missed updates" tail (`lib/_emerge/depgraph.py:2085-2106`) -- a version the re-resolve drops for the enforced pin is reported through `_show_missed_update_slot_conflicts`. Record the rejection as a slot-conflict-style mask on the chosen version (parents = the enforcing consumer), so the existing `backtrack_missed_updates` derivation renders the `WARNING` row. The positive pin above still drives selection; the negative excludes nothing the pin does not already exclude. Gated on a live search like every mask (Portage `_allow_backtracking`); budget-free like the RevDep feedback it rides.
- C1: enforced pins are positive enforcement atoms (the consumer's recorded atom must keep matching), not masks -- they stay in the positives bucket, exactly like the solvable-conflict wants above.
- Backlog #209: a distinct variant (see its own notes) so the driver can feed these pins back in-pass under `--backtrack=0`, where every other feedback kind still settles.

## fn live_reachable_consumers
Backlog #490. Portage's `_complete_graph` pulls in an installed package only through a graph node's dependencies, and an installed package this run replaces is not a node. The pre-merge closure (`ResolveCtx::slot_op_reachable`) still walks through such packages, so an installed leaf that only a replaced parent needs (karchive's pin from `kdoctools-6.30.0` via `kded-6.30.0`) kept pinning the provider's old version; the dropped pin survived across passes and printed as a slot conflict at the end. The live closure re-closes the same seeds with `InstalledReachability::reachable_without`, skipping the cps the pass merges. Both pin scans (`step_reverse_dep_pins` and the slot-conflict feedback site in `feedback/mod.rs`) use it. The `ResolveCtx` stub in the unit tests passes `slot_op_seeds` derived from its reachable set.

## fn dropped_pin_mask_restart
- Backlog #563 (perl-5.44 heal): a dropped pin the settled merge contradicts aborts today as a residual slot conflict (`build_residual_slot_conflicts` in assembly), while Portage backtracks over it (L0 `logs/l0-20261025T084209Z`, `dev-perl_XML-Parser`; S0 oracle `LLM/evidence/2026-10-25-563/captures/portage-pull-bare.*`: rc 0, backtrack 2/20, old puller merged, skip-block + `triggered by backtracking` notice). With backtracking live and no enforced restart pending, the first residual record whose merge version is not yet masked becomes one `BacktrackFeedback::MissingDep` mask so the retry re-picks around it. Portage: `depgraph.py`, `_slot_confict_backtrack` (mask over the conflict) + `_get_missed_updates` (the skip rendering).
- The entry carries `MaskReason::SlotConflict` (parents = the contradicting dropped pins), NOT `MissingDependency`: only the slot-conflict reason derives the `WARNING` skip-block row via `backtrack_missed_updates`; a missing-dependency reason would derive the abbreviated/full `triggered by backtracking` row instead. Same reason shape as the enforced-pin "Record missed updates" tail above. The `MissingDep` *variant* still applies it (the variant is reason-agnostic; the driver files the entry and latches the neg).
- Gate is the assembly report itself (`build_residual_slot_conflicts` over the pass): only a pin combination the settle path would abort on restarts, so clean resolves never mask. Parents must contradict the merge candidate (stale matches skipped) and their consumers must not be merge-bound (mirrors the builder's filter); no contradicting pin means no mask. One record per pass; the mask presence (runtime mask or `missing_dep_masked`) latches, so the search terminates even when the retry re-forms the record.
- p563 flow: pass 1 masks `!=dev-libs/p563prov-5.44` here; pass 2's `=dev-libs/p563prov-5.44*` dep then misses and the existing `maybe_missing_dep_backtrack` masks `!=dev-libs/p563pull-2`; pass 3 settles with `p563pull-1` + kept 5.42, deriving the skip-block and notice rows. A pinned-newver request (`=dev-libs/p563pull-2`) retries the same way, then fails its top-level select and falls back to the residual abort (see the driver fallback note).
- Known divergence (D1): the skip-block prints one conflict line (`=dev-libs/p563prov-5.42*`) where Portage prints two (also `dev-libs/p563prov:0/5.42=`). Root cause is pre-existing: `reverse_dep_constraint_atom` strips built `:S/SS=` atoms to the bare cp, so the built atom never pins on upgrade and never reaches the mask parents. Fixing the strip is global withhold/reinstall behavior, out of #563 scope.
- Fix round 1 (over-firing): the ungated arm fired wherever a residual
  would abort, but Portage only heals the puller shape. Four contract
  regressions (rc 1→0): #54 K3 keeper (`=paired-2.0` vs world keeper),
  #222 cgp ver-break, #223 igw contrast, #35 R4 or-choice (bug 531656,
  update wrongly skipped). Three gates, all default-deny:
- G0 (doomed passes): fresh autounmask USE activity this pass
  (`pass.autounmask_use_changes`/`use_overlay` -- accumulated config from
  earlier passes must NOT block a later heal) or a hard masked/USE/plain
  miss disclosure means Portage reports that abort before any backtrack
  heal (backtrack 0/20 on the C3 libreoffice/gimp/plasma-meta/gnome-shell
  probes); a mask excursion there only loses the abort's own rows.
- G1 (third-party puller): the contradicted merge version must be
  hard-wanted by a merge-bound non-argument puller
  (`has_merge_bound_puller`: non-empty `parent_cpv`, `split_cpv` parses,
  same-version merge-bound entry exists). A bare explicit-arg want
  (keeper/igw/cgp: only `(Argument)` parents) cannot be re-picked --
  masking strands the argument and Portage aborts loudly instead.
- G2 (non-selected `||` branch): a contradicting pin from a `||` branch the
  merge satisfies elsewhere is a stale alternative (Portage demotes it via
  dep_zapdeps, never backtrack-skips). `or_branch_superseded` re-reads the
  consumer depstring with the scan's own layering, parses `||` groups with
  a USE-aware recursive descent (`DepItem`, dead conditional branches
  vanish), and excludes the pin when a sibling alternative targeting
  ANOTHER package matches a merge-bound entry. Same-package version menus
  never count (bed virtuals re-pick `=5.42*` through exactly this path, so
  the 9 bed heals keep firing).
