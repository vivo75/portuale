# crates/portage-repo/src/resolve/feedback/slot_operator_probe.rs
Dead-end-pass trigger seeding an installed parent's unsatisfiable built `:=` dep into the replace set.

## fn step_slot_operator_probe
- Backlog #215 (v2 `#24f`): Portage `_slot_operator_unsatisfied_probe` (`_emerge/depgraph.py:2817`, fired in-walk at `:3447-3458`) as a dead-end-pass trigger -- see `slot_operator_unsatisfied_probe`'s own notes. Seeds `slot_operator_replace_installed` (Portage's `backtrack_infos["config"]` + `_need_restart`, the same `BacktrackFeedback::Config` the #211 scan returns) instead of aborting.
- Backtrack-gated like Portage's `_allow_backtracking` (`:3447`): under `--backtrack=0` the abort stands. The seed only ever grows the set, so the search terminates; a pass with no replacement stays a dead end below.
- #257: a non-installed built (binary) parent is masked like #212's stale binaries (`[binary]`-marked `SlotOperatorBuilt` negative, `runtime_pkg_mask` bucket), Portage `:2896-2903`. Filed through the shared helper with the trigger arm above.
