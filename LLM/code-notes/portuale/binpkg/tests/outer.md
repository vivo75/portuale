# crates/portuale/src/binpkg/tests/outer.rs
Outer-member type-check tests: one crafted container per hostile member.

## module
- Backlog #56, S0 cells: each cell is one crafted hostile member. Cell a: a symlinked `Manifest` was followed onto the host (`fs::read` read `/etc/hostname`). Cell b: a symlinked `metadata.tar.zst` was followed by the decompressor. Cell c: a symlinked prefix made the walk list a host directory (`/etc`, 266 dirents); slice #58 S5 rejects the crafted member from its header instead. Cell d: a FIFO at `metadata.tar.zst` blocked the decompressor forever (which is why the check runs before any read). Cell f: a hardlink to another member extracts as a regular file (the walk cannot see the header type), so the type check passes and the pre-existing reading paths reject it -- the decompressor on the 0-byte link and the Manifest size check; no host bytes are read (S0 verdict). Cell h: a symlinked `gpkg-1` marker was trusted by name and, on the merge path, read `Manifest` through the link. Cell e (root only, like the device itself): a character device at `image.tar.zst` made the merge path read `/dev/zero` unboundedly.

## fn regular_outer_member_check_rejects_a_symlinked_metadata_member
- Cell c: a symlinked prefix made the walk list a host directory (`/etc`, 266 dirents). `#58` S5 rejects the crafted member from its header instead -- the prefix name itself is a top-level member (`pkg-1.0`, zero slashes), which Portage's own `f.count("/") != 1` structure check refuses before any type check, so all three sites reject it the same way.
