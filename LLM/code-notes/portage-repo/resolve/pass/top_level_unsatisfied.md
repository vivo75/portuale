# crates/portage-repo/src/resolve/pass/top_level_unsatisfied.rs
Drops or aborts a top-level atom with no visible candidate, Portage's argument handling.

## fn step_top_level_unsatisfied
- Portage `depgraph.py:5577-5605` (`_resolve`): explicit arguments fail the run, set members land in `_missing_args`. The missing parent is an installed one with no merge-bound replacement (the #276 shape: both instances masked by now), so the healed rest settles and the display layer reports the skipped update plus the world-file problem.
- Backlog #210: see `complete_mode_withheld_version`. `slot_op_reachable` is populated on the CLI's complete pass and its `--deep` re-run alike, empty otherwise.
- USE-miss block: Portage `depgraph.py:6969-6994`, with `AtomArg` parents getting no `(dependency required by …)` lines (`:7080-7090`). Portage's staging `for <root>.` suffix stays absent (the long-standing `fixture-miss-message-unsuffixed` class); backlogs #265(b), #273. The dependency arm renders the same way.
- Keyword suggestion: see `keyword_masked_only`; portuale does not yet combine multiple simultaneous suggestion kinds the way Portage can. The on/off default-resolution logic lives in this function's caller config.
