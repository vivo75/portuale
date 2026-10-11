# crates/portage-repo/src/resolve/pass/step_queue_item.rs
Process one queued atom through resolve, graph and enqueue.

## fn step_queue_item
- Portage `--autounmask-use` PART B (`_apply_parent_use_changes` -> `_show_unsatisfied_dep(collect_use_changes=True)`, `depgraph.py:5820`/`6768`): a dependency's use-dep was originally conditional on the *requesting parent's* own USE, and no candidate satisfies the evaluated form. Details: `opt?`/`opt=` forms; `suggested_parent_use_candidate` into `_needed_use_config_changes`; the graph already failed so `_success_without_autounmask` fails the resolve with no merge rows (plus the "terminated early" notice with `--autounmask-backtrack` off). With `--autounmask-backtrack=y` (Slice 4) the flip is folded into `autounmask_use_config` (keyed `(parent_cat, parent_pkg)` -- exactly a `package.use` entry) and the driver's `autounmask_grew` restart re-walks everything instead. A probe re-resolution of just the freed atom still gates both arms, so only a flip that actually helps is recorded or folded. `--autounmask-use=n` suppresses it via the shared `autounmask_suggest_use` gate.

