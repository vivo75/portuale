# crates/portuale/src/binpkg/outer.rs
The gpkg outer-tar reader: member enumeration and single-member streaming.

## fn trusted_outer_member_name
- The trusted set (backlog #56): the `gpkg-1` version marker, the two inner tars (any compression), their `.sig` sidecars, and `Manifest`.
- The non-regular check runs everywhere a container is read, not only where the member is selected, so a crafted symlink can never be skipped in favour of a later regular one.

## struct OuterMember
- Slice #58 S5: the member as its tar header presents it. `name` is the basename after the single prefix directory (the name Portage's `Manifest` records and #56 trusts); `raw_name` is the full member name bytes reported for re-opening via `outer_member_read`.

## fn read_outer_members
- Slice #58 S5: reads the outer container in process with the `tar` crate. Portage's `_verify_binpkg` structural rules become header checks: every name relative, exactly one level deep under one common prefix (`f.count("/") == 1` plus `os.path.commonpath`), no duplicate names.
- The #56 trusted names must be a regular member, judged from the tar header rather than a `symlink_metadata` after `tar -xf`: a symlink/FIFO/device/hardlink header at `gpkg-1`, `Manifest`, an inner tar or a `.sig` sidecar errors naming the member and its type, and its data is never read through it.
- Python `tarfile` strips a trailing `/` before Portage sees the name, so a directory member is a depth error exactly as Portage's `f.count("/") != 1` check says.

## fn outer_member_read
- Slice #58 S5: streams one member through `consume` without materialising it (a `verify_gpkg_manifest` pass hashes members that can be hundreds of MiB). The container is opened fresh per read -- Portage's `container.extractfile` does the same, and re-opening beats buffering a member.
