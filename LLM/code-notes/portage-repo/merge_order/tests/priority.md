# crates/portage-repo/src/merge_order/tests/priority.rs
Dep-key/priority mapping and ignore-ladder pins.

## fn dep_edges_carry_the_evaluated_atom_for_candidates_display
- Backlog #138 (Phase 5b S3): Portage's `--debug` prints `Depstring:` raw and `Candidates:` evaluated -- the evaluated form must ride the edge beside the raw token. The parent was built -flip, so `~dev-libs/x-1.0[flip=]` evaluates to `[-flip]` (oracle: `logs/l111-s0-20260921/real-rest-debug.log`, bed `l0-fx-20260922T192350Z` arm A).

## fn ignore_predicates_pin_every_ladder_rung
- Mirrors the ladder's own documented rule; any deleted `!`, flipped connective, or constant-return mutant disagrees on some word.

## fn split_disjunctive_keeps_a_group_when_a_second_group_marker_is_pending
- A structured reduce flattens nested `||`, so this pins the guard against a future walker change.

## section follow-up
- Survivor of the backlog #146 after-run.
