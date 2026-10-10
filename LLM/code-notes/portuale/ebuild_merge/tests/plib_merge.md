# crates/portuale/src/ebuild_merge/tests/plib_merge.rs
Merge-side preserved-libs coverage: register/unregister semantics, the `-MERGING-` feed, and `NEEDED.ELF.2` pruning.

## fn register_preserved_libs_unregister_only_matches_the_same_cpv_and_counter
- Portage's `unregister` is `register(cpv, slot, counter, [])`: removes the `cps` entry only when it still records the *same* `cpv` and `counter` -- a different package (or a stale counter) sharing the same `category/pn:slot` key must survive untouched.

## fn register_preserved_libs_with_paths_unconditionally_overwrites
- Portage's `register` with non-empty `paths` has no same-cpv/counter guard the way empty-`paths` (`unregister`) has.

## fn merge_side_preserve_sees_the_replacing_packages_merging_needed
- Backlog #229 (merge-side half of #224): Portage's `dblink.treewalk` feeds the replacing package's build-info `NEEDED.ELF.2` into the merge-side preserve computation explicitly (`self._linkmap_rebuild(include_file=needed)` before `_find_libs_to_preserve()`), because the new entry still sits in its `-MERGING-<pf>` temporary and enumeration (`cpv_all()`) skips it. A library owned by the replaced instance whose only consumer is the replacing package itself must therefore be preserved merge-side; without the feed the consumer is invisible and the lib is never preserved (missed-preserve, so the file is unmerged with the old instance).
- Same hand-written-`NEEDED.ELF.2` seed shape as the #224 test, except the provider is the replaced same-slot instance itself (`oldapp-1.0`, live) and the only consumer (`/usr/bin/newtool`, shipped by the new image) is known solely to the `-MERGING-oldapp-2.0` temporary -- the Portage merge-side state since #183 (`populate_vdb_tmp` runs before the preserve block). The call goes through `find_preserve_paths_for_merge` itself (new `new_pf` parameter), so this test compiles and fails without the fix and passes with it.
- Seed: the replaced same-slot instance, live, owns the library; the new image ships the consumer but not the library.

## fn remove_from_contents_prunes_the_matching_needed_elf2_entry_too
- Portage's `removeFromContents`'s own `NEEDED`-line stripping (`vartree.py:1279-1310`): Portage stale-linkage-data prevention for a *later* `LinkageMap.rebuild()`'s own preserve-libs decision (see `remove_from_contents`'s own doc comment).

## fn remove_from_contents_does_not_create_a_needed_elf2_file_that_never_existed
- Portage's `if new_needed is not None:` (`writeContentsToContentsFile`): no file is conjured into existence just because a `CONTENTS` entry happened to be removed.

## fn remove_from_contents_leaves_needed_elf2_untouched_when_nothing_was_removed
- Portage's `if removed:` (`vartree.py:1279`): when none of the given `paths` actually matched a Portage `CONTENTS` entry, `NEEDED.ELF.2` isn't even read, let alone rewritten -- untouched, byte for byte.
