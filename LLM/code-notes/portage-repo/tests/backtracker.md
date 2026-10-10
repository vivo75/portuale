# crates/portage-repo/src/tests/backtracker.rs
Backtracker unit tests: `--backtrack=0` shape, mask predicate, circular retry.

## fn fixture_backtrack_max_zero_disables_slot_conflict_reconciliation
- Backlog #90 (S1), expected value from Portage: with `backtrack_max == 0` (Portage `--backtrack=0`) no conflict forms at all (the old consumer graphs 1.0 first, the bare dep reuses it), so bt0 settles silently on 1.0 with rc 0, exactly Portage's S0 matrix row.
- The pre-S1 expectation (one recorded conflict) is retired: that conflict was the forward-order phantom.

## fn check_runtime_pkg_mask_discards_fully_masked_parent_sets
- Portage `Backtracker._check_runtime_pkg_mask` (bug 375573); C3 oracle for the discard predicate itself (no fixture can reach it end to end yet; needs a 3-pass parent-masking chain, see `docs/history/023-oracle.md`).
- Masks with no parents are Portage's 692746 arm; missing-dependency entries are skipped entirely.
- `c-1.0`'s parent `d` is unmasked, but `a`'s parent `b-2.0` is still fully masked -- still discarded; unmasking `b-2.0` by dropping its entry frees `a`'s parent.

## fn virtual_or_choice_is_demoted_through_its_pullers_circular_record
- #249: Portage `dep_zapdeps` chains `circular_dependency.get(parent)` and `.get(virt_parent)` (`portage/dep/dep_check.py:673-678`); a virtual's `||` is expanded inline under the package that pulled it, so the puller's circular record demotes the cycle-closing branch.
- Fixture `dev-util/u249make` (bug 703440's cycle with the `||` inside `virtual/u249make`); expected value from Portage: Portage's ResolverPlayground merges the bootstrap after one circular restart and aborts under `--backtrack=0` (portuale `LLM/evidence/2026-09-30-249/u249.txt`).

## fn circular_choices_backtrack_to_the_cycle_breaking_branch
- Backlog #221: circular `||` backtracking (upstream `test_circular_choices.py`, bugs 703440 and 705986). Each cell's first pass takes the in-graph `||` branch and strands the merge-order walk; the restart demotes the cycle-closing branch (`circular_atom`).
- Cells: pg1 `[bootstrap, jsoncpp, cmake]`; pg4 `[exe-bin, pypy]` with no `+low-memory` rescue; pg4 requesting the exe itself merges `[exe-bin, pypy, exe]` after its buildtime dep; pg5 (`testDirectVirtualCircularDependency`) backtracks the runtime-closing ring to `-bin` first.
- Expected value from Portage, probed against 3rdparty/portage 3.0.82.2's ResolverPlayground, which fails all three cells with `{"--backtrack": 0}`.

## fn circular_slot_pinned_branch_prefers_bin_over_older_slot
- g221c (L0 `dev-lang/rust` regression), expected value from Portage: a slot-pinned `||` build dep over an older slot of the package itself vs the `-bin` package (the older slot carrying its own self buildtime edge). Grounded in 3rdparty/portage 3.0.82.2 ResolverPlayground (`/tmp/opencode/g221c/probe_slcirc.py`): backtracking on merges `[slcirc-bin-2.0, slcirc-2.0]` after `backtracking try 1`; `--backtrack=0` fails circular.
- Bug story: the g221b reconciliation broke this; the slot-blind in-graph check ranked the older-slot branch `Installed`, beating `-bin` and re-closing the cycle through the older slot's self edge (rc 1, `slcirc-1.0 depends on slcirc-1.0`).

## fn softened_ring_reports_the_persisting_cycle_with_backtracking_on
- Backlog #221 (I3): the unsolved-cycle-with-backtracking-ON leg of `assemble_result`'s persisting-ring report. `dev-libs/sbrA` BDEPENDs+RDEPENDs on `dev-libs/sbrB` (the dual edge softens the hard reporter's arm to `(true, true)`) while `sbrB` BDEPENDs back, so the hard reporter stays empty but the walk strands.
- Grounded in Portage runs (`/tmp/opencode/g221b/probe_sbr.py`, 3rdparty portage 3.0.82.2 ResolverPlayground, debug `backtracking try` count): backtracking on fails after exactly 1 retry, `--backtrack=0` fails with none.

## fn backtrack_params_equal_compares_every_accumulator
- Backlog #161 S2: `params_equal` is Portage's parameter-only `__eq__` (`backtrack_config`, `depth` and `mask_steps` are search position, not state).
- Backlog #129 (S1): first-seen mask order is render order, not search state; same masks in any arrival order dedup.

## fn backtrack_missed_updates_stays_silent_without_a_clean_settle
- Backlog #129 (S1), expected value from Portage: at `--backtrack=0` the mg2top root pass keeps its slot conflicts (highest-first picks the `-2`s), so no mask trial state exists and neither notice may fire.
