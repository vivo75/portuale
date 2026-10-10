# crates/portage-repo/src/merge_order/tests/build.rs
`build_digraph` scheduling-graph pins.

## fn self_referential_disjunctive_bdepend_skips_the_self_branch
- Backlog #53: `dev-lang/mogo`'s own BDEPEND is `|| ( >=mogo-1.24 >=mogoboot-1.24 )`. The resolver picks the non-circular `mogoboot` branch (L0 finding D, `b5b256f`) -- `alt_suppressed` must agree, not let the self branch win via its trivial "matches the owner's own not-yet-merged graph node" match, which used to add a permanent `mogo -> mogo` buildtime self-edge and demote the portage edge to `mogoboot` down to the weaker `required_by`-fallback `runtime` priority.

## fn nvc_entries_do_not_participate_in_minimize_elimination
- Backlog #85 follow-up (`opartlya` disclosure order): a `NoVisibleCandidate` entry is never a selected package -- Portage's `_select_package` returns None for its atom, so it never enters `_minimize_children`'s elimination. Here the bare atom matches both the NVC entry and the installed 1.0; without the NVC exclusion the ascending-version elimination drops the NVC entry (its empty version sorts first) and the edge moves to the installed entry, flipping the merge order of the disclosure rows. With it, the pre-#85 ranking stands and the NVC entry keeps the edge.
