# crates/portuale/src/emerge_build/tests/bootstrap_merge.rs
Comment text moved out of the source (review of branch `opencode`, backlog #763).

## module

- Charter 2a bootstrap, real merges (backlog #753 C1/C2): the 3-row plan
  `[boot otherpkg(-useboh) -> pkgnew -> otherpkg (definitive)]` merges to
  completion under `-j1` (serial engine) and `-j2` (the parallel
  scheduler), and the boot row executes with its own reduced USE.

  The boot build's marker cannot be asserted after the full plan: the
  definitive reinstall is the same PF, so its unmerge deletes the boot
  row's files first (Portage reinstall semantics). C2 is therefore pinned
  by merging the boot row alone in its own ROOT (its marker and VDB USE
  lack the reduced flag), while the full-plan test pins C1 (all 3 rows
  land, rc 0) and the survivor's declared USE.

  Fixtures are the `dev-libs/w753*` ebuilds, whose `src_install` records
  `USE` into per-build marker files: `USE="-useboh"` in the boot build,
  full USE in the definitive one.
