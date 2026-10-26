# crates/portuale/src/global_updates/vdb.rs
The vdb half of `_do_global_updates` (backlog #576): `move_ent` / `move_slot_ent` over the installed database under the vdb lock.

## fn apply_vdb_moves
- Ports the move loop (`_global_updates.py:155-206`): iterate the repo map in order, skip `DEFAULT` and empty lists, build the per-repo closure over `RepoMatch::matches` (same repo, or the main repo's updates applying to an entry whose repository is in no map -- `:161-164`), and count `move` renames and `slotmove` rewrites separately.
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

## fn update_ents
- Ports `dbapi.update_ents` (`dbapi/__init__.py:383-447`) for the vdb only: entries in sorted order, dep keys `Package._dep_keys` (`BDEPEND`, `DEPEND`, `IDEPEND`, `PDEPEND`, `RDEPEND`), one write transaction per changed entry, count of changed entries returned. The `bindb.update_ents` half (embedded xpak/gpkg metadata, `*` progress) rides with the binpkg piece, which owns the only write path for those formats.
- Per-entry updates are `repo_dict[pkg.repo]` else `repo_dict["DEFAULT"]` (`:422-431`): the first map entry (in order, skipping `DEFAULT`) whose `RepoMatch::matches` hits the entry's `repository`. A repo in the map with an empty list yields no updates for its entries (no `DEFAULT` fallthrough, `:433-434`); an unmapped repository falls to the main repo's list. `repository` reads through `aux_get`, `""` when the file is absent.
- `dep_content` is `vartree._aux_get` for a dep key (`vartree.py:1016-1037`): a missing file reads as `""` (dep keys are `_aux_cache_keys` members, so the `OSError` arm serves `""`, not a skip), and a present one goes through the shared `super::collapse_ws` (same rule the binpkg half uses for its metadata values). `setfile` writes the rewritten string verbatim (`vartree.py:6197-6206`), which is why rewritten files lose their trailing newline. Only changed files are written (`update_dbentries` returns just the diffs, `update.py:107-127`); a rewrite that only normalizes whitespace without moving an atom is not a change, so the raw bytes stay.
- Rewrites run through `portage_repo::apply_cmds_to_dep_string` (per-token `apply_move_to_token` / `apply_slotmove_to_token`, commands in order, whitespace preserved). Cuts vs `update_dbentry` (`update.py:23-104`), all unpinned by the fixture (bare atoms, no blockers): no `isvalidatom(new, eapi)` gate, no per-EAPI atom grammar, no #367215 self-blocker skip (would need the `_pkg_str` parent and a `match_from_list` against it), no `Atom`-roundtrip normalization after a `move` splice, and `slotmove` leaves versioned tokens and `::repo`/`[usedeps]` tokens alone where Portage rewrites the slot. The versioned-`slotmove`-directive gap is backlog #583 (parse-time); the versioned-token cut here is the same family at apply time.
- The empty-rewrite unlinks the file (`vartree.py:1132-1139`) but is unreachable: rewrites only substitute tokens, and empty content never matches a command, so no delete path is implemented.
- No vdb lock inside: like `update_world`, this runs under the driver's lock (Portage holds `vardb.lock()` across the whole `_do_global_updates`); a nested `flock` `LOCK_EX` from the same process would block.
- `on_update(maxval, curval)` mirrors the `onUpdate` protocol (`:408-409, :444-445`): `(maxval, 0)` up front, `(maxval, pos+1)` per rewritten entry. The `#` printing itself is the output piece's.

## tests
- `vdb_moves_match_the_portage_capture`: scratch copy (`cp -a`, mtimes preserved) of the fixture template (`fixtures/global-updates/`); no `repos.conf` needed (the `RepoConfig` is built directly). Expected vdb tree = the committed Portage capture (`LLM/evidence/2026-10-25-review/t5/after/var/db/pkg`), except `*DEPEND` files: `move_ent`/`move_slot_ent` never touch dep strings, so those are pinned to the before-state (the capture's rewrites there are `update_ents`). World is pinned byte-identical (untouched by this piece).

## fn apply_vdb_cmd
- One command's vdb moves, for the output driver's per-command interleave (`_global_updates.py:190-206` nests the binpkg move inside the per-command loop, so batch totals alone would print `@s%S` instead of the captured `@%sS`): `move` counts through `move_ent` (`'@'`), `slotmove` through `move_slot_ent` (`'s'`); the hook fires only on a nonzero count (Portage's `if moves:`). Returns this command's counts; `apply_vdb_moves` sums them into the report and passes its own hook through. Takes the database unlocked; the batch wrapper (and the driver) holds the vdb lock.

## struct VdbMovesArgs (test-only)
- Takes the gated map alongside `repos` (repos now only supply the main-repo name for the matcher). The whole-map function is test-only (`#[cfg(test)]`): the driver loops `apply_vdb_cmd` per command for the `@%sS` interleave while holding the one vdb lock itself (a nested `flock` would block).
