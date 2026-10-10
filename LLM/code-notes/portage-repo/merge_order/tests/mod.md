# crates/portage-repo/src/merge_order/tests/mod.rs
Shared helpers for the merge-order unit tests.

## fn aub0_entries
- Backlog #244: the `aub0` shape (upstream `test_autounmask_use_breakage`).

## section dep-key / priority mapping
- Survivor of the backlog #146 dep-key/priority-mapping work.

## section installed / vdb-backed inputs
- Survivor of the backlog #145 installed/vdb-backed-inputs work.
- `mo_tmp_vdb` mirrors `lib.rs`'s own `tmp_vdb` helper; the committed fixture vdb has no injected-libc record, which is why tests build their own.

## fn new_entry
- Mirrors `synthetic_installed_entry`'s full field list.

## fn Rng
- Deterministic xorshift64*: no rand dependency for a unit test.
