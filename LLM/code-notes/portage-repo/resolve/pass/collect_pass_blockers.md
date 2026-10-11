# crates/portage-repo/src/resolve/pass/collect_pass_blockers.rs
Collect the pass blockers and flag tree-solved replacements.

## fn collect_pass_blockers
- Backlog #77 (A1): Portage `depgraph.py:8919-9078`, early exit at `:8908-8910`.
- Backlog #81: everything but `--tree` display ignores the flags.

