# crates/portuale/src/ebuild_merge/tests/installed_instance.rs
Installed-instance, SLOT-reading, CONTENTS-lookup and install-mask coverage for the merge path.

## fn installed_instance_pf_picks_the_highest_counter_same_slot_version
- No moved context.

## fn replacing_versions_is_the_installed_same_slot_version
- #158: Portage's `REPLACING_VERSIONS` is the version of the installed instance(s) this merge replaces (`vartree.py:4769`), which is exactly the `{pn}-{version}` PF `installed_instance_pf` selects.

## fn read_installed_slot_returns_0_for_a_slot_less_vdb_entry
- #126 (O5 ruling 2026-09-23): Portage's `aux_get` translates a missing/empty SLOT to "0", so a SLOT-less entry groups with slot-"0" entries instead of being skipped.

## fn read_installed_slot_returns_0_for_an_empty_slot_file
- No moved context.

## fn read_installed_slot_collapses_a_multiline_slot_before_the_main_slot_projection
- #126: through the seam a multi-line SLOT collapses (`" ".join(myd.split())`, Portage's `_aux_get`) where the old bare `.trim()` kept the embedded newline.
- #115 S1: the collapsed value (`"0 1"`) is itself invalid, so the seam translates it on to `"0"` (Portage's `aux_get`'s `_get_slot_re` rule) -- the pre-S1 expectation is kept in git history.

## fn owned_node_value_pf_reads_an_obj_and_a_sym_entrys_own_value
- No moved context.

## fn apply_install_mask_deletes_matched_paths_and_records_the_mask
- No moved context.
