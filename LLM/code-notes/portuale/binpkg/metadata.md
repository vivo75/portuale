# crates/portuale/src/binpkg/metadata.rs
Reading gpkg and xpak metadata for the `$PKGDIR` scan.

## fn read_gpkg_metadata
- Ports Portage's `portage.gpkg.gpkg.get_metadata()` / `unpack_metadata(want=None)` (`lib/portage/gpkg.py:838-870`), narrowed to the local metadata read. Values have the same shape as a vdb aux file / `read_md5_cache` entry.
- The `gpkg-1` version marker's presence is Portage's `_get_inner_tarinfo` `InvalidBinaryPackageFormat` guard.
- Symlink discipline history: the outer member check comes from the tar header (not an extracted `symlink_metadata`), and the inner `metadata.tar` is read in process with the `tar` crate, porting Portage's `tarfile.extractfile` (symlink/hardlink resolves inside the archive or errors; directory/FIFO/device members and names outside `metadata/` error). Before this, the extracted tree was walked with `Path::is_file`/`fs::read`, so a symlinked prefix listed the host `/etc`, a symlinked `Manifest` read a host file, and an inner `metadata/DESCRIPTION -> /etc/hostname` symlink returned the host file's bytes (see `TEST/findings/l2.md`).
- Deliberate cut: no GPG `.sig` check on this populate path. A container that carries `.sig` members still has its cleartext `DATA` digests verified at merge time (what an unverified read amounts to is Portage's own `binpkg-ignore-signature` behaviour); Portage's `unpack_metadata`/`get_metadata` verify (`_verify_binpkg(metadata_only=True)` still checks the metadata `.sig` + Manifest), so a corrupt-or-foreign-signed binpkg resolves in portuale's pool but fails at merge. The resolve side stays deterministic and `gpg`-free on purpose.

## fn read_xpak_metadata
- Ports Portage's `portage.xpak` `.tbz2` reader (`tbz2.scan` + `getindex_mem`/`searchindex`, `lib/portage/xpak.py:395-460` / `234-266`). Values carry a trailing newline like a vdb aux file. Codec-agnostic: the trailer is raw, whatever compressor produced the tarball.

## fn read_xpak_segment
- Ports Portage's `tbz2.scan`. Every xpak read error is `Fatal`: Portage's `tbz2.scan()` swallows the failure and returns falsy, `get_data()` then returns `None`, and the caller raises `AttributeError` on `None.get`.
