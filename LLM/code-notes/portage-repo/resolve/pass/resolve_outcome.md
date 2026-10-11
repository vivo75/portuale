# crates/portage-repo/src/resolve/pass/resolve_outcome.rs
One `resolve_pretend` call against the pass-constant constraints.

## fn resolve_queue_atom
- `resolve_atom` is shared with the #236 (A) forced-upgrade flip (same pass-constant inputs, another atom); the minimize group loop passes each sibling's own flag while the single resolve uses the popped item's.

