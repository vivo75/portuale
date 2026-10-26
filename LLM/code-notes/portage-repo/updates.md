# crates/portage-repo/src/updates.rs
`profiles/updates` package and slot moves, applied at read time.

## module
- Portage: `portage.update` + `portage._global_updates._do_global_updates`. Each repo's `profiles/updates/<quarter>` files hold `move cat/old cat/new` and `slotmove cat/pkg <oldslot> <newslot>` directives that a sync applies, once and permanently, to the vdb, world file, binary packages and config files, and that ebuild metadata regeneration bakes into `metadata/md5-cache`.
- Portuale never syncs and never mutates the vdb, so it applies the directives at *read* time instead: `apply_updates_to_atom` (command-line and world atoms), `apply_updates_to_dep_string` (each `*DEPEND` string read from an *installed* vdb or binary entry -- Portage `update_dbentries` over `Package._dep_keys` via `vardb`/`bindb` `update_ents`, plus the `FakeVartree.perform_global_updates` fallback), and `apply_updates_to_cp` / `apply_updates_to_slot` (an installed package's identity as the resolver sees it). `installed_cp_sources` runs the `move` map *backwards* so a query for the new name still finds the not-yet-renamed vdb directory.
- Backlog #391: repository *ebuild* metadata is never rewritten. Portage's `_do_global_updates` calls `move_ent` / `update_ents` only on the vardb and bintree (plus the world/config files); the ebuild `*DEPEND` strings the resolver reads stay verbatim, so a dep on a moved-away name with no installed entry aborts (`no ebuilds to satisfy`) instead of merging the move target. `read_md5_cache` used to rewrite ebuild `*DEPEND` on read and returned rc 0 for `movedepconsumer`; the rewrite is gone, and `installed_cp_sources` now also hides a moved-away name's own stale directory (its entry was renamed away -- Portage `vartree.py:626 move_ent`). Evidence `LLM/evidence/2026-10-19-391/`.
- Origin: SCOPE_BACKLOG Part 2 Section H item 2.
- Deliberate cuts (all no-ops for a `--pretend` that never writes): the on-disk vdb category rename / `*DEPEND` rewrite, the world / world_sets rewrite, binary-package (`%`/`S`) updates, and the `/etc/portage/package.*` (`p`) rewrite -- `--pretend` mutates none of these, so in-memory application on read is equivalent.
- Portage `grab_updates`' `os.scandir` file order (+ `prev_mtimes` incremental logic): portuale sorts the quarter files by `(year, quarter)` parsed from the `NQ-YYYY` name (non-matching names last, by name) and re-reads them every run. Deterministic, and strictly better than Portage's arbitrary order for a chained move spanning two quarter files.
- A `slotmove` whose atom carries a version/operator: Portage `parse_updates` accepts any non-blocker atom (valid, recorded, applied); portuale's parser counts it valid too, but `UpdateCmd::SlotMove` carries only the bare `cat/pkg`, so the versioned form stays a no-op at apply time (backlog #583: keep the atom and match versions in `move_slot_ent` only).

## enum UpdateCmd
- `Move`: Portage `parse_updates` rejects a blocker or any `atom != atom.cp`.

## fn parse_updates_content
- Portage `portage.update.parse_updates`: one directive per non-blank line, `move`/`slotmove` only, wrong arity or a malformed atom/slot skips that line (Portage appends to `errors` -- never fatal).

## fn quarter_sort_key
- `(year, quarter)` from a `NQ-YYYY` updates-file name, for the quarter sort. A name that doesn't match sorts last (`year = i32::MAX`), by name.

## fn bare_cp
- Portage `parse_updates`' `Atom(x)` + `atom.blocker or atom != atom.cp` rejection.

## fn grab_updates_for_repo
- Every directive from one repo's `profiles/updates/`, quarter files in `(year, quarter)` order. Portage `grab_updates` + `parse_updates`, minus the scandir order / `prev_mtimes` logic (see module notes).

## fn apply_move_to_token
- First occurrence only: Portage `token.replace(old, new, 1)`.

## fn apply_slotmove_to_token
- Portage `update_dbentry` for a single `slotmove` (`orig_atom.version is None` branch), applied to one atom token: an atom on `cp` whose `:slot` (ignoring sub-slot / operator) is `old_slot` gets it rewritten to `new_slot`. The version check is on the *directive's* atom only -- a versioned *token* (`>=dev-libs/foo-1:0=`, upstream `test_update_dbentry.py:74-79`) is rewritten, as are blocker-prefixed and `[usedeps]` tokens (upstream `:80-103`).
- Splice is `with_slot` verbatim: the prefix before the first `:` (blockers survive), the rebuilt `:slot[/sub][op]` (a sub-slot equalling the old slot is rewritten, a differing one kept), then the tail past the old slot expression carried over byte-for-byte. `[x, y]`-style spacing needs no normalization: Portage rejects it as `InvalidAtom` under every EAPI.
- `::repo` tokens are left alone, and the reason is an EAPI rule, not splice fear: `repo_deps` is false for every EAPI in use (`eapi.py:314`), so `Atom(token, eapi=<entry EAPI>)` raises `InvalidAtom` (`dep/__init__.py:2140-2141`) and `update_dbentry` skips the token on each of the vdb/binpkg/read paths (all pass `parent=` → the entry's own EAPI). Only the `eapi=None` config-file path (which portuale deliberately does not implement) rewrites them.
- Portage `with_slot` keeps a differing sub-slot but rewrites one that equalled the old slot.

## fn update_move_targets
- Backlog #119: the flat `HashSet<(String, String)>`'s `contains` demanded a `(category.clone(), package.clone())` tuple (two `String` allocations per token of every rewritten dep string); grouping keeps the hot per-token check two borrowed lookups. On a Portage tree `profiles/updates/` holds thousands of accumulated commands, and this walk runs per token per dep string.

## fn apply_updates_to_atom_with
- Backlog #119.
- Fast path: if the atom's own `cp` isn't the `old`/`cp` of any command, nothing in the loop can ever rewrite it. Both lookups borrow (`HashMap<String,_>::get(&str)`, `HashSet<String>::contains(&str)`), so this costs no allocation.
- A command that matched the cp but left the token unchanged (a slotmove on another slot, a versioned atom) is reported as unchanged, exactly like the old `updated != *dep` comparison.

## fn apply_updates_to_dep_string
- Backlog #119: `None` means the input is returned as-is, so the caller can skip its insert and the unchanged case allocates no output at all. The old shape `format!`ed every chunk and `collect`ed unconditionally, even when no command matched -- 7.5% of the reference workload across the md5-cache misses' dep strings.
- Portage `update_dbentry`'s whitespace-preserving `re.split(r"(\s+)")` pass.

## fn apply_cmds_to_dep_string
- The `update_dbentries` shape (`update.py:107-127`) for the global-updates driver: explicit per-repo commands instead of the `global_package_updates()` cache, with the targets map built from those commands up front so the per-token fast path in `apply_updates_to_atom_with` still applies. Empty commands or blank input return `None` without touching the splitter (Portage: no `old_value in mycontent`, no change).

## fn move_chain_map_with
- Portage bakes package moves into the vdb + metadata cache at sync time; portuale applies them at read time instead. A linear replay of all ~500 `profiles/updates/` `move` commands *per lookup* -- and `apply_updates_to_cp` runs once per installed package at ~10 resolver call sites -- was ~15% of a deep `emerge -pu`. Built by replaying the command list once per distinct `old` cp, so a chain (`a/x -> b/x -> c/x`) resolves exactly as the old per-cp loop did.
- Same `OnceLock`-over-`global_package_updates()` shape as `update_move_targets` (so `--package-moves=n` -> empty list -> empty map -> identity, matching the old loop's no-op).

## fn installed_cp_sources_map_with
- `installed_cp_sources` used to replay the whole list (one `apply_updates_to_cp` per command) on every call -- 6 M calls on a deep `emerge -pu`; built once, so the lookup is a single `HashMap` get. Same `OnceLock` shape (so `--package-moves=n` -> empty list -> empty map).
- `installed_cp_sources` pushes `old` exactly when its fully-resolved chain lands on the queried cp; group by that destination up front, in command order, deduped. A `move` cycle replays back to its start (`dest == old`); the target is already `out[0]`, so skipping keeps the old per-call loop's `!out.contains(old)` invariant.

## fn installed_cp_sources
- Every `cat/pkg` whose forward-`move` chain lands on `(category, package)` -- including `(category, package)` itself, unless a `move` renamed it away (backlog #391: its entry no longer exists under that name -- Portage `move_ent` renamed the directory -- so its own stale directory is skipped; only names landing on it are scanned). Lets a vdb query for the *new* name also scan the *old*, not-yet-renamed directory (Portage global-updates renames it on disk; portuale doesn't).

## fn installed_cp_sources_core
- Backlog #391: shared core of `installed_cp_sources` and its test seam (`extra` is the inverted bucket, `moved_away` drops the queried name itself). A `move` cycle's second command still renames its own old name away (Portage threads the entry through every command in order), so that query scans nothing -- the old cycle pin's second half now asserts `[]`.

## fn installed_cp_sources_with
- Precedent: `apply_updates_to_dep_string_with`. Lets a synthetic `move` cycle be tested without global state.

## fn parse_updates_with_errors
- Portage `portage.update.parse_updates` as a `(cmds, valid, has_errors)` split: blank lines skipped, every other line one valid directive or one error. A versioned `slotmove` atom counts as valid (Portage only rejects blockers there) yet yields no command -- applying it is backlog #583. `parse_updates_content` delegates to it (identical command lists, so the read path is unchanged); the global-updates scan uses the full split (valid count for the force/dots, error flag for the record).
