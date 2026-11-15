# crates/portage-repo/src/tests/bootstrap.rs
Comment text moved out of the source (review of branch `opencode`, backlog #763).

## fn bootstrap_sequence_is_boot_middle_definitive

- Charter 2a (backlog #753): the owner's pair serializes to exactly three
  rows — the reduced-USE boot build, the middle, the definitive rebuild —
  with exact USE sets, the exact marker, and exact `merge_order` 0/1/2.

## fn bootstrap_graph

- The owner's pair over the fixture tree, both flags default-on: the
  genuine build-time 2-cycle the #749 report names.

## module

- Charter 2a bootstrap (backlog #753): the resolver synthesizes a
  reduced-USE boot row after the sort instead of failing the genuine
  USE-conditional cycle. The owner's pair resolves to three rows —
  `otherpkg[boot] -> pkgnew -> otherpkg` — with the `bootstrap` marker
  on the boot row only; unbreakable shapes keep today's failure.
