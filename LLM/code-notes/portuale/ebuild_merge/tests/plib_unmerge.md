# crates/portuale/src/ebuild_merge/tests/plib_unmerge.rs
Unmerge-side preserved-libs prune coverage: replacement mode, linkage exclusion, just-preserved feed, and cycle handling.

## fn prune_keeps_a_preserved_lib_whose_only_consumer_is_old_owned_in_replacement_mode
- Fix round 1 for review item 3 (backlog #178): Portage's `unmerge_no_replacement = unmerge and not unmerge_with_replacement` (`dblink._prune_plib_registry`) -- in replacement mode a consumer owned by the old instance still keeps a preserved lib alive (it survives through the replacing package). Pre-fix, portuale always passed `true`, eliminating those consumers and wrongly pruning the lib.

## fn prune_excludes_the_replaced_instance_linkage_data
- Fix round 1 for review item 4, `exclude_pkgs` half (backlog #178): Portage's `LinkageMap.rebuild(exclude_pkgs=...)` drops the unmerged instance's `NEEDED.ELF.2` lines, so a consumer only the old instance knows about cannot keep a preserved lib alive past the replacement. Without the exclusion the stale consumer data "would only serve to corrupt the `LinkageMap`".
- Seed: the library is indexed (its provider is still installed).

## fn prune_sees_just_preserved_files_missing_from_every_needed_elf2
- Fix round 1 for review item 4, `preserve_paths` half (backlog #178): Portage's `LinkageMap.rebuild(preserve_paths=...)` scans the merge-side just-preserved set, so a preserved library whose only linkmap consumer is a just-preserved file -- indexed in no `NEEDED.ELF.2` anywhere -- is kept, not deleted as unneeded. Grounded in Portage's `LinkageMap.rebuild` (the `preserve_paths` parameter): without the feed the consumer is invisible to the prune.
- Seed detail: `libA` is indexed (hand-written provider entry, the way any still-installed owner's `NEEDED.ELF.2` would index it).

## fn find_unneeded_preserved_drops_a_self_contained_cycle
- Portage's `_find_unneeded_preserved_nodes` cycle handling (bug 652382).
