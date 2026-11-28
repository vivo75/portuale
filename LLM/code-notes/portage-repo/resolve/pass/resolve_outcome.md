# crates/portage-repo/src/resolve/pass/resolve_outcome.rs
One `resolve_pretend` call against the pass-constant constraints.

## fn resolve_queue_atom
- `resolve_atom` is shared with the #236 (A) forced-upgrade flip (same pass-constant inputs, another atom); the minimize group loop passes each sibling's own flag while the single resolve uses the popped item's.
- Backlog numbers removed from the comment (original text): The slot-operator auto-rebuild set is an argument (`__auto_rebuild__`): a rebuilt package is selected like one and takes its best visible version (#728). Portage: depgraph.py, _want_update_pkg (`arg_atoms`), _gen_reinstall_sets

## S2 (#362): depth/deep/complete/args/buildtime plumbing
- `resolve_queue_atom` passes the queued `depth`, `ctx.deep`, `ctx.complete` (as `complete_mode`) and `ctx.atoms` (as `arg_atoms`, set members included -- they seed at depth 0 with `reset_depth=True` semantics) into `SelectOptions`, plus the item's `buildtime_hard` (used only by the installed-only fallback: build-time-only edges never satisfy there). Sibling callers pass their own depth/edge: the minimize group prediction per-sibling `bh`, the forced-upgrade flip and the parent-use-flip probe their own item's flag, the running-root build walk (`root_deps.rs`) marks its entries build-time.

## gate threading (#571)
- The `SelectSource` site uses `ctx.gate()`.
