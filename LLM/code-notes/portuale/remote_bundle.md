# crates/portuale/src/remote_bundle.rs
Remote-merge wire bundles: one binpkg prepared for stdin streaming.

## module
- Slice 2 (`docs/remote-merge.md` §13).
- A bundle is the extracted `image/` + `build-info/` (via `binpkg::extract_binpkg`, the same extraction the local merge runs), a server-decompressed `environment` file (the `bzip2 -dc` step `run_phase_from_saved_env` runs locally — the client never needs `bzip2`), and a `remote-manifest` the client sanity-checks before unpacking.
- The `bin/` dir also ships the `bzip2-passthrough` `${PORTAGE_BZIP2_COMMAND}` stand-in (backlog #171) for the env-regeneration postinst run.
- Tarred uncompressed (`tar -cf`, system tar like `binpkg::run_tar` — wire compression is a future slice).

## struct StagedBundle (fields `eapi`/`category`/`pn`/...)
- Server-side `_pkgsplit` values, re-exported by the phase driver: the binary-branch load filter strips `CATEGORY/PVR/PF/PN/PR/PV/P` from the saved environment (see `portage_repo::split_pf`), so — like local `phase_env_vars` — these travel outside the file.
- The byte count the client must observe is a `wc -c` equality gate.

## struct StagedBundle (fields `phases`, `postinst_defined`)
- The local `merge_binpkg` degrade, mirrored: only when both the ebuild file and the hook environment shipped. `postinst_defined` is non-fatal on failure, like the local merge.

## const BZIP2_PASSTHROUGH
- Backlog #171: the client's `${PORTAGE_BZIP2_COMMAND}` stand-in for the merge-time environment-regeneration postinst run. Portage's `phase-functions.sh` `PORTAGE_UPDATE_ENV` block runs `${PORTAGE_BZIP2_COMMAND} -c -f9 > "${PORTAGE_UPDATE_ENV}"` (`bin/phase-functions.sh:1141-1153`), and the client must have no `bzip2` (`docs/remote-merge.md` §6, §14.1) — so this ignores its arguments and copies stdin to stdout (`cat` itself would choke on `-c -f9`), leaving the regen'd environment plain text for the server to compress. Only ever invoked through that one block: nothing else in the phase run resolves through it.

## fn copy_bin_runtime
- `bin_dir()` may be a symlink overlay of absolute links into the checkout (`build_bin_overlay`); a client on another host would get dangling links from a plain `cp -a` (l31 hid this: its client is the same container with the repo mounted at the same path).

## fn split_pvr
- `PR` defaults to Portage's own `"r0"`.

## fn read_eapi
- PMS 7.3.1 rule. Fail-early: Portage binpkgs always carry EAPI in `build-info`.

## struct FileMeta
- The client joins on the path to write CONTENTS lines and make protect decisions — it never hashes (no md5sum on the client) and only stats for the mtime check on paths the *old* version owned.
- The client reproduces the mtime with `cp -p` / `touch -h -r`, best-effort.

## fn collect_filemeta
- Symlink targets read via `read_link`; anything else (fifo/socket/device) is an error — Portage binpkgs never carry those (block/char devices are v1 cuts documented in the merge modules, and the client could not create them portably anyway).

## fn select_phases
- Slice-3 client phases. Same gate as the local merge's `extracted_ebuild` + `phase_defined`.

## fn build_bundle
- `repo_override` is the resolver-known repo name (the resolve flow's `repo_position`); the `--remote-binpkg` trial path passes `None`, honestly reporting the bytes as-is.
- `install_mask` / `install_mask_prunes_usr_share` are the resolve's own `config_install_mask` values for the placed config (the client's under `ConfigPlacement::Client`, the server's under `ConfigPlacement::Server`; empty on the `--remote-binpkg` trial path, which resolves no config). The client's copy loop, CONTENTS and collision checks see the masked set exactly like Portage's `vartree.py:4581` `treewalk` (mask before collision-protect, before `pkg_preinst`), and `build-info/INSTALL_MASK` rides the bundle into the vdb verbatim.
- `gpg` is the same merge-time signature policy `merge_binpkg` runs (see `crate::binpkg::GpgVerify`) — the bundle stages exactly what the merge would see, verified the same way.
- The remote client merges through its own generated scripts (plan §6); the #334 xattr restore covers the local merge only.
- Backlog #170: BINPKGMD5. Portage `_emerge/Binpkg._start_task`: "Store the md5sum in the vdb." The same digest the local `merge_binpkg` records (whole binpkg file, `"{md5}\n"`), written into the staged `build-info/` so the client's verbatim `build-info/*` vdb copy lands it.
- Portage `dblink.treewalk()`'s `preinst_mask` (`vartree.py:4581`, `bin/misc-functions.sh:373-387`): the staged image is pruned before `collect_filemeta` so the client's copy loop, CONTENTS and shadow pre-check all see the masked set, and `build-info/INSTALL_MASK` rides the bundle into the vdb verbatim when the resolved mask is non-empty (#170). A no-op with an empty mask (the `--remote-binpkg` trial path, which resolves no config, masks nothing and writes no file).
- The ebuild runtime the client phases run under (`bash bin/ebuild.sh <phase>`). Per unit for now; a per-session ship is the obvious later optimization (note the ~500K in the trial logs if it ever matters).
- `0o755` on the passthrough must survive the tar below (`tar -cf` preserves modes).

## tests: bundle_stages_image_build_info_environment_and_manifest
- The tar lists exactly one top-level unit dir with the four expected members (image/, build-info/, remote-manifest; environment because this fixture carries environment.bz2).

## tests: bundle_repo_falls_back_to_resolver_override
- An explicit override of `"__unknown__"` (a vdb-less resolve) stays unknown.

## tests: bundle_writes_binpkgmd5_of_the_shipped_binpkg
- Backlog #170: the bundle carries `build-info/BINPKGMD5` = the md5 of the binpkg file being shipped + `\n` (33 B) — Portage `_emerge/Binpkg.py:374` `_start_task` ("Store the md5sum in the vdb"), the same digest the local `merge_binpkg` records. Portage's BINPKGMD5 is 33 B.

## tests: bundle_applies_install_mask_before_filemeta
- Backlog #170: with a non-empty resolved mask the bundle prunes the staged image *before* `collect_filemeta` (via the same `ebuild_merge::apply_install_mask` the local merge runs) and ships `build-info/INSTALL_MASK` = value + `\n` — Portage `vartree.py:4581` `treewalk` running `preinst_mask` (`bin/misc-functions.sh:373-387`) before collision-protect. The fixture image ships `usr/share/packagepkg/hello.txt`, so an anchored `/usr/share/packagepkg` mask must drop it from the image, from `filemeta`, and from the tarball's image listing.

## tests: bundle_empty_mask_prunes_nothing_and_writes_no_file
- Backlog #170: Portage's own `[[ -n ${x} ]] && echo … > INSTALL_MASK` gate in `preinst_mask`.

## tests: bundle_ships_bzip2_passthrough_for_regen
- Backlog #171: the bundle ships `bin/bzip2-passthrough` (the client's `${PORTAGE_BZIP2_COMMAND}` stand-in for the env-regen postinst run): executable, inside the tar, and ignoring compression flags while passing stdin through.

## tests: copy_bin_runtime_dereferences_symlinks
- #326 S8.5: the bundle ships Portage files, never an overlay's symlinks. A runtime dir holding an absolute symlink (the `build_bin_overlay` shape: absolute links into a checkout that a client on another host cannot resolve) copies as the target's bytes — a regular file, not a dangling link.
