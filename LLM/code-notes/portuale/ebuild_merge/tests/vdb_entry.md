# crates/portuale/src/ebuild_merge/tests/vdb_entry.rs
Context moved out of the code comments.

## fn interrupted_vdb_write_leaves_merging_and_no_final_entry
- Backlog numbers removed from the comment (original text): Backlog #183: interrupting a merge while its vdb entry is still being written leaves `-MERGING-<pf>` and nothing at `<pf>` -- real `dbtmpdir` visibility from its creation (before `pkg_preinst` and the `${ROOT}` copy) to the `_movefile` into place after the old instances are unmerged. Drives the staged writer directly: `create` + `populate` + `write contents` is the interrupt point (no `publish` ever runs), pinning that every pre-`publish` stage leaves the live entry untouched and the temporary invisible.
