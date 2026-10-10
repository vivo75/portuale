# crates/portuale/src/binpkg/tests/digest.rs
Binpkg digest-verification tests: structure guards, xattr restore, index checks.

## fn read_gpkg_metadata_rejects_a_non_gpkg_tar
- A plain tar with a top-level `a.txt`: Portage's one-level structure check (`f.count("/") != 1`) rejects it before any marker lookup (slice #58 S5; the old filesystem walk never saw the raw member name).
- A structurally valid container with no `gpkg-1` marker is Portage's `_get_inner_tarinfo` guard.

## fn extract_binpkg_restores_xpak_xattrs_under_features_xattr
- Backlog #334 end to end: an xpak whose image tarball was packed with `tar --xattrs` (Portage's `__dyn_package` under `FEATURES=xattr`) restores the attributes on extraction under `xattr`, minus `PORTAGE_XATTR_EXCLUDE`, and none without it. Portage does the same in the bed image: pmtest `differential-test-bed/scripts/329-xattr.sh <pm> [-xattr] binpkg`.

## fn verify_binpkg_against_index_reports_size_then_digest_like_portage
- Ports Portage's `_emerge/BinpkgVerifier`: the stat size is checked against the index `SIZE` first, then each digest the record carries; the failure triple is `(name, got, expected)` with `name == "size"` for the size check.
- No SIZE is Portage's early OK. Size mismatch beats everything, even a correct digest.
- The failure block is Portage's `BinpkgVerifier._digest_exception` shape: leading blank line, three `!!!` lines, then the bare `File renamed to` line (no `!!!` prefix).

## fn checksum_failure_rename_moves_to_a_checksum_failure_sibling
- Ports Portage's `_checksum_failure_temp_file` (`fetch.py:692-696`, i.e. `tempfile.mkstemp("", <basename> + "._checksum_failure_.", dir)`); `<rand>` is Portage's `tempfile._RandomNameSequence` shape, 8 characters over `[a-z0-9_]` (the bed oracle shows `._checksum_failure_.4b2g93ai`). A second identical failure reuses the existing sibling (same size + md5).
