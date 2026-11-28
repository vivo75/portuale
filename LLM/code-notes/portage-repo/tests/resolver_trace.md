# crates/portage-repo/src/tests/resolver_trace.rs
Context moved out of the code comments.

## fn installed_child_row_displays_the_vdb_use_not_an_empty_string
- Backlog numbers removed from the comment (original text): #134 (Phase 5b S4): an installed `Child:` row's `USE="…"` column must carry the vdb-recorded USE, not `""`. The fixture's vdb is `IUSE=flip` / `USE=` (built -flip), so `installed_use_str` renders `-flip` -- the minimum bar S0's capture sets (`port-rest-debug.log:5990`'s shape). Real's parenthesised / USE_EXPAND / cpv-suffix richness is a recorded residual, not this pin's subject.
