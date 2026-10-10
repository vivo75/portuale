# crates/portuale/src/binpkg/inner.rs
The gpkg inner `metadata.tar`: limits, members, links.

## const MAX_INNER_METADATA_BYTES
- 64 MiB on the total member bytes. Portage reads the whole tar into memory too (`unpack_metadata`'s `io.BytesIO(metadata_reader.read())`); 64 MiB is far above any genuine `metadata.tar` (a few KiB-MiB).

## const MAX_INNER_METADATA_ENTRIES
- Portage's writer emits ~15 members.

## const MAX_INNER_LINK_DEPTH
- Portage's `tarfile.extractfile` follows links recursively until Python's recursion limit (`RecursionError` on a cycle).

## fn known_inner_entry_type
- The entry types Python's `tarfile` lists in `SUPPORTED_TYPES` (`lib/python3.14/tarfile.py`); any other byte is treated as a regular file by `extractfile` (`tarinfo.isreg() or tarinfo.type not in SUPPORTED_TYPES`).

## fn inner_entry_type_phrase
- Phrases Portage's own `None.read()` error cases, for the error message.

## struct InnerMember::mode
- The merge path masks the header mode to `0o7777` and strips setuid/setgid before writing.

## struct InnerMetadata
- `entries` is first-appearance order with last-wins values (Portage's own `dict` comprehension over `getmembers()`, `gpkg.py:855-862`); `duplicate_keys` lets the merge path refuse an archive Portage's own `tar_safe_extract` rejects as "Duplicate files detected". The read path is last-wins like Portage.

## fn collect_inner_members
- Name rules port Portage's `_strip_metadata_prefix` / `tar_safe_extract`.
- An inner tar that will not parse: Portage's `tarfile.open(mode="r:", fileobj=metadata_tar)` raises `ReadError` uncaught (Fatal).
- The trailing drain exists because when the reader is a decompressor's stdout pipe, dropping it early would send the child SIGPIPE. A drain failure wraps Portage's whole-stream `metadata_reader.read()` errors as `CompressorOperationFailed` (skip class).

## fn resolve_inner_member
- Symlink search is the whole archive, last match wins (Portage's `_getmember` bottom-to-top); hardlinks see only earlier members. Portage's `extractfile` returning `None` for a directory/FIFO/device becomes an `AttributeError` on `None.read()` (Fatal); an unresolved target is Portage's `_find_link_target` `KeyError` (Fatal).

## fn decompress_member_to_file
- Streams one outer member into `dest` with no scratch copy of the compressed member and no second decompression pass.

## fn read_inner_metadata
- The compressed member is small, so it is read in memory instead of a scratch file and capped; no `<scratch>/metadata.tar`, no second `tar` process.
- A missing decompressor is Portage's `CompressorNotFound` (`PortagePackageException`, skip class); a failed decompressor is Portage's `CompressorOperationFailed`.
