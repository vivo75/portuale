# crates/portage-repo/src/tests/merge_order.rs
Merge-order and tree-display-order tests.

## fn tree_display_order_sequences_the_p2b_tree_like_portage
- Backlog #131 S1: `tree_display_order` sequences the p2b tree like Portage's tree-mode retlist (bed `l0-fx-20260924T090502Z`: Portage's display walks `[p2bowner, newpkg, (nomerge p2bowner,) blocks b, p2btarget]`, i.e. merge direction `[p2btarget, newpkg, p2bowner]`).
- The entries are first put in the display-time (flat merge) order `[newpkg, p2btarget, p2bowner]` -- pinned independently by the flat pin -- because the scheduler's discovery seed follows the input Vec order, and `run()` hands `print_tree` the merge-ordered slice, not resolver order (everything `tree_display_order` reads -- deps, blockers, `required_by` -- is value-addressed, so the reorder is safe).
- The `@system` seed matters: the fixture profile lists `*dev-libs/newpkg` in `repo/profiles/base/packages`, and the display-time run resolves with that profile config -- so the bias pulls `newpkg` first exactly like Portage's own bias does. Plain `test_config()` has an empty system set and would seed nothing (its batch comes out `[owner, newpkg]`).
- The resolve must still graph exactly the three merge-bound entries -- the system seed reorders, never re-members.
- Display-time (merge) order, as `run()` passes it: the scheduler's discovery seed follows the input Vec order.

## fn tree_display_order_schedules_an_admitted_removal_after_its_owner
- Backlog #131 S1: an admitted removal waits on its owner in tree mode (Portage reverses the validation edge on admission, `depgraph.py:9998-10020`). u1's tree retlist is `[blockerpkg, samepkg-uninstall, (Blocker appended at selection)]` -- the owner first. Without the wait the one-at-a-time pick takes the removal (it has parents, the owner does not) and the display opens on the merge row, losing Portage's leading `[nomerge]` ancestor.

## fn uninstall_entries_follow_their_owner_in_merge_order
- Backlog #68/#72 B3 / B0 u1: `blockerpkg` blocks installed `samepkg-1.0`, resolved by removing it -- the removal entry Portage serializes right after its owner (Portage capture: ebuild row, `[uninstall]` row, `[blocks b]`).
- B0 u3: the owner's own dependency still merges first, then the owner, then the removal.
