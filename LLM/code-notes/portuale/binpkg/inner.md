# crates/portuale/src/binpkg/inner.rs
The gpkg inner `metadata.tar`: limits, members, links.

## const MAX_INNER_METADATA_BYTES
- `#58` K0: the inner `metadata.tar` is read in process with the `tar` crate. 64 MiB on the total member bytes (the bound still limits a decompression bomb). Portage reads the whole tar into memory too (`unpack_metadata`'s `io.BytesIO(metadata_reader.read())`); 64 MiB is far above any genuine `metadata.tar` (a few KiB-MiB).

## const MAX_INNER_METADATA_ENTRIES
- Portage's writer emits ~15 members.

## const MAX_INNER_LINK_DEPTH
- Portage's `tarfile.extractfile` follows links recursively until Python's recursion limit (`RecursionError` on a cycle). Portuale stops at a fixed depth with an error naming the member (`#58` K1).

## fn known_inner_entry_type
- The entry types Python's `tarfile` lists in `SUPPORTED_TYPES` (`lib/python3.14/tarfile.py`); any other byte is treated as a regular file by `extractfile` (`tarinfo.isreg() or tarinfo.type not in SUPPORTED_TYPES`).

## fn inner_entry_type_phrase
- Phrases Portage's own `None.read()` error cases, for the error message (`#58` K6).

## struct InnerMember::mode
- The merge path masks the header mode to `0o7777` and strips setuid/setgid before writing (`#58` K2).

## struct InnerMetadata
- `#58` S1: the resolved inner metadata. `entries` is first-appearance order with last-wins values (Portage's own `dict` comprehension over `getmembers()`, `gpkg.py:855-862`); `duplicate_keys` lets the merge path refuse an archive Portage's own `tar_safe_extract` rejects as "Duplicate files detected". The read path is last-wins like Portage; `#58` S3 refuses duplicates on the merge path (read only by S3 and the unit tests at first).

## fn collect_inner_members
- `#58` K3/K5: a name outside `metadata/`, an absolute name, or any `..` component is an error; the zip-bomb guards cap the member count and the total bytes. Name rules port Portage's `_strip_metadata_prefix` / `tar_safe_extract`.
- An inner tar that will not parse: Portage's `tarfile.open(mode="r:", fileobj=metadata_tar)` raises `ReadError` uncaught (Fatal).
- The trailing drain exists because when the reader is a decompressor's stdout pipe, dropping it early would send the child SIGPIPE (`#58` S5; the `tar` crate stops at the end-of-archive marker without reading the trailing zero padding, so the stream is drained to EOF so the child can exit cleanly). A drain failure wraps Portage's whole-stream `metadata_reader.read()` errors as `CompressorOperationFailed` (skip class).

## fn resolve_inner_member
- Symlink search is the whole archive, last match wins (Portage's `_getmember` bottom-to-top); hardlinks see only earlier members. Portage's `extractfile` returning `None` for a directory/FIFO/device becomes an `AttributeError` on `None.read()` (Fatal); an unresolved target is Portage's `_find_link_target` `KeyError` (Fatal).

## fn decompress_member_to_file
- `#58` S5: streams one outer member (`outer_member_read`) into `dest`, decompressing when `comp` names a codec. The member stream is either copied straight through (a plain `metadata.tar`/`image.tar`) or fed to the `gpkg_compressions()` decompressor's stdin with `dest` as its stdout. Streams one outer member into `dest` with no scratch copy of the compressed member and no second decompression pass.

## fn read_inner_metadata
- `#58` S1: Portage `tarfile.extractfile` over the inner `metadata.tar`. The compressed member is small (`member_bytes` is already read; `#58` S5), so it is piped through the matching decompressor (`gpkg_compressions`) straight into the `tar` crate; it is read in memory instead of a scratch file and capped; no `<scratch>/metadata.tar`, no second `tar` process.
- A missing decompressor is Portage's `CompressorNotFound` (`PortagePackageException`, skip class); a failed decompressor is Portage's `CompressorOperationFailed`.
