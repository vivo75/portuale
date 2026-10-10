# crates/portuale/src/binpkg/tests/inner.rs
Inner-tar reader tests: links, types, names, duplicates.

## fn read_inner_metadata_resolves_symlinks_in_archive
- S0 cell i2 plus a `../`-normalized target and a link chain: Portage joins `dirname(name)/linkname` and normpaths it, and recurses through further links.

## fn read_inner_metadata_rejects_a_symlink_out_of_the_archive
- S0 cell i1: Portage raises `KeyError: linkname ... not found`; the host file must never be read (the old walk followed it).

## fn read_inner_metadata_resolves_hardlinks_to_earlier_members
- S0 cell i3: the target has to be earlier than the link.

## fn read_inner_metadata_rejects_hardlinks_to_later_or_missing_members
- S0 cell i3b: target comes later. S0 cell i4: target is not a member at all.

## fn read_inner_metadata_rejects_fifo_and_device_members
- S0 cells i5/i6: Portage's `extractfile` returns `None`, which errors; the type is named.

## fn read_inner_metadata_rejects_directory_members_and_outside_names
- The committed fixture's inner `metadata/` directory member: Python tarfile strips the trailing slash, and Portage's `_strip_metadata_prefix` rejects the bare `metadata` name.
- S0 cell i7: a nested directory member is a `None.read()` error for Portage; the `sub/KEY` member after it never resolves. S0 cell i8: a name outside `metadata/` (Portage's `InvalidBinaryPackageFormat`).

## fn read_inner_metadata_reads_nested_keys_without_a_directory
- S0 cell i7b: Portage's key is the remainder after `metadata/`.

## fn read_inner_metadata_errors_on_symlink_cycles
- S0 cell i10: Portage hits Python's `RecursionError`; portuale stops at its own depth cap with a named error.

## fn read_inner_metadata_last_wins_and_flags_duplicate_keys
- S0 cell i11: Portage's dict comprehension keeps the last member with a name; S3 refuses the archive on the flagged duplicate.

## fn read_inner_metadata_rejects_traversal_and_absolute_names
- S0 cells i12/i12b: Portage's read path would produce a `../KEY` key and its merge path rejects both; portuale refuses both everywhere.

## fn read_inner_metadata_reads_unknown_entry_types_as_regular_files
- Portage: `tarinfo.isreg() or tarinfo.type not in SUPPORTED_TYPES` reads bytes.
