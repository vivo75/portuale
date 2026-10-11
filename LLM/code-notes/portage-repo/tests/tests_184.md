# crates/portage-repo/src/tests/tests_184.rs
Shadowed-arm order legs for `direct_solve_instance_use` / `direct_solve_atom_matches`.

## module
- Backlog #184, in the #166 scratch style. Kept self-contained (own scratch repo helpers copied from `tests_166`) so the other Track U branches' blocks rebase mechanically.

## mod tests (shadowed arm swap)
- The shadowed (no-entry) arm of `direct_solve_instance_use` returned `slot_conflict_flag_sets`'s `(iuse, use_flags)` = `(declared, enabled)` pair as-is against the documented `(enabled, declared)` contract (the installed and entry arms both return `(enabled, declared)`, and the caller `direct_solve_atom_matches` destructures `let (enabled, declared) = ...`).
- Portage evaluates the atom's USE deps against the package's enabled USE with its IUSE as the valid set: `Package.with_use` (`3rdparty/portage/lib/_emerge/Package.py:870`) applied to `depgraph._pkg_use_enabled` (`3rdparty/portage/lib/_emerge/depgraph.py:7669`). The swap evaluated them against the IUSE as if every declared flag were enabled.

## fn direct_solve_instance_use_184_shadowed_reads_tree_effectively
- IUSE `+fa fb` enables only `fa` (the `+` default) while declaring both. Before the fix the arm returned `slot_conflict_flag_sets` as-is, i.e. `(enabled, declared) = ({fa, fb}, {fa})`.

## fn direct_solve_atom_matches_184_shadowed_rejects_disabled_flag
- Before the fix the swapped arm put the whole IUSE in the enabled position, so the dep wrongly matched.
- Plain `[fb]` cannot discriminate: the `use_deps_satisfied` valid-set gate rejects a default-less flag outside IUSE either way.
