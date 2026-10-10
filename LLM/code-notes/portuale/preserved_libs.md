# crates/portuale/src/preserved_libs.rs
Advisory "run `emerge @preserved-rebuild`" notice printed after a vdb-changing run.

## module
- Ports Portage's `post_emerge()` preserved-libs advisory (`lib/_emerge/post_emerge.py:126-152`) + `display_preserved_libs()` (lib/portage/util/_dyn_libs/display_preserved_libs.py): after any `emerge` run that changed the vdb, if the `preserved_libs_registry` still has entries, print the list of preserved libraries and their consumers and tell the user to run `emerge @preserved-rebuild`.
- Portage's `post_emerge` reloads + prunes the registry first ("in order to ensure that we do not display stale data"); portuale already prunes during the merge/unmerge itself (`ebuild_merge::prune_unused_preserved_libs`), so the registry read here is already current.

## fn show_preserved_libs_notice
- Called at the end of every `emerge` action that can change the vdb (merge, `--resume`, `--unmerge`, `--depclean`, `--prune`).
- `--quiet` collapses the whole thing to a one-line `!!! existing preserved libs found`.

## fn display_preserved_libs
- Ports `display_preserved_libs(vardb, verbose)` (`display_preserved_libs.py`): the per-`cpv` `>>> package:` blocks.
- The consumer map filters out a consumer that is itself one of the *same* provider package's own preserved libs (Portage's `internal_plib_keys`).
- Same-file grouping is Portage's `samefile_map`; the non-preserved-consumer preference matches Portage (a preserved-lib consumer is rebuilt via its own block).
