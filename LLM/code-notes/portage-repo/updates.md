# crates/portage-repo/src/updates.rs
`profiles/updates` package and slot moves, applied at read time.

## module
- Portage: `portage.update` + `portage._global_updates._do_global_updates`. Each repo's `profiles/updates/<quarter>` files hold `move cat/old cat/new` and `slotmove cat/pkg <oldslot> <newslot>` directives that a sync applies, once and permanently, to the vdb, world file, binary packages and config files, and that ebuild metadata regeneration bakes into `metadata/md5-cache`.
- Portuale never syncs and never mutates the vdb, so it applies the directives at *read* time instead: `apply_updates_to_atom` (command-line and world atoms), `apply_updates_to_dep_string` (each `*DEPEND` string read from md5-cache -- Portage `update_dbentry`), and `apply_updates_to_cp` / `apply_updates_to_slot` (an installed package's identity as the resolver sees it). `installed_cp_sources` runs the `move` map *backwards* so a query for the new name still finds the not-yet-renamed vdb directory.
- Origin: SCOPE_BACKLOG Part 2 Section H item 2.
- Deliberate cuts (all no-ops for a `--pretend` that never writes): the on-disk vdb category rename / `*DEPEND` rewrite, the world / world_sets rewrite, binary-package (`%`/`S`) updates, and the `/etc/portage/package.*` (`p`) rewrite -- `--pretend` mutates none of these, so in-memory application on read is equivalent.
- Portage `grab_updates`' `os.scandir` file order (+ `prev_mtimes` incremental logic): portuale sorts the quarter files by `(year, quarter)` parsed from the `NQ-YYYY` name (non-matching names last, by name) and re-reads them every run. Deterministic, and strictly better than Portage's arbitrary order for a chained move spanning two quarter files.
- A `slotmove` whose atom carries a version/operator: Portage `update_dbentry` acts only on the unversioned case (`orig_atom.version is None`), so portuale drops the qualified form at parse time (same net effect).

## enum UpdateCmd
- `Move`: Portage `parse_updates` rejects a blocker or any `atom != atom.cp`.

## fn bare_cp
- Portage `parse_updates`' `Atom(x)` + `atom.blocker or atom != atom.cp` rejection.

## fn grab_updates_for_repo
- Portage `grab_updates` + `parse_updates`, minus the scandir order / `prev_mtimes` logic (see module notes).

## fn apply_move_to_token
- First occurrence only: Portage `token.replace(old, new, 1)`.

## fn apply_slotmove_to_token
- Portage `update_dbentry`'s `orig_atom.version is None` branch; the `:slot` match ignores sub-slot / operator.
- Micro-cut: Portage `Atom.with_slot` rebuilds every part; portuale leaves a `::repo`/`[usedeps]` target alone rather than risk a corrupt splice (no fixture covers the combination).
- Portage `with_slot` keeps a differing sub-slot but rewrites one that equalled the old slot.

## fn update_move_targets
- Backlog #119: the flat `HashSet<(String, String)>`'s `contains` demanded a `(category.clone(), package.clone())` tuple (two `String` allocations per token of every rewritten dep string); grouping keeps the hot per-token check two borrowed lookups. On a Portage tree `profiles/updates/` holds thousands of accumulated commands, and this walk runs per token per dep string.

## fn apply_updates_to_atom_with
- Backlog #119.
- A command that matched the cp but left the token unchanged (a slotmove on another slot, a versioned atom) is reported as unchanged, exactly like the old `updated != *dep` comparison.

## fn apply_updates_to_dep_string
- Backlog #119: `None` means the input is returned as-is, so the caller can skip its insert and the unchanged case allocates no output at all. The old shape `format!`ed every chunk and `collect`ed unconditionally, even when no command matched -- 7.5% of the reference workload across the md5-cache misses' dep strings.
- Portage `update_dbentry`'s whitespace-preserving `re.split(r"(\s+)")` pass.

## fn move_chain_map_with
- Portage bakes package moves into the vdb + metadata cache at sync time; portuale applies them at read time instead. A linear replay of all ~500 `profiles/updates/` `move` commands *per lookup* -- and `apply_updates_to_cp` runs once per installed package at ~10 resolver call sites -- was ~15% of a deep `emerge -pu`. Built by replaying the command list once per distinct `old` cp, so a chain (`a/x -> b/x -> c/x`) resolves exactly as the old per-cp loop did.
- Same `OnceLock`-over-`global_package_updates()` shape as `update_move_targets` (so `--package-moves=n` -> empty list -> empty map -> identity, matching the old loop's no-op).

## fn installed_cp_sources_map_with
- `installed_cp_sources` used to replay the whole list (one `apply_updates_to_cp` per command) on every call -- 6 M calls on a deep `emerge -pu`; built once, so the lookup is a single `HashMap` get. Same `OnceLock` shape (so `--package-moves=n` -> empty list -> empty map).
- A `move` cycle replays back to its start; skipping keeps the old per-call loop's `!out.contains(old)` invariant.

## fn installed_cp_sources
- Portage global-updates renames the vdb directory on disk; portuale doesn't.

## fn installed_cp_sources_with
- Precedent: `apply_updates_to_dep_string_with`. Lets a synthetic `move` cycle be tested without global state.
