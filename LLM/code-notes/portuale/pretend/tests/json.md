# crates/portuale/src/pretend/tests/json.rs
Comment text moved out of the source (review of branch `opencode`, backlog #763).

## fn bootstrap_json_marks_only_the_boot_row

- Charter 2a (backlog #753): a boot row carries the `bootstrap` marker
  and the minus-spelled reduced flag; every other row carries neither
  key, so acyclic plans are byte-identical.
