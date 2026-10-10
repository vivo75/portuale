# crates/portuale/src/remote/tests/preflight.rs
Preflight-gate, unpack-driver and merge-driver behaviour tests.

## tests: preflight_gates_catch_old_bash_missing_tools_and_roots
- A missing vdb warns: the slice-5 placement matrix owns the degrade.

## tests: remote_merge_masks_image_and_lands_aux_files_in_vdb
- Backlog #170: end to end through `MERGE_FLOW`. Portage `_emerge/Binpkg.py:374` (`BINPKGMD5`) and `vartree.py:4581` `preinst_mask` (`INSTALL_MASK`). The fixture image ships `usr/share/packagepkg/hello.txt`, so the anchored `/usr/share/packagepkg` mask stands in for the bed's `/usr/share/porttest/im/...` mask. The merge-time aux files land in `var/db/pkg/<cat>/<pf>/`.

## tests: unpack_then_merge_preserves_special_mode_bits
- Backlog #169, site (b): an unprivileged `tar -xf` applies the umask and strips all three bits at extraction, `tar -xpf` (`--preserve-permissions`) keeps them, so the merge stage never sees them otherwise (the bed's non-root client capture, `4711/2755/1750 -> 711/755/750`). Fails while unpack uses `tar -xf`; as non-root the merge's chown step is skipped, so this isolates the extraction site.
