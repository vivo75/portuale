# crates/portuale/src/ebuild_merge/tests/after_install.rs
Replace-loop prune coverage for the unmerge path: the replacing package's `-MERGING-` NEEDED feed.

## fn replace_loop_prune_sees_the_replacing_packages_merging_needed
- Backlog #224: Portage's `dblink.unmerge(needed=...)` feeds the replacing package's `NEEDED.ELF.2` lines into the replace-loop prune explicitly (`_prune_plib_registry` -> `_linkmap_rebuild(include_file=needed, ...)`, because the new entry still sits in its `-MERGING-<pf>` temporary and enumeration (`cpv_all()`) skips it). A preserved library whose only remaining consumer is the replacing package itself must therefore survive the replace-loop prune; without the feed it looks orphaned and is deleted + unregistered.
- Same seed shape as `replacement_unmerge_keeps_a_preserved_lib_needed_by_the_replacing_consumer` (hand-written `NEEDED.ELF.2` lines, per the brief's "or a hand-written NEEDED line"), except the replacing entry is `-MERGING-newapp-1.0` -- the Portage replace-loop state since #183, invisible to `read_all_needed_entries` -- and the call goes through the replace loop itself (`unmerge_replaced_same_slot`, whose signature is unchanged: it reads the feed from that temporary entry), so this test compiles and fails without the fix and passes with it.
- The `CONTENTS` entry is what the `also_keep` ownership check sees (via the `-MERGING-` fallback).
