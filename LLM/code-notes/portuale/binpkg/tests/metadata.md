# crates/portuale/src/binpkg/tests/metadata.rs
Metadata reader tests: gpkg scalars, xpak index, error classes.

## fn read_gpkg_metadata_extracts_the_embedded_scalar_metadata
- A hand-built `.gpkg.tar` (GNU `tar` plus `zstd`): outer plain-tar container, `gpkg-1` marker, zstd-compressed inner `metadata.tar` with `metadata/<KEY>` files.

## fn binpkg_error_classes_mirror_portages_caller
- The two-class split Portage's `_populate_local` caller makes (`bintree.py:1185`; per-site table in `LLM/07.60-gpkg-populate-read-errors.opus.md` section 4). The inner-member cells' classes are asserted in their own tests; this pins the outer-container and xpak rows.
- A trusted outer member that is not a regular file: Portage's `_verify_binpkg` raises `InvalidBinaryPackageFormat`, i.e. Invalid. A container with no `metadata.tar` member: Portage's `_get_inner_tarinfo` raises the same. A file that is not a tar at all: Portage's `_verify_binpkg` catches `tarfile.ReadError` and re-raises `InvalidBinaryPackageFormat` (`getnames()`), so it is the skip class too.
- An xpak trailer pointing at a segment that is not an xpak: Portage's `tbz2.scan()` returns falsy, `get_data()` returns None, and `None.get` raises `AttributeError`, i.e. Fatal.

## fn read_xpak_metadata_reads_a_real_ebuild_package_tbz2
- Reads a genuine `.tbz2` checked in at `fixtures/pkgdir/dev-libs/packagepkg-1.0.tbz2`, built once by portuale's own `ebuild <file> package` on `dev-libs/packagepkg` (Portage's `bin/misc-functions.sh` -> unmodified `xpak-helper.py recompose` -> Portage's `xpak.py`). Kept as a committed fixture rather than rebuilt per-test: the read side doesn't need reproducible bytes, and driving `run_package` (the full brush phase chain) here would add parallel-load pressure to the suite's brush-heavy tests for no reader-coverage gain.
- The fixture ebuild's `RDEPEND="dev-libs/samepkg"` came through via Portage's `build-info` (`ebuild_phases::write_post_install_metadata`), no `Packages` index involved. The bundled `<pf>.ebuild` source is a member too. An xpak never carries CONTENTS (Portage's `xpak()` skips it; generated at merge time). `environment.bz2` is binary, lossy-decoded but present as a key.

## fn read_gpkg_metadata_refuses_inner_links_and_special_members
- `#500`: symlink (also to a host path), hard link and FIFO members are `Invalid` (`Metadata member is not a regular file`); S0 cell i1: the host file's bytes never appear in the message.
## fn read_gpkg_metadata_rejects_outside_names
- S0 cell i8: Portage's `InvalidBinaryPackageFormat`; the old walk ignored the member entirely.
## fn read_gpkg_metadata_reads_nested_keys_and_last_wins_duplicates
- S0 cell i7b: Portage's key is the remainder after `metadata/`; S0 cell i11: the last member with a name wins (the merge path refuses it).

## fn read_gpkg_metadata_reads_the_regenerated_fixture_without_a_directory_member
- The committed fixture was rebuilt (slice #58 S2) without the inner `metadata/` directory member Portage's writer never emits and its reader rejects (`TEST/findings/l2.md` "## #58 S0"); Portage's `get_metadata()` now reads 13 keys from it.
