# crates/portuale/src/binpkg/tests/mod.rs
Shared binpkg test helpers: XPAK/gpgk builders, GPG keyring, inner-tar craft.

## fn xpak_u32
- `usize -> u32` narrowing that panics like Python's `struct.pack(">I", n)` instead of silently wrapping; the fixture-making sizes are far below `u32::MAX`, so it only fires when a test is corrupted by mistake.

## fn make_xpak_binpkg
- Builds an XPAK segment in Portage's `xpak.xpak_mem` layout and appends it to prefix bytes, exactly the way a `.tbz2` is `[tarball][XPAK trailer]`.

## fn build_gpkg
- Assembles a plain-outer-tar gpkg container from `<prefix>/<name>` members plus a `Manifest` body, in gpkg member order.

## enum GpkgEntry / fn build_gpkg_entries
- One member of a backlog #56 crafted test container. `Hardlink` is an `fs::hard_link` to an earlier member, which GNU `tar` records as `LNKTYPE`.

## fn assert_rejects_everywhere
- Slice #56 S1: every one of the three outer-container sites must reject `g`, and every error must contain each of `needles` (the member name and, for the type checks, its type).

## GPG helpers
- Portage's committed GnuPG test keyring, copied verbatim into pmtest (`fixtures/helpers/gpg-keyring/`, from `lib/portage/tests/.gnupg` -- the same keys Portage's own `test_gpkg_gpg.py` signs with; slice #326 S4, so these tests run with no Portage checkout): trusted `0x5D90EA06352177F6`, untrusted `0x8812797DDF1DD192`, both passphrase `GentooTest`. Neither key expires, so a container signed with them verifies deterministically.
- `test_gpg_home`: a writable copy under a fresh tempdir because `gpg` refuses a homedir it doesn't own and signs/verifies with lock files inside it; `chmod 700` is Portage's own requirement. `kill_gpg_agent`: a signing `gpg` daemonizes its agent, which would otherwise outlive the test session (Portage's own `conftest.py` does the same).
- `gpg_sign`: clear-signs `data` exactly the way Portage's `checksum_helper(SIGNING, detached=False)` does for the `Manifest` (`gpkg.py:1552-1560`, `gpg --clearsign` over stdin); detached-signs the way Portage signs each member (`--detach-sig`, `gpkg.py:1031-1058`).

## fn repack_gpkg_with_mutation
- Unpacks the outer tar, lets `mutate` alter the tree, and repacks. Lists the prefix's files rather than the prefix directory itself because GNU tar would also write a `<prefix>/` directory member, which Portage's one-level structure check rejects (slice #58 S5; the old filesystem walk ignored directory entries).

## inner-tar craft (S0/S1 cells)
- Slice #58 S1: the in-process inner `metadata.tar` reader. `InnerTestEntry` carries the same raw fields the crafted S0 cells set, written through `tar::Builder`; `Header::set_path` refuses `..`/absolute names, so raw name bytes are written, exactly what the crafted cells need.
- Slice #58 S2 (`build_gpkg_with_inner_entries`): a valid outer gpkg whose inner `metadata.tar` carries the entries and whose Manifest is recomputed around the member bytes, so `verify_gpkg_manifest` passes and a `read_gpkg_metadata` error can only come from the inner check. S3/S4 image side (`build_gpkg_with_inner_tars`, `build_gpkg_with_tar_bytes`): the sparse/xattr image comes from GNU `tar`, not `tar::Builder`.
- Slice #58 S4 (`build_gnu_sparse_image`): stages the S0 cell i19 shape (symlink, hardlink-to-earlier, sparse file, PAX xattr records).
