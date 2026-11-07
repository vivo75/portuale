# crates/portage-repo/src/resolve/pass/assemble_pass.rs
The pass result from the finished walk state.

## fn assemble_pass_result
- Backlog #90 (S1): the `WARNING: … package.provided:` block lists requested atoms in requested order (not walk order); atoms absent from argv sort last, stably. Staged oracle: `providedpkg` before `providedpkg2`. Only depth-0 pushes record, so absent-from-argv shouldn't happen.

## fn mark_walk_or_choices
- Backlog #625(b): turns `PassState::walk_or_choices` (filled by `disjunction_stack.rs` `enqueue_chosen_atoms`, keyed by the owner's `(category, package, version, targets_running_root)`) into `DepEdge::walk_chosen`. Per `(key, group)` the lowest-numbered branch all of whose atoms (`atom` or `evaluated` text) are in the owner's chosen set is marked; a group with no such branch stays unmarked and falls back to `suppressed_alt_edges`' ranking. The root flag is part of the key: a cross-root package (g216comp) has one entry per root with different choices (`tests::graph::cross_root_bdepend_self_branch_merges_in_both_roots`).
