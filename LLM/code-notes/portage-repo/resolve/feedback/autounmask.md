# crates/portage-repo/src/resolve/feedback/autounmask.rs
Autounmask breakage reporting and grown --autounmask-use config re-runs.

## fn step_autounmask
- Portage `_autounmask_breakage` (depgraph.py:12262-12280): an accumulated autounmask USE change made another use-dep unsatisfiable and the two can't be reconciled (a flag wanted both ways). Drop every autounmask change, turn suggestion fully off, and re-resolve one final clean pass -- exactly Portage's `myparams["autounmask"] = False` retry. The `autounmask_disabled` latch keeps it to a single extra pass.
- Backtracking slice: this pass folded a new `--autounmask-use` flip into `autounmask_use_config` (Portage `_feedback_config` growing `_needed_use_config_changes`). Re-run the whole walk with the grown config so the flipped package is re-resolved with the flag on and its `flag?`-gated deps appear. Converges because `autounmask_grew` is only set on a *newly*-added `(cp, flag)` entry. Config feedback is budget-free (Portage's `_feedback_config` doesn't count toward `--backtrack=N`).
