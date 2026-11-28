# crates/portage-repo/src/tests/tests_first_unsatisfied_dep.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): A USE-only miss Portage meets first stops the walk with no restart, and a pass that recorded an autounmask change before its missing-dependency stop ends the search (#594). Expected values are host Portage 3.0.83 captures in `LLM/evidence/2026-10-26-575-residues/r3/captures/`, cited per test.

## fn backtrack_0_reports_only_the_first_failure_and_no_later_conflict
- Backlog numbers removed from the comment (original text): Backlog #609, Portage 3.0.83 `emerge -pv --backtrack=0 dev-libs/r592atop` (`LLM/evidence/2026-11-06-sweep/609/`): rc 1, `(backtrack: 0/0)`, only the `r592agone` miss under `r592ay-2`. The LIFO walk pops `ay-2` first and stops, so `ax-2` never pulls `as-1`.

## fn a_use_stop_drops_the_later_masked_dependency_and_its_restart
- Backlog numbers removed from the comment (original text): Backlog #609, Portage 3.0.83 `emerge -pv --autounmask=n dev-libs/r594qtop`: rc 1, `(backtrack: 0/20)`, only the `r594vdep` USE block. `r594qa` (listed last) is popped first and stops the walk, so `maskeddep` under `r594qb` is neither reported nor backtracked for.

## fn the_first_masked_dependency_wins_over_a_later_one
- Backlog numbers removed from the comment (original text): Backlog #609, Portage 3.0.83 `emerge -pv [--backtrack=0] dev-libs/r594htop`: rc 1, `(backtrack: 0/20)` and `0/0`, only the `r594xdep[r594flag]` all-masked block under `r594ha-2`.

## fn a_failing_or_branch_keeps_its_missing_dependency_restart
- Backlog numbers removed from the comment (original text): Portage 3.0.83 `emerge -p --update --deep dev-libs/opartly` and `--emptytree`: rc 1, `(backtrack: 1/20)`. The failing atoms sit in a `||` group, which Portage picks in `_pop_disjunction`; the replay does not model that, so the #609 cut leaves the trigger alone.

## fn a_world_file_member_chains_through_selected
- Backlog numbers removed from the comment (original text): Backlog #611(e), Portage 3.0.83 on a fixture copy with the atom in the world file, `emerge -p --backtrack=0 @world` (`LLM/evidence/2026-11-06-sweep/611e/`): the masked block's chain ends `"@selected" [set]`, `"@world" [argument]`, and the autounmask comment `# required by @selected`, `# required by @world (argument)`.
