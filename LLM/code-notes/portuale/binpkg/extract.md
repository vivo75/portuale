# crates/portuale/src/binpkg/extract.rs
Extracting a binary package's image and metadata at merge time.

## fn extract_binpkg
- Ports Portage's binary-package unpack (`portage.xpak.tbz2.decompose` / `portage.gpkg.gpkg.decompress` + `_generate_metadata_from_dir` in reverse); scalar metadata has the `build-info` shape.
- xpak image: the leading `file_len - (infosize + 8)` bytes. Ports `bintree.dbapi.unpack_metadata` -> `tbz2.unpackinfo` (`xpak.py:486-524`): every XPAK member written verbatim. The verbatim write fixed two bugs: an empty `DEBUGBUILD` became `"\n"`, and the trailing space Portage keeps on the last `NEEDED` line was trimmed (the gpkg branch had the same bug class).
- The merge runs `pkg_preinst`/`pkg_postinst` by `bunzip2`'ing `environment.bz2` into `${T}/environment` (Portage's `BinpkgEnvExtractor` -> `bin/ebuild.sh`'s saved-env source path); Portage keeps both non-scalar members in the vdb.

## fn extract_gpkg_metadata
- Ports `bintree.dbapi.unpack_metadata` -> `gpkg().unpack_metadata(dest_dir)`. Portage would extract an in-archive symlink as a symlink, which every later build-info reader (`populate_vdb_tmp`, `BINPKGMD5`, the saved-`environment.bz2` extractor, the `pkg_*` phases) would then follow; Portage's writer never emits one. Nested keys would need directory creation Portage's writer never necessitates.

## fn scan_image_tar
- Ports Portage's `gpkg.tar_safe_extract` member rules (`gpkg.py:690-724`; validated inside `gpkg.decompress`, `gpkg.py:1071-1103`, but only as it extracts). `isdev()` in Python 3.14 includes FIFOs; symlinks are allowed (a genuine image carries them legitimately).
- One deliberate addition beyond Portage's rule set (owner-approved, from the S0 cell): a member whose path traverses an earlier symlink member is refused. Portage's `tar_safe_extract` would extract through that symlink (as root straight into the host) while GNU `tar` refuses at unpack time; refusing before unpacking makes the guarantee independent of the unpacker.

## fn xpak_xattr_tar_options
- Under `FEATURES=xattr` the xpak itself was packed with `gtar --xattrs` under the same feature (`misc-functions.sh:587-590`). Portage builds a shell string, so its quoted `'*'` reaches tar as the plain word `*`; this argv is what that shell hands tar.

## fn extract_gpkg_member
- The inner tarball's strip: the members all live under a single `<want>/` top-level directory (Portage's `gpkg._add_data`: `image_tar.add(root_dir, "image", recursive=True)`); Portage extracts to a temp dir and moves `<want>/*` into `dest`. `--strip-components=1` is the same strip.
