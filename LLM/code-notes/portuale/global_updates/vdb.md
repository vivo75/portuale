# crates/portuale/src/global_updates/vdb.rs
The vdb half of `_do_global_updates` (backlog #576): `move_ent` / `move_slot_ent` over the installed database under the vdb lock.

## fn apply_vdb_moves
- Ports the move loop (`_global_updates.py:155-206`): iterate the repo map in order, skip `DEFAULT` and empty lists, build `repo_match` per repo (same repo, or the main repo's updates applying to an entry whose repository is in no map -- `:161-164`), and count `move` renames and `slotmove` rewrites separately.
- Lock: `PortageLockfile::acquire_new_lockfile` on the vdb directory -- the same path Portage's `lockdir` holds (`<parent>/.<basename>.portage_lockfile`, `locks.py`). Database backends serialize in their own write transactions instead. A lock failure surfaces as `Err`; the `PermissionDenied` early-False gate (`_global_updates.py:42-46`) is a later piece's.
- Backend path: `InstalledDb` write transactions (`entry_image` into `insert_entry`, `delete_entry`, `replace_file`) -- the same calls `vdb_rw.rs::move_entry` funnels into, minus the FUSE inode rebinding (no mount here, nothing to rebind). Per-entry transactions, like Portage's per-entry `_movefile`/`aux_update`.
- Deliberate cut: `bindb` moves, world, `package.*`, `update_ents`, output and the record/force are later pieces; this function touches `var/db/pkg` only.

## fn move_ent
- Ports `vartree.move_ent` (`vartree.py:621-682`): candidates are entries whose parsed `cp` equals `oldcp` (`split_pf`, the `_pkgsplit` shape Portage's `match` uses; an unparseable `pf` never matches). `PROVIDE`-virtual matches cannot occur (matching is on the stored key, not the atom index).
- `repository` gating via `aux_get("repository")`, `""` when the file is absent (Portage resolves the repo through `_pkg_str` metadata and would raise `KeyError` on a missing file -- the fixture always carries it; untested).
- `isvalidatom(newcp, eapi=...)`: the updates-time `bare_cp` check already rejected malformed names; per-EAPI spelling rules are a `portage_dep` gap, not re-checked per entry.
- Faithful quirks: the counter increments even when the destination exists (the entry stays put -- `:655-662` increments before the dest check); the rename is skipped when the source is gone (the existence guard comes first, like Portage's).
- `new_pf` comes from the whole-`cpv` string replace (`mycpv.replace(mycpv_cp, newcp, 1)`, `:650`), so a `pf` embedding the old name elsewhere resolves the same way.
- The rename is one image transaction: `insert_entry` (the `FilesDb` write path preserves bytes, modes and mtimes) then `delete_entry`, with the ebuild rename and the `PF`/`CATEGORY` rewrites folded into the image (same end bytes as Portage's `write_atomic` pair). `delete_entry` drops an emptied category directory, which Portage's rename keeps, so the old category directory is recreated on files trees for cross-category moves.
- The vdb lock is held by the caller, matching `vardb.lock()` around `_do_global_updates`.

## fn move_slot_ent
- Ports `dbapi.move_slot_ent` (`dbapi/__init__.py:449-494`): entries on `cp` whose `SLOT`'s slot part equals `origslot` (split on `/`; Portage matches the slotted atom through `match`, same outcome for plain slots).
- Faithful quirk: `newslot` is reassigned inside the loop, so a second entry inherits the first one's sub-slot suffix (`:483-488` runs per entry on the shared variable). `SLOT` is written as `newslot + "\n"` via `replace_file` (Portage `aux_update` funnels to `setfile`).
- No `KeyError`/`InvalidData` path: entries come from the listing, so the `_pkg_str(mycpv, atom.repo)` refinement cannot fail here.

## tests
- `vdb_moves_match_the_portage_capture`: scratch copy (`cp -a`, mtimes preserved) of the fixture template (`fixtures/global-updates/`); no `repos.conf` needed (the `RepoConfig` is built directly). Expected vdb tree = the committed Portage capture (`LLM/evidence/2026-10-25-review/t5/after/var/db/pkg`), except `*DEPEND` files: `move_ent`/`move_slot_ent` never touch dep strings, so those are pinned to the before-state (the capture's rewrites there are `update_ents`). World is pinned byte-identical (untouched by this piece).
