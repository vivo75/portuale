# crates/portuale/src/remote/merge_flow.sh
The client-side merge driver for one remote unit (backlog #382: one unmerge path).

## flow remove-old (backlog #382)
- The old same-slot instance is retired by the shipped Rust binary (`$PORTUALE_BIN ebuild <staged-copy> unmerge`), never by shell: prerm, the orphan/CONFIG_PROTECT/`!md5` arms, `UNINSTALL_IGNORE`, postrm and the vdb delete all run through `run_unmerge` — the same code as direct `ebuild <file> unmerge` and the local replace loop. Owner decision Q4 (batch plan §6.4): the shell copy is deleted, not ported.
- Portage order: the new files merge first, then the old instance unmerges with its prerm (`vartree.py`, `treewalk` docstring + `:5203-5225`: `others_in_slot.append(self)`, then each old `dblnk.unmerge(...)`). The old prerm therefore runs after the copy, not before it.
- The staged copy (`$UNIT/old-ebuild/$CAT/$PKG/$OLD_PF.ebuild`) exists because `compute_environment` only parses `<category>/<pn>/<pf>.ebuild` paths (parent dir = PN) and canonicalizes (a symlink would resolve back to the vdb layout): the same staging the local replace loop does (`after_install.rs`, `run_vdb_saved_env_phase` copies the vdb ebuild to `scratch/<category>/<package>/` "for the path parse").
- The new instance is already in the vdb when the unmerge runs, so it lands in `others_in_slot` by full-slot match and shared files survive as replaced (Portage `unmerge(others_in_slot=...)`, `vartree.py:2505+`).
- `ROOT`, `PORTAGE_TMPDIR=$WORKDIR`, `CONFIG_PROTECT="$PROTECT"` and `CONFIG_PROTECT_MASK="$MASK"` ride the child env (the `ebuild` CLI reads all four from the environment when no client config resolves). `FEATURES` is not threaded: the no-config fallbacks equal the `make.globals` defaults (`unmerge-orphans`, `preserve-libs` on).
- A failed unmerge warns and continues (old files and entry kept) — the driver's standing warn-and-continue policy for old-instance teardown, and Portage's non-fatal replace unmerge (`vartree.py:5214-5218` logs `!!! unmerge FAILURE` and continues). The protocol lines `MERGE_PRERM=`/`MERGE_REMOVE=`/`MERGE_POSTRM=` are still emitted (pmtest pins them), now all three from the one Rust unmerge call: `ok` when it succeeded, `warn` for prerm/postrm when it failed, `skip:*` arms unchanged.
- Cross-architecture stays unsupported: the shipped binary's ELF `e_machine` is compared with the client's `uname -m` before anything ships (`client_binary.rs`, `docs/remote-merge.md` §5/§8). This change adds no cross-arch support.

## flow ownership scan (residue)
- The `REPLACED_OWN`/`OTHERS_OWN` scan still feeds the pre-copy collision gate only; its `awk '{print $2}'` still cuts paths with spaces. The unmerge no longer reads it. Fixing the gate's parse is left open (no backlog number filed here).
