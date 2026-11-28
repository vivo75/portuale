# crates/portage-repo/src/tests/built_parent_update_probe.rs
Context moved out of the code comments.

## fn built_parent_update_probe_counts_a_restricting_consumer_the_walk_never_reaches_despite_its_upgrade
- Backlog numbers removed from the comment (original text): Portage: `_slot_operator_check_reverse_dependencies` excuses a parent that has an upgrade available only when `_too_deep(parent.depth)` is false; a `@world` member the request does not reach has an unreachable depth, so its `<dev-libs/mbabilib-2.0` atom still refuses mbabilib-2.0 (backlog #491).
