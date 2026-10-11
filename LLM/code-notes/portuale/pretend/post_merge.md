# crates/portuale/src/pretend/post_merge.rs
After a merge: the info-dir update, the preserved-libs and news notices, and the world-file record.

## fn show_post_merge_notices
- `elog_process` is no longer batched here: Portage runs it per-package inside `dblink.merge()` (before the post-merge `clean`, `vartree.py:6160-6198`), and backlog #42's clean now removes `${T}` -- so `ebuild_merge::process_merge_elog` is called from `merge_after_install`/`merge_binpkg` themselves. The `echo` summary still reaches stdout, the `save`/`save_summary` files the logdir, and the `mail`/`mail_summary` the MTA exactly as before (see `elog.rs`).
- Ports Portage's `post_emerge()` (`post_emerge.py:126-130`): the vdb-changed gate is structural here -- every merge failure returned early above, `--pretend` never reaches this branch, `buildpkgonly` merges nothing, and an empty mergelist changed nothing either.
- Backlog #196: ports Portage's `post_emerge()` (`post_emerge.py:155`), which prints the GLEP 42 count notice a second time once the merge changed the vdb (after the preserved-libs notice and the config-file check, both mirrored above) -- same FEATURES-plus-nonzero-count gates, no `--quiet` gate. `--buildpkgonly` builds but never merges (Portage's `_pkgs_changed` stays false, so its `post_emerge` early-returns with no notice). A merge failure prints the notice too when the vdb changed (Portage prints regardless of retval, `post_emerge.py:104-108,155`).
- Ports Portage's `post_emerge()` (`post_emerge.py:141-152`): once the merge changed the vdb, warn about any library the merge/unmerge just preserved and point the user at `emerge @preserved-rebuild`.

## fn record_world_after_merge
- Ports Portage's `Scheduler._world_atom` / `saveNomergeFavorites`.
- A directly-given `@name` set arg is recorded in `world_sets` (same suppression set as the world file). Backlog #232 review (Important-2): non-candidate sets are skipped (`depgraph.py:11366`).
- Portage's `saveNomergeFavorites`'s `@set` half: a directly-given `@name` set arg is recorded in `world_sets` (same suppression set as the world file); the `usersets_world_candidate` filter is the same as the deferral arm above.
