# crates/portuale/src/global_updates/mod.rs
The `_do_global_updates` port (backlog #576): the mtime-gated scan, the per-root driver, and the shared map/matcher/collapse helpers.

## fn updates_file_mtime
- Whole-second `st_mtime` (`grab_updates` compares `mystat[stat.ST_MTIME]` ints); `None` when unreadable. Moved here from `pretend/global_updates.rs` so the scan owns the only copy.

## fn main_repo_name
- The main repo's name for the `DEFAULT` alias and the matchers, extracted once per run.

## struct GlobalUpdatesSettings
- The per-root settings halves `_do_global_updates` reads: the two `CONFIG_PROTECT` lists, the case flag, `$PKGDIR`, and the binpkg writer halves (`binpkg-multi-instance`, `BINPKG_COMPRESS` + flags, `MAKEOPTS`, bzip2 command, `compress-index`).

## fn settings_for
- Resolves the halves from one root's config: the incremental folds for the protect lists and `FEATURES`, the resolved chain for the binpkg scalars. The gpkg files-only-vs-full-chain nuance (`package_options_from_env`) is not modelled: the full chain feeds both formats.

## fn collapse_ws
- `vartree._aux_get`'s collapse in one place (`vartree.py:1016-1037`: `" ".join(myd.split())`), shared by the vdb half (`dep_content`) and the binpkg half (metadata values, dep strings, aux reads): every whitespace run becomes one blank. The binary database reads through the same rule (`bintree.py:aux_get`).

## struct RepoMatch
- The `_do_global_updates` `repo_match` closure (`_global_updates.py:161-164`) in one place: `new` snapshots the map's repo names plus the main repo's name, and `matches(repo_name, repository)` answers the same-repo-or-main-covering-unmapped check. Used by the vdb moves, both config-files passes (world, `package.*`) and `update_ents`, each through a one-line closure adapter.

## fn repo_update_map
- The ungated full read the half-tests drive in isolation: every file counts as changed, into a void printer. Implemented as `scan_updates` with an empty record so the dedup and `DEFAULT` logic live in exactly one place. Test-only (`#[cfg(test)]`): production always goes through the gated scan.

## fn scan_updates
- Ports the `_do_global_updates` scan loop (`_global_updates.py:79-153`, notice/dots at `:99-137`): every repo's updates directory (missing dir omitted, `:82`; shared ones scanned once, `:85-87`) contributes only its changed files' valid directives; the notice prints for the first changed file and each one gets its dots (`valid` count, including versioned `slotmove` lines). An errored file applies but is never recorded (`:131-137`), so it forces again next run; error-free files land in `timestamps` even with zero valid directives. `retupd` is set by the valid count, not the representable commands. File order is sorted by name (deterministic; Portage uses `scandir` order, arbitrary per filesystem -- chained moves across files could therefore apply in a different order; the fixture has one file so the capture cannot tell). The main repo's list is aliased as `DEFAULT` (`:149-153`).

## fn run_global_updates
- Ports `_global_updates` + `_do_global_updates` for one root, in Portage's order: the `secpass < 2` (caller-computed privilege, `data.py` unprivileged mode) and `SANDBOX_ACTIVE` gates (`:39`; the caller reads the environment and passes `sandboxed`, so the gate test sets no process-wide variable that would stop concurrent tests), the mtime-gated scan, the vdb lock (denied refuses the whole run before anything prints, `:42-46`), the world decisions, the per-command vdb and binpkg moves (interleaved, `:190-206`), the world write, `update_config_files`, the record inside `if retupd` (`prev_mtimes.update(timestamps)`, `:246-251` -- merged into the previous record, never replacing it), the vdb then binpkg `update_ents`, and the closing blank lines. Returns whether any valid directive was applied (the caller's `AUTOCLEAN` force).
- Colour is always off while the block prints: `emerge_main` disables it at startup (`main.py:1255`) and `adjust_configs` re-enables it only after the updates run (`actions.py:4037`).
- A failed record write is reported on stderr and the run continues (the moves already applied; the missing record retries them next time). Application-step failures return `Err` with the record untouched, so nothing is half-committed.
- The lock is the whole-run `vardb.lock()`: created like `ensure_dirs`, held across every half. The halves take no lock themselves (a nested `flock` `LOCK_EX` from the same process would block); the whole-map move functions are test-only for that reason.

## fn lock_vdb_for_updates
- The vdb gate: a missing vdb is created, an un-writable one refuses the whole run (`Ok(None)`), like `PermissionDenied` from `vardb.lock()`. The writability probe targets the lock sibling's directory (the vdb's parent -- portuale's lock file lives beside the vdb, not inside it, unlike Portage's `lockdir`), falling back to the nearest existing ancestor when creation itself fails.

## tests
- `record_holds_only_the_error_free_changed_files`: the full fixture run leaves `mtimedb["updates"]` holding exactly the one updates file with its live mtime, while foreign keys seeded beforehand survive verbatim.
- `directiveless_changed_file_records_nothing`: a changed but directive-less file still prints the notice and its dots (`:99-130` runs before the valid check at `:139`) but records nothing and forces nothing.
- `error_only_run_applies_but_records_nothing`: an errored file forces yet writes no record file at all (`timestamps` stays empty).
- `versioned_slotmove_counts_as_valid`: a versioned `slotmove` atom forces, earns its dot and is recorded (Portage applies it; the apply side is backlog #583).
- `unprivileged_root_runs_nothing`, `sandbox_active_runs_nothing`, `unwritable_vdb_runs_nothing`: the three gates (each pinned to run nothing, print nothing, record nothing).
- `record_write_failure_still_forces`: an unwritable record still forces with the moves applied (reported, never discarded).
- `the_record_accumulates_across_runs`: the record merges (`prev_mtimes.update`), so a second run changing another file keeps the first run's entries (regression pin for the replace-instead-of-merge bug).
- `settings_for_reads_the_resolved_config`: the halves resolve from one root's config.

## fn scan_updates (ordering)
- Empty lists stay so non-master `repository not in repo_map` checks see every repo (Portage keeps them in `repo_map` and skips them at apply time).

## fn copy_fixture
- Scratch copy of the Task 5 fixture tree (`fixtures/global-updates`).
