# crates/portage-repo/src/tests/force_scan.rs
Slot-change probe and update force-scan tests: moved children, stale binaries.

## fn slot_operator_slot_change_probe_schedules_a_moved_installed_child
- Bug 456208 shape, from the fixtures: `kde-base/ark`'s tree `RDEPEND=app-arch/libarchive:=` (an unbuilt slot operator) against installed `libarchive-3.1.1` (vdb `SLOT` 0, tree `0/13`): a slot move without a revbump. Portage `_slot_change_probe` (`depgraph.py:2317-2359`) schedules the installed child.
- Portage's `dep.child.built`: a merge-bound resolution (an upgrade) does not fire the probe. The S4 undo latch suppresses a re-add.
- The `souneedslot` fixture (`dev-libs/souprov:=`, installed `souprov-1.0` vdb 0/1, tree `SLOT="0/1"`) is the unmoved control.

## fn slot_operator_slot_change_probe_fires_for_upgrade_and_ignores_plain_slot_deps
- Backlog #161 S1: bug 456208 is not `Reinstall`-only. Installed child moved (vdb 0/9 vs tree 0/1).

## fn slot_operator_slot_change_probe_keeps_a_binary_whose_ebuild_is_invisible
- #243: Portage `_slot_change_probe` (`depgraph.py:2347`). A binary at SLOT 0/1 whose tree ebuild moved to 0/2 and is keyword-masked (`~amd64` against an `amd64`-only config) is left alone; both trees moved 0/1 -> 0/2, only `hiddenchild`'s ebuild is masked.

## fn slot_operator_slot_change_probe_masks_a_stale_binary
- Backlog #212 (v2 `#24c`): Portage `_slot_change_backtrack` (`depgraph.py:2381-2398`). Shape: an ebuild parent with unbuilt `:=` deps; the graph resolved `dev-libs/maskchild` to a binary built at SLOT 0/1 while the tree ebuild at the same cpv moved to 0/2 without a revbump (the S0 `somaskchild` shape).
- Stale maskchild vs unmoved samechild (tree and binary agree on 0/1) vs ebuild-less notreechild; no installed instance exists in the scratch root, so nothing schedules.
- `--exclude` on the tree ebuild vetoes the mask (Portage `:2343`); a built (binary) parent is not a probing parent (Portage's `not dep.parent.built` gate, `:2324`).

## fn slot_operator_update_force_scan_forces_the_installed_childs_update
- #236 A: Portage `_slot_operator_update_probe` + `_slot_operator_update_backtrack` (`_emerge/depgraph.py:2576-2800`, `:2400-2452`) on the shape the `_minimize_children` collapse leaves behind: the walk bound the installed consumer's built `>=forceprov-1.0:0/1=` and live `forceprov:=` to the installed child, and no update of the child is in the graph. Forces both halves (consumer into the replace set, child's candidate into `slot_operator_forced_upgrade`).
- A second in-scope parent pinning `<forceprov-2.0` refuses it (Portage `_slot_operator_check_reverse_dependencies`); without `--update` the probe does not run (`want_update`).
- Portage's own answer on this shape: the ResolverPlayground probe in `LLM/evidence/2026-09-29-236/` (`slotop`: `backtracking due to missed slot abi update`, then `U provpkg-2.0 + rR consrdep`).
