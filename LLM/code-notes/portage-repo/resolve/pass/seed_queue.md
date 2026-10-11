# crates/portage-repo/src/resolve/pass/seed_queue.rs
Seed the walk queue with the top-level and auto-replace atoms.

## fn seed_queue
- Backlog #90 (S1): Portage feeds same-level atoms through its `_dep_stack`, so the last declared resolves first; selection is order-sensitive since an already-graphed satisfier is reused (`_select_package:8370`) -- forward order graphed `2.0` for a bare dep then recorded a conflict Portage never sees (the `slotconflictparent` bt0 silence). `top_level_cps` still comes from `req.atoms` in declaration order, untouched.

- Backlog #24 (S3): Portage `_gen_reinstall_sets` (5457-5480) turns `_slot_operator_replace_installed` into the pseudo `SetArg` `@__auto_slot_operator_replace_installed__`, appended after the user's own args (`select_files` 5430). `owner: None` mirrors the set arg having no package parent. `depth: 1` is Portage's `reset_depth=False`: the node must not interact with `--deep=N` depth accounting, and -- portuale-side `depth == 0` means top-level argument everywhere (`resolve_pretend`'s `toplevel`, the `(Argument)` slot-puller line). `top_level`/`top_level_cps` come from `req.atoms`, untouched by construction. `_eliminate_rebuilds` rule 5 excludes the auto sets from its "the user asked for it" test (3859-4000). The set is walked as a root node, which is why the rebuilt consumer gets its deps re-walked at all.
- Backlog #236 (A): the probe's upgraded child rides the same auto set (Portage `abi_reinstalls.add((child.root, child slot atom))`), seeded at the exact candidate the probe chose.

