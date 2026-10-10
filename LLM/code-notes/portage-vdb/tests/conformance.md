# crates/portage-vdb/tests/conformance.rs
Backend conformance suite for `portage-vdb`: every backend answers the same reads.

## module
- Landed as plan step S2.2 (backlog #305, feature #157).

## fn set_counter_round_trips_or_names_its_step
- `FilesDb::set_counter` is still `Unsupported` (planned follow-up steps S2.6/S2.7; the `files` stub says "S2.6").

## fn preserved_libs_round_trip_and_unchanged_write_is_skipped
- `entries == loaded` writes nothing (the N6 no-op rule, mirroring Portage's `store()` `_data == _data_orig` check).

## fn reverse_dependents_*
- Per lib.rs module-doc item 19, `FilesDb` does not implement `reverse_dependents` (the scan stays in portage-repo over `aux_get`); planned step S8.1 would flip `Caps::reverse_dependents` to `None` for `files`.
- The superset rule and unsure entries come from plan step S8.1.

## fn owners_follow_the_claim_paths_rule_on_edge_spellings
- Plan step S8.2.

## fn rebuild_index_recomputes_the_same_index_or_names_its_step
- Plan step S8.3; per-backend damage repair is tested in `sqlite.rs` / `redb_db.rs`, which can reach the rows.

## fn read_file_at_is_supported_or_names_its_step
- Every backend implements it now.

## fn snapshot_is_supported_or_names_its_step
- `FilesDb::snapshot` is `Unsupported` ("nothing on `files` uses it"; planned step S3.2).

## fn entry_image_*
- `FilesDb` `entry_image` / `insert_entry` are `Unsupported` (planned steps S2.6/S2.7; the `files` stubs say "S2.6"). The converters need them.

## fn dir_mode_is_permission_bits_on_every_backend
- Backlog #307: a merged entry's directory reports `0o7xx`, never `0o040xxx`, in both image and stat, so `verify` of a files VDB against a natively merged sqlite/redb one has no "directory mode" finding.

## backends: files / sqlite / redb
- `entry_image`, `insert_entry` and `set_counter` are implemented on files too (planned step S2.6); sqlite implements every method (planned step S2.5); redb implements every method in one redb write transaction per `begin_write` (planned step S5.3).
- `Caps` step-name strings (`Some("S3.2")`, `Some("S8.3")`) are functional: `assert_unsupported` reports them.

## fn redb_handle
- redb refuses a second read-write handle on one file, in this process too (`Error::Busy`); the factory hands out the one live handle per path (a `Weak` table, so it closes with its `Ctx`).

## fn preserved_libs_format_and_parse_round_trip
- The `files` formats the converters read and write follow Portage's `json.dumps` layout.

## fn sealed_entry_serves_a_non_utf8_field_lossily_like_an_unsealed_one
- Backlog #309: Portage's consolidation reads with `errors="replace"` (`vartree.py:268`; `_aux_get` too, `vartree.py:1029`).

## fn an_in_place_field_rewrite_is_served_stale_until_the_dir_mtime_moves
- Backlog #309: Portage's own property (`vartree.py:938-951`; its `aux_update` calls `_bump_mtime` for that reason).
