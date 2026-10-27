# crates/portage-repo/src/resolve/feedback/reverse_dep_pins.rs
Reverse-dependency pins: an upgrade breaking an installed consumer's recorded dep is pinned or dropped.

## fn step_reverse_dep_pins
- Portage `_complete_graph`'s installed-universe walk reaching `_slot_operator_check_reverse_dependencies`: an upgrade this pass picked breaks a dependency an *installed* package -- one nowhere near the requested atom's own tree -- already records against it. Portage sees such an atom because complete mode (auto-enabled by any installed-package version/USE change) deep-walks `@world`/`@selected`/`@system` and pulls every *reachable* installed package into the graph as a nomerge node (#54: not the whole installed universe -- see `reverse_dependency_constraints`'s own notes), contributing its recorded atoms to `_parent_atoms`; portuale finds them with a direct vdb reverse scan instead and feeds them back here as positive enforcement atoms (C1: they stay in the positives bucket -- they are not masks), which is precisely what parent atoms are. The re-resolve then picks the highest candidate satisfying them -- normally the installed version, so the entry settles as `AlreadyInstalled` -- and drops whatever the rejected version had dragged into the graph. `reverse_dep_masked` latches every constraint already added, so a pass that finds nothing new falls through and the loop terminates.
- A pin jointly unsatisfiable with this pass's hard requirements (an explicit versioned request, or a dependency only a newer version satisfies) is dropped instead of enforced: Portage merges the hard-required version anyway and reports the broken consumer against the installed instance. Dropped pins accumulate in `dropped_pins` for that residual report (see `build_residual_slot_conflicts`); they never enter `slot_constraints`, so the hard-required version is never masked into invisibility.
- #24 S4: rule 4 of `_eliminate_rebuilds` checks every parent atom of the rebuilt pkg, and Portage's complete-graph nomerge consumers are part of `_parent_atoms`; these are the pins carrying their atoms. Cloned before the loops consume the vectors.
- Backlog #90 (S2) + #92: retain every enforced pin (deduped like `dropped_pins`) so `assemble_result` can report the upgrades they withhold -- no conflict ever forms on that path, so without this the withhold would stay silent.
- Backlog #205: Portage's "Record missed updates" tail (`lib/_emerge/depgraph.py:2085-2106`) -- a version the re-resolve drops for the enforced pin is reported through `_show_missed_update_slot_conflicts`. Record the rejection as a slot-conflict-style mask on the chosen version (parents = the enforcing consumer), so the existing `backtrack_missed_updates` derivation renders the `WARNING` row. The positive pin above still drives selection; the negative excludes nothing the pin does not already exclude. Gated on a live search like every mask (Portage `_allow_backtracking`); budget-free like the RevDep feedback it rides.
- C1: enforced pins are positive enforcement atoms (the consumer's recorded atom must keep matching), not masks -- they stay in the positives bucket, exactly like the solvable-conflict wants above.
- Backlog #568: the enforced-pin "Record missed updates" mask carries the
  pin's `built_render` atoms as extra parents ahead of the pin's own (the consumer-pin record uses the same order, see `file_consumer_pin_conflicts`), so
  the skip block prints the built `:S/SS=` form first, like Portage's
  missed-update parents. Constraint path untouched (`slot_constraints`
  still takes `pin.atom` only).
- Backlog #209: a distinct variant (see its own notes) so the driver can feed these pins back in-pass under `--backtrack=0`, where every other feedback kind still settles.

## fn live_reachable_consumers
Backlog #490. Portage's `_complete_graph` pulls in an installed package only through a graph node's dependencies, and an installed package this run replaces is not a node. The pre-merge closure (`ResolveCtx::slot_op_reachable`) still walks through such packages, so an installed leaf that only a replaced parent needs (karchive's pin from `kdoctools-6.30.0` via `kded-6.30.0`) kept pinning the provider's old version; the dropped pin survived across passes and printed as a slot conflict at the end. The live closure re-closes the same seeds with `InstalledReachability::reachable_without`, skipping the cps the pass merges. The one per-pass scan (`scan_reverse_dependencies`) uses it. The `ResolveCtx` stub in the unit tests passes `slot_op_seeds` derived from its reachable set.

## struct ReverseScan / fn scan_reverse_dependencies
- One reverse-dependency scan per pass, run at the top of `collect_feedback_inner` and handed to the two consumers: the early dropped-pin collection (a pass with slot conflicts returns before the pin step, so its dropped pins would otherwise never reach the next pass) and `step_reverse_dep_pins`. Before the consumer-pin route there were two scans on a settling conflict pass.

## fn file_consumer_pin_conflicts
- Replaces `dropped_pin_mask_restart` and its gates G0 and G1 (the mask-and-restart over a dropped pin). Portage backtracks over a consumer atom the merge contradicts with `_slot_confict_backtrack` (`depgraph.py:2220-2304`): the conflict atoms are the parent atoms of every slot node, the installed consumer's included (D:2160-2165, D:2238-2245); one choice per package, highest version first (D:2235), then a stable ascending sort by conflict-atom count (D:2273); one backtrack node per choice, the last popped first (B:136, B:185-195). So a contradicted pin is now a residual slot-conflict record fed through the ordinary conflict steps.
- The record is the one `build_residual_slot_conflicts` builds: merge instance and the installed instance, taken from the raw vdb whatever its backtracking mask (Portage re-adds it in `_complete_graph` without a mask check, D:8803-8834 -- that is what makes the same conflict form again after a "mask the installed instance" choice, and Portage's 4/20 on the T2/T3 shapes). The installed instance's parents are the consumer's selected atoms plus its built `:S/SS=` atoms verbatim, sub-slot kept (D:2189-2197); the pin's `built_render` supplies them.
- Appended after the walker's own conflicts (`file_after_walker_conflicts`); `step_slot_conflict_backtrack` takes `slot_conflicts[0]`, Portage feeds back only the first conflict (B:180-185). A walker record of the same slot and instances keeps its place and takes the installed parents. The chain runs the ABI step once more when a record was filed (D:2208-2214).
- Not filed in a pass with a missing, masked, USE-unsatisfied or plain-miss dependency, with a pending missing-dependency trigger, with a fresh autounmask change, or with a newly enforced pin (D:5691-5696, D:12407-12411; Portage solves the solvable conflicts first, D:2131). That is G0's predicate moved to this site; G1 is gone, since an argument-only want is now a conflict atom like any other.
- `or_branch_superseded` (G2) stays as a filter on the pins that form the record, until the consumer's `||` is chosen the way `dep_zapdeps` chooses it. `dropped_pins` carries pins over from earlier passes, so the filter also keeps a stale alternative from an earlier pass out of the record.
- `dropped_pins` and `reverse_dep_pins` do not grow in a loop pass: the pin is already there, and `params_equal` compares both, so a grown copy would make the duplicate node a new one. The record's parents are sorted for the same reason.
- Counts, Portage 3.0.83 against the printed count (`backtrack: N/20`): the search makes Portage's tries on every cell of the plan's count list, but the driver's exit count (`passes - 1`) stays one short when the last pass is a failed walk with nothing left to explore. See `driver.md`, "exit count".

## fn or_branch_vdb_deselected_set
- Backlog #569 (C1 portuale-only half): the pin scan fanned every `||`
  branch out into its own pin, so non-selected arms (`=5.38*`/`=5.40*`,
  15 portuale-only lines on the bed XML-Parser probe) printed as conflict
  parents while Portage prints only the vdb-selected arm. Portage
  `dep_zapdeps` (`dep_check.py:339`) resolves each `||` group once,
  installed-preferred, and only the selected arm ever becomes a parent
  atom. This set holds the demoted arm texts (verbatim plus
  constraint-normalised, so the flat token and the stripped pin text both
  compare equal); the scan skips them before any constraint or rendering
  logic runs. Empty (today's behaviour) when the depstring holds no `||`,
  fails to parse, or selects nothing — a group no arm satisfies keeps all
  its arms, the conservative direction.
- Selection predicate (`or_member_matches_installed`): a plain atom
  matching any installed instance of its own package (full
  `cp-ver:slot/sub::repo` candidate strings from `installed_refs`),
  a conjunction only when every leg matches, a nested `||` when any leg
  does; blockers and unparseable text never match. Demotion needs a
  *sibling* that matches — one satisfied arm keeps its pin (`=5.42*`
  still withholds), the rest go. Same-package menus and cross-package
  alternatives share the rule (the `~perl-core` arm demotes the same way);
  G2's merge-graph predicate is untouched and still guards the restart
  site. Walks the shared `reduced_dep_tree` (same reduction the scan's
  flattening uses).
- Non-goals: no dep_zapdeps preference-bin ordering beyond
  installed-match (first-listed vs best-ranked among several satisfied
  arms is not modelled — every satisfied arm keeps its pin). Demotion
  runs before any pin exists, so the abort path
  (`build_residual_slot_conflicts`) loses the demoted arms too — matching
  Portage, which never records them as parent atoms anywhere. The `||`
  groups come from the shared `reduced_dep_tree` (same reduction the
  scan's flattening uses); only the `AnyOf` walk is local.

## fn or_member_matches_installed
- See `or_branch_vdb_deselected_set`: the vdb-satisfaction test with the
  `All`/`Any` recursion spelt out.

## fn or_member_flat_atoms
- See `or_branch_vdb_deselected_set`: flattens a demoted member
  (through conjunctions) to the atom texts the scan would otherwise pin.

## gate threading (#571)
- The `ReverseDependencyConstraintsArgs` site uses `ctx.gate()`.
