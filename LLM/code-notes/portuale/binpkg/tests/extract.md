# crates/portuale/src/binpkg/tests/extract.rs
Binpkg extraction tests: xpak verbatim writes, gpkg image/metadata handling.

## fn xpak_xattr_tar_options_match_portages_extractor
- Backlog #334: ports Portage's `BinpkgExtractorAsync._xpak_start` tar options, as the argv its shell string becomes: none without `FEATURES=xattr` or without a tar that knows `--xattrs`.

## fn extract_binpkg_writes_xpak_members_verbatim_like_portage_unpackinfo
- Slice #326 S5 (L2 xpak control): ports Portage's `tbz2.unpackinfo` (`xpak.py:486-524`), which writes every XPAK member verbatim. An empty `DEBUGBUILD` stays empty, and the trailing space Portage's `NEEDED` keeps survives. A name that climbs out with `..` is skipped, a leading `/` is stripped, and a `sub/x` member gets its directory.

## fn extract_binpkg_unpacks_a_real_gpkg_image_and_build_info
- The inner `image/` and `metadata/` top-level dirs that Portage's `tar_safe_extract` strips are stripped here too; members land directly, not one level deep. A missing strip is what made the merged vdb `CONTENTS` record every path as `/image/...`.
- Slice #58 S3: the metadata side writes the resolved member bytes verbatim as regular files with the header mode (0o644 in the fixture), the same bytes and modes the former `tar -xpf` left (captured on `main`).

## fn extract_binpkg_refuses_an_inner_metadata_symlink_and_writes_nothing
- `#500`: Portage refuses a metadata link whatever it points at; neither the image nor build-info is written (both halves are validated first). `pretend.rs::run_info` calls extract_binpkg with no prior read, so this path is what protects it.
## fn extract_binpkg_rejects_unsafe_inner_metadata_archives
- S0 cell i1: a metadata symlink is refused (the old `tar -xpf` extracted the symlink itself into build-info, where later readers follow it); since `#500` the text is Portage's `Metadata member is not a regular file`.
- S0 cell i5: Portage's 3.14 `isdev()` rejects a FIFO on merge too; the old `tar -xpf` created one in build-info.
- S0 cells i7/i7b: nested keys (K4). S0 cell i11: duplicates (Portage's `tar_safe_extract`).

## fn extract_binpkg_keeps_extracting_image_symlinks
- K2 does not touch the image until the S4 pre-scan: a legitimate relative symlink must still land as a symlink (Portage extracts image symlinks as-is; only devices/dups/abs/traversal/escaping hardlinks are rejected).

## fn extract_binpkg_rejects_unsafe_image_tars_before_unpacking
- The pre-scan runs before the unpack: the image dest stays empty (a FIFO/device member never reaches the filesystem).
- S0 cells i13/i13b: a char device (Portage's `isdev()`; 3.14 includes FIFO). S0 cells i14/i15: a hard link to a non-member or a later member. S0 cell i16: duplicate names (GNU tar used to last-wins silently). S0 cells i17/i17b: traversal / absolute member names. S0 cell i18: a member under an earlier symlink member (the owner-approved rule beyond Portage's own set).

## fn extract_binpkg_refuses_the_portage_hostile_image_shapes
- `#500`: one case per shape of the 3.0.83-r1 oracle table (`LLM/evidence/2026-10-23-500-501/README.md`): `a/..`, bare `..`, `./..`, directory through a symlink, symlink parent (absolute, relative, to `/`, through a chain, a loop), hard link to an escaping symlink, relocated relative symlink. Each asserts the `Danger:` text and that neither the image nor build-info directory holds anything.

## fn extract_binpkg_accepts_contained_symlink_parents
- `#500`: upstream `test_contained_symlink_parent`; the old lexical rule refused it (the `control-contained-symlink-parent` oracle row). Members of the test have no directory entries because the `tar` crate writes them mode 0644, which GNU `tar` cannot traverse.

## fn extract_binpkg_keeps_a_valid_sparse_xattr_image_working
- S0 cell i19, the regression shape: legitimate symlink, hardlink-to-earlier, GNU sparse file, PAX xattr records must still extract exactly as before S4 (the pre-scan only rejects).
- GNU `tar -xpf` (no `--xattrs`) ignores the PAX xattr records, exactly as before the pre-scan and as Portage's own `decompress` (`TEST/findings/l2.md` "## #58 S0" cell i19).
