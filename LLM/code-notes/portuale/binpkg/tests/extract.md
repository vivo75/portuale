# crates/portuale/src/binpkg/tests/extract.rs
Binpkg extraction tests: xpak verbatim writes, gpkg image/metadata handling.

## fn xpak_xattr_tar_options_match_portages_extractor
- Backlog #334: ports Portage's `BinpkgExtractorAsync._xpak_start` tar options, as the argv its shell string becomes: none without `FEATURES=xattr` or without a tar that knows `--xattrs`.

## fn extract_binpkg_writes_xpak_members_verbatim_like_portage_unpackinfo
- Slice #326 S5 (L2 xpak control): ports Portage's `tbz2.unpackinfo` (`xpak.py:486-524`), which writes every XPAK member verbatim. An empty `DEBUGBUILD` stays empty, and the trailing space Portage's `NEEDED` keeps survives. A name that climbs out with `..` is skipped, a leading `/` is stripped, and a `sub/x` member gets its directory.

## fn extract_binpkg_unpacks_a_real_gpkg_image_and_build_info
- The inner `image/` and `metadata/` top-level dirs land directly, not one level deep (the staging move, like Portage's `extractall` move); members land as `usr/...`, not `/image/...`. A missing move is what made the merged vdb `CONTENTS` record every path as `/image/...`.
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
- `#514`: the relocated-relative-symlink needle is Portage's own `image/../../marker` (was the earlier `image/opt/link`); the relocated-out-of-place pair (accepted by Portage, oracle row `u-relocated-out-of-place`) moved to `extract_binpkg_accepts_contained_symlink_parents`.

## fn extract_binpkg_accepts_a_symlink_climbing_to_the_temp_root_like_portage
- `#514`: the bare symlink climbing exactly to Portage's temp root merges with its target byte-preserved (was already accepted by accident); a member under it passes the scan but GNU tar truncates it, so the run reports tar's failure with the image dir left empty. Oracle: `LLM/evidence/2026-10-23-500-501/514-oracle-matrix.txt` (`b-*` rows).

## fn extract_binpkg_reports_the_gnu_tar_truncation_for_a_dotdot_hardlink_shape
- `#514`: the hard link of an in-image `..` symlink with a member under it passes the scan (Portage extracts it, matrix row `c-dotdot-same-depth`) but GNU tar 1.35 defers the link and fails; the run reports tar's failure with the image dir left empty. The composed L5 pair is (1, 1): the control trips over CPython's post-hardlink stream bookkeeping instead (`LLM/portage-quirks.md` Q10).

## fn extract_binpkg_accepts_contained_symlink_parents
- `#500`: upstream `test_contained_symlink_parent`; the old lexical rule refused it (the `control-contained-symlink-parent` oracle row). Members of the test have no directory entries because the `tar` crate writes them mode 0644, which GNU `tar` cannot traverse.
- `#514`: also covers the relocated-out-of-place hard link pair (`image/opt/deep/link -> ../x` + hard link, no member under), which Portage accepts (oracle row `u-relocated-out-of-place`); the old root refused it.

## fn extract_binpkg_keeps_a_valid_sparse_xattr_image_working
- S0 cell i19, the regression shape: legitimate symlink, hardlink-to-earlier, GNU sparse file, PAX xattr records must still extract exactly as before S4 (the pre-scan only rejects).
- GNU `tar -xpf` (no `--xattrs`) ignores the PAX xattr records, exactly as before the pre-scan and as Portage's own `decompress` (`TEST/findings/l2.md` "## #58 S0" cell i19).
