# crates/portage-repo/src/resolve/replay.rs
Fail-fast replay of the best pass.

## fn replay_use_of
- Backlog #244. `merge_order::replay_create_graph`'s USE model: `ctx.config`, never a pass's autounmask-tiered view. An installed or binary package's USE is fixed, so the replay does not check use-deps against it.

## fn replay_walk
- Backlog #244. Portage's `_create_graph` walk over `entries`.

## fn pass_has_use_changes
- Backlog #244.

## fn breakage_clean_params
- Portage's `_autounmask_breakage` (`depgraph.py:12262-12280`): `myparams["autounmask"] = False`.

## fn run_breakage_clean_pass
- Backlog #244. Portage's clean pass after an autounmask breakage runs with `allow_backtracking=False` (`depgraph.py:12270-12279`). Portage builds the clean depgraph with no `backtrack_parameters` at all: none of the search's masks or config carries over.

## fn fail_fast_truncate
- Backlog #244. Portage's `_create_graph` returns at the first dependency that fails (`depgraph.py:3254-3271`, `:3483-3522`); portuale's pass walks everything and classifies afterwards. This runs on the breakage clean pass (autounmask off).
- Keeps the unsatisfied-dependency report of the first failing atom with its own dep chain (Portage's `_get_dep_chain` from the failing parent up the admitting edges to the argument); the other failing atoms were never reached.
- Slot conflicts: instances and parents in admission order (Portage's package tracker insertion order); a conflict the walk would only have formed after the failure never existed.
- Leaves `result` untouched when the replay finds no failure or the pass has no report for the failing atom (nothing provably better).

## fn replay_conflict_use_display
- Backlog #264. Portage's `pkg_use_display(pkg, modified_use=...)` (`_pkg_use_enabled`), grouped into `USE` + `USE_EXPAND` like every other conflict line. The merge-row display (`GraphEntry::use_expand_display`) is the wrong helper: it is Portage's `_create_use_string`, which knows only `IUSE`-declared flags and drops unchanged ones at plain `-p`, while Portage renders the conflict block through `pkg_use_display` (implicit-expand groups included, every flag shown).
- The modified set is the walk-time effective USE (`candidate_iuse_and_use`, the same source `skipped_update_use_display_for` assembles) overridden by the entry's own `use_flags_display` states -- the replay flips land there first (`apply_replay_flips`), and only `IUSE`-declared flags can flip (the flip loop skips anything outside it, same as Portage's changed-state reporting).

## fn replay_slot_conflicts
- Backlog #244. A slot-conflict block as Portage's package tracker holds it after the replayed walk (see `merge_order::replay_create_graph`): includes a requester whose use-dep the instance no longer satisfies once a later requester's autounmask change flipped it (Portage keeps the edge; the conflict block lists and marks both). Each instance's `USE="…"` is the flip-applied `pkg_use_display` (backlog #264: Portage's `_pkg_use_enabled` through `UseFlagDisplay.pkg_use_display`, grouped, not the merge row's `_create_use_string`).
- The block is built after the walk `r` replayed over `entries` (see `merge_order::replay_create_graph`): instances in admission order, each instance's parents every edge that landed on it in walk order. Conflicts or instances the replay cannot place are left as they are.

## fn apply_replay_flips
- Backlog #244. `r.needed` beyond the ones `base` already carried in from earlier passes. The row's and the slot-conflict block's `USE="…"` show the changed state (Portage's `_pkg_use_enabled`). Portuale's walk skips the flip when the requester's atom first lands on another instance of the slot, hence the `changes` recording.

## gate threading (#571)
- Replay lookups use `ctx.repo_candidates`.
