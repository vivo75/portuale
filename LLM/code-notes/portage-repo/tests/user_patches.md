# crates/portage-repo/src/tests/user_patches.rs
`--binpkg-respect-user-patches` unit pins: the mask reason, the block renderer, and the elimination port.

## module (#509)
- Unit pins for the `--binpkg-respect-user-patches` filter, the `user patches` mask reason, the ignored-binaries block renderer and the `_eliminate_ignored_binaries` port. The black-box shapes live in the oracle (`LLM/evidence/2026-10-23-509/`) and the pmtest `test_509_*` pins.

## fn ignored_block_renders_exactly_like_portage
- Oracle `LLM/evidence/2026-10-23-509/captures/02-differ-k.*` (non-verbose) and `11-verbose-k.*` (verbose tail).

## fn elimination_keeps_a_same_version_ebuild_row_but_drops_newer_selections
- Portage `_eliminate_ignored_binaries` (upstream `a7c346aa4`).
