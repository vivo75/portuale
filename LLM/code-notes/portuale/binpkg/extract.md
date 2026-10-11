# crates/portuale/src/binpkg/extract.rs
Extracting a binary package's image and metadata at merge time.

## fn extract_binpkg
- Ports Portage's binary-package unpack (`portage.xpak.tbz2.decompose` / `portage.gpkg.gpkg.decompress` + `_generate_metadata_from_dir` in reverse); scalar metadata has the `build-info` shape.
- The scalar metadata is `read_{xpak,gpkg}_metadata`'s own map; the two non-scalar members (`environment.bz2` -- the package's saved build-time bash environment -- and `<pf>.ebuild`) are written back verbatim as raw bytes, not through the lossy scalar path.
- xpak image: the leading `file_len - (infosize + 8)` bytes. Ports `bintree.dbapi.unpack_metadata` -> `tbz2.unpackinfo` (`xpak.py:486-524`): every XPAK member written verbatim. The verbatim write fixed two bugs: an empty `DEBUGBUILD` became `"\n"`, and the trailing space Portage keeps on the last `NEEDED` line was trimmed (the gpkg branch had the same bug class). Found by the L2 xpak control (#326 S5): every XPAK member is written VERBATIM, not as a scalar parse plus reserialize, the same bug the gpkg branch fixed.
- The merge runs `pkg_preinst`/`pkg_postinst` by `bunzip2`'ing `environment.bz2` into `${T}/environment` (Portage's `BinpkgEnvExtractor` -> `bin/ebuild.sh`'s saved-env source path); Portage keeps both non-scalar members in the vdb.

## fn xpak_member_dest
- Portage `unpackinfo`'s target path for one member: the name with leading `/` stripped, joined under `dest` and normalized. A name whose `..` components climb out of `dest` is skipped (`None`), as Portage's `if not filename.startswith(mydest): continue` does.

## fn extract_gpkg_metadata
- `#58` K2: the metadata half of `extract_gpkg_member` -- Portage `bintree.dbapi.unpack_metadata` -> `gpkg().unpack_metadata(dest_dir)` extracts every inner `metadata/` member into build-info. Portuale resolves the S1 reader and writes each member as a regular file with its header mode (masked to `0o7777`, setuid/setgid stripped).
- `OpenOptions::mode` passes through the process umask; Portage's `tarfile.extract` chmods the header mode on afterwards, so the mode is set explicitly for a umask-independent result.
- Ports `bintree.dbapi.unpack_metadata` -> `gpkg().unpack_metadata(dest_dir)`. Portage would extract an in-archive symlink as a symlink, which every later build-info reader (`populate_vdb_tmp`, `BINPKGMD5`, the saved-`environment.bz2` extractor, the `pkg_*` phases) would then follow; Portage's writer never emits one. Nested keys would need directory creation Portage's writer never necessitates.

## fn scan_image_tar
- `#58` K2 / S4: Portage `gpkg.tar_safe_extract`'s member rules, applied to the already-decompressed scratch `image.tar` before GNU `tar` unpacks it -- so nothing reaches the filesystem until the whole member list is accepted.
- Ports Portage's `gpkg.tar_safe_extract` member rules (`gpkg.py:690-724`; validated inside `gpkg.decompress`, `gpkg.py:1071-1103`, but only as it extracts). `isdev()` in Python 3.14 includes FIFOs; symlinks are allowed (a genuine image carries them legitimately).
- One deliberate addition beyond Portage's rule set (owner-approved, owner-approved 2026-09-16 from S0 cell i18): a member whose path traverses an earlier symlink member is refused. Portage's `tar_safe_extract` would extract through that symlink (as root straight into the host) while GNU `tar` refuses at unpack time; refusing before unpacking makes the guarantee independent of the unpacker.
- Portage `tar_safe_extract.file_list`, one entry per accepted member (trailing `/` trimmed the way Python `tarfile` trims it).

## fn xpak_xattr_tar_options
- Portage `BinpkgExtractorAsync._xpak_start` (`BinpkgExtractorAsync.py:39-50`, #334): under `FEATURES=xattr`, when the tar knows `--xattrs`, extract with `--xattrs --xattrs-include='*'` plus one `--xattrs-exclude=<pat>` per `PORTAGE_XATTR_EXCLUDE` pattern.
- Under `FEATURES=xattr` the xpak itself was packed with `gtar --xattrs` under the same feature (`misc-functions.sh:587-590`). Portage builds a shell string, so its quoted `'*'` reaches tar as the plain word `*`; this argv is what that shell hands tar.

## fn extract_gpkg_member
- The inner tarball's strip: the members all live under a single `<want>/` top-level directory (Portage's `gpkg._add_data`: `image_tar.add(root_dir, "image", recursive=True)`); Portage extracts to a temp dir and moves `<want>/*` into `dest`. `--strip-components=1` is the same strip.
- Shares the outer unpack, the `#56` trusted-name regular-file walk and the `gpkg-1` validity guard with `read_gpkg_metadata`.
- The `image` half keeps GNU `tar` (`-xpf --strip-components=1`) for its ownership/permission/xattr/sparse semantics, with Portage `tar_safe_extract`'s rules applied by a pre-scan first (`#58` K2, S4). The `metadata` half is resolved in process (`#58` S3, `extract_gpkg_metadata`).
- `#58` S5: the outer container is read in process -- the `#56` trusted-name regular-member check, the one-level structure and the `gpkg-1` marker all come from the tar headers before any bytes are streamed.
- `#58` K2/S3: the inner metadata tar is resolved in process and written as regular files only (no scratch copy, no `tar -xpf`).
- Portage `gpkg.decompress` validates the image stream with `tar_safe_extract` as it extracts (`gpkg.py:1094-1096`); portuale validates the whole scratch `image.tar` first (`#58` S4, `scan_image_tar`) so a rejected member never reaches the filesystem, then lets GNU `tar` do the actual unpack.
- Portage `gpkg.tar_safe_extract.extractall(dest)`: the format guarantees the one prefix component, so with `--strip-components=1` the members become `usr/...` / `environment.bz2` directly. Without it every path in the merged package's vdb `CONTENTS` was recorded as `/image/...`, which breaks unmerge (portuale's and Portage's).
