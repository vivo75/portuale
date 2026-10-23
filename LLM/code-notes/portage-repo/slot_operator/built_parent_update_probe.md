# crates/portage-repo/src/slot_operator/built_parent_update_probe.rs
The update probe for a merge-bound binary parent.

## fn other_consumers_accept
- Backlog #491 [A]. Portage `_slot_operator_update_probe` only accepts a replacement child when `_slot_operator_check_reverse_dependencies(dep.child, pkg, replacement_parent)` (`depgraph.py:2472`) passes: every other graph parent of the child must still match the new child, with built `:slot/sub=` atoms reduced to `:=`.
- Exemptions in Portage that this port mirrors: the replaced parent itself; a parent that is itself being replaced in the graph; a parent with a visible upgrade (`_upgrade_available`); a parent with a direct digraph edge to the child (`digraph.has_edge(parent, existing_pkg)`, `:2523`). Not mirrored: `_in_blocker_conflict` and `_too_deep`.
- The edge exemption decides `@world` versus a narrow request. In `@world` the installed `weston-16.0.0` is a root with an edge to `libdisplay-info`, so `<libdisplay-info-0.4.0:=` is exempt and the binary `mesa` is masked (Portage P1). In `emerge -uDpvN =kde-frameworks/kdbusaddons-6.31.0` `weston` only enters through the complete graph without an edge, so it refuses 0.4.0 and Portage masks nothing. Portuale approximates "has an edge" by `request_walk_consumers` (installed closure of the request atoms, not through replaced packages) and "is in the graph" by the live consumer set (the blocker-retry closure on a plain pass).
- Portage runs the probe after `_complete_graph`; portuale's first pass is plain, hence the blocker-retry stand-in.
