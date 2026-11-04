# crates/portage-repo/src/resolve/pass/seed_queue.rs
Seed the walk queue with the top-level and auto-replace atoms.

## fn seed_queue
- Backlog #90 (S1): Portage feeds same-level atoms through its `_dep_stack`, so the last declared resolves first; selection is order-sensitive since an already-graphed satisfier is reused (`_select_package:8370`) -- forward order graphed `2.0` for a bare dep then recorded a conflict Portage never sees (the `slotconflictparent` bt0 silence). `top_level_cps` still comes from `req.atoms` in declaration order, untouched.

- Backlog #24 (S3): Portage `_gen_reinstall_sets` (5457-5480) turns `_slot_operator_replace_installed` into the pseudo `SetArg` `@__auto_slot_operator_replace_installed__`, appended after the user's own args (`select_files` 5430). `owner: None` mirrors the set arg having no package parent. `depth: 1` is Portage's `reset_depth=False`: the node must not interact with `--deep=N` depth accounting, and -- portuale-side `depth == 0` means top-level argument everywhere (`resolve_pretend`'s `toplevel`, the `(Argument)` slot-puller line). `top_level`/`top_level_cps` come from `req.atoms`, untouched by construction. `_eliminate_rebuilds` rule 5 excludes the auto sets from its "the user asked for it" test (3859-4000). The set is walked as a root node, which is why the rebuilt consumer gets its deps re-walked at all.
- Backlog #236 (A): the probe's upgraded child rides the same auto set (Portage `abi_reinstalls.add((child.root, child slot atom))`), seeded at the exact candidate the probe chose.


## fn seed_queue (#394 restart seeds)
- Backlog #394: the `rebuild_if_replace_installed` set rides the same internal-arg shape as the slot-operator set (`owner: None`, `depth: 1` -- no `--deep` accounting, no `(Argument)` puller line, never in `top_level_cps`). Portage: `_gen_reinstall_sets` (`depgraph.py:5460-5480`).

## fn seed_queue (virtual expansion seeds)
- Backlog #572: `BacktrackParams::virtual_expansion_seeds` ride as the virtual's own deps (`owner`/`owner_version` set, `depth: 1` like the other auto seeds -- no `--deep` accounting, no `(Argument)` puller line). Portage attributes expanded atoms to the virtual (`dep_check.py:_expand_new_virtuals`: `atom_graph.add(pkg, virt_atom_node)`), which is what makes the fall-through arm merge as a child of the kept `[nomerge]` virtual.

## fn seed_queue (#651 transient-demoted seeds)
- Backlog #651: a seed whose guard-demotion input was transient never rides. When every recorded demoter is masked in the pass's backtrack state (`demoter_masked`: exact `!=cat/pkg-ver` hit in `runtime_pkg_mask`, the `check_runtime_pkg_mask` precedent -- a `[binary]`-marked negative masks a binary candidate, never a demoter version), the installed arm the demotion held down survives Portage's post-skip re-resolution instead (`dep_check.py :636-655`/`:599-616`), so the seed is skipped with a `SEED skip ... demoters={...} masked` trace line (identity + verdict per seed). A seed with no demoters has no transient input and always rides. The verdict comes from the existing mask state, never a new walk; the queue contract is otherwise untouched. Latch note: the skipped seed stays in `grown` (the queue step only reads) -- it is skipped again on every masked pass and rides again on an unmasked sibling node, so the search shape is unchanged.

## fn demoter_masked
- Backlog #651: exact-`!=cpv` mask probe over `runtime_pkg_mask` for one recorded demoter version. Same granularity as `check_runtime_pkg_mask` (a different masked version does not void); `[binary]` marks never match a demoter.
