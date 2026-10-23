# crates/portuale/src/binpkg/tests/inner.rs
Inner-tar reader tests: links, types, names, duplicates.

## fn read_inner_metadata_refuses_every_non_regular_member
- `#500` (S0 cells i1-i6, meta-* oracle rows): symlink (also absolute and looping), hard link (also to a missing target), FIFO and both device kinds are all refused as `Invalid` with Portage's `Metadata member is not a regular file: <name>` even when the link target exists. Portage: `gpkg._check_metadata_files`.
## fn read_inner_metadata_refuses_directory_members
- The committed fixture's inner `metadata/` directory member and a nested one are directories: `Metadata member is not a regular file` (`tarfile` strips the trailing slash). S0 cell i8: a name outside `metadata/` is still Portage's `InvalidBinaryPackageFormat`.
## fn read_inner_metadata_reads_nested_keys_without_a_directory
- S0 cell i7b: Portage's key is the remainder after `metadata/`.

## fn read_inner_metadata_last_wins_and_flags_duplicate_keys
- S0 cell i11: Portage's dict comprehension keeps the last member with a name; S3 refuses the archive on the flagged duplicate.

## fn read_inner_metadata_rejects_traversal_and_absolute_names
- S0 cells i12/i12b: Portage's read path would produce a `../KEY` key and its merge path rejects both; portuale refuses both everywhere.

## fn read_inner_metadata_refuses_unknown_entry_types
- `TarInfo.isreg()` is false for a type byte `tarfile` does not know, so `_check_metadata_files` refuses it (the old reader read it as a regular file).
## fn read_inner_metadata_decompresses_through_the_pipe
- A failing decompressor is still an error (the old path reported zstd's own `({status})`).
