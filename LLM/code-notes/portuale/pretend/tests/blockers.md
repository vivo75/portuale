# crates/portuale/src/pretend/tests/blockers.rs
Replacement-satisfied blocker rows: hidden, inline, and counted.

## fn replacement_satisfied_blocker_rows_are_hidden_or_inline
- #72 B1/B2: a `Replacement`-satisfied row is carried on the entry; it prints inline right after the replacement's line and counts only when the replacement waits on the owner's merge (`replacement_wait_index`); otherwise it contributes neither a `[blocks]` line nor a `Conflict:` count. An `Uninstall`-tagged row keeps its current trailing display.
- The uninstall-tagged row trails, the replacement row hides (there is no replacement entry in this list at all).
- The B0b p2c shape: the replacement's `~dev-libs/bparent-1.1` BDEPEND is unsatisfied (no installed bparent in `/nonexistent`), so the row prints inline after the replacement (display index 2) and counts.
- `--columns` suppresses the line but the counter still sees it (Portage counts every Blocker node in the merge list).

## fn replacement_wait_follows_the_selected_disjunctive_branch
- #76 B2 / B0 q1-q3: `dep_zapdeps` collapses a `|| ( … )` group before the serializer runs, so a wait chain through the **kept** branch is a wait, and a suppressed branch is not followed.
- `|| ( ~bparent-1.1 absent )`: the wait branch is the kept one (B0 q1), so the row prints inline after the replacement.
- `|| ( ~other-1.0 ~bparent-1.1 )`: both branches are in graph, the first wins (B0 q2's all-in-graph ranking), so the bparent alternative is suppressed and the row hides.

## fn kept_alt_for_display_is_derived_once_and_shared
- #84: the display list's shared kept set must equal the per-call `kept_alt_branches` derivation it replaces, and must stay empty (free) when no `Replacement` row can consult it.
- The owner-restricted form (what `trailing_blocker_lines` uses) derives for the owner that carries the row and for nobody else.
- The predicate consults exactly the shared set: the kept branch waits on the replacement. And an empty set (no derivation) suppresses the same edge.
