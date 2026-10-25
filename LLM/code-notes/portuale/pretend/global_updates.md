# crates/portuale/src/pretend/global_updates.rs
The applied-updates AUTOCLEAN force (backlog #562): whether any repo's `profiles/updates/` holds a changed file with >=1 valid directive, measured against the `updates` mtime record in `<EROOT>/var/cache/edb/mtimedb`.

## fn global_updates_force
- Ports Portage's pre-resolve `_global_updates` call (`lib/_emerge/actions.py:3941-3964`): runs for every action except `help`/`info`/`sync`/`version` (`--help` returns before any config load, so only the other three are gated here) and unless `--package-moves=n`. Computed in `run()` before the resolve, so `--pretend` and failed merges still commit the record (Portage commits at `actions.py:3954`, pre-merge). Returns the *target* root's force for the merge gate; the running root is handled inside `updates_force_for_roots`.
- Which root's AUTOCLEAN the tail reads: `action_build` unpacks `settings` from `emerge_config`, i.e. the *target* config's settings (`_emerge_config.__iter__` yields `target_config.settings`), while the force sets `AUTOCLEAN` on whichever `root_config`'s updates fired (`actions.py:3958-3962`). So a running-root-only force stays silent on the tail -- mirrored by returning only the target force. The #551 re-read cut stays: no config reload, only the force signal + record are mirrored (the world/vdb/binpkg/config-file moves stay read-time, and the `Performing Global Updates` progress block stays cut -- the suite blesses "modulo Global Updates").
- Deliberate cuts (documented, not mirrored): the `secpass<2`/`SANDBOX_ACTIVE`/vdb-lock early-False gates (`_global_updates.py:39-56`) -- portuale runs unprivileged in the suite, where those gates would pin the feature off; an unreadable updates file is skipped rather than propagating (Portage's `grab_updates` has no such guard, an unhandled corner there).

## fn updates_force_for_roots
- The two-root core behind `global_updates_force`: the target check's force is returned; when the running root differs its record is maintained as a side effect (same repo list, its own record -- Portage passes the same `trees` with each `root_config`'s own `mtimedb`). Split out so tests need no process-global env.

## fn check_root_updates
- Ports `_do_global_updates`' return (`retupd`, `_global_updates.py:139`) without its side effects: true iff a *changed* updates file yields >=1 valid directive. Per-repo `profiles/updates/` scan skipping dotfiles and non-files; repos sharing one updates directory scan it once (`updpath_map`); only files whose whole-second `st_mtime` differs from the record are parsed (`grab_updates`); error-free mtimes merge into the record and commit on true only (a valid-but-errored file fires but stays unrecorded, `:131-135`; a changed file with no valid directive commits nothing). Every I/O error is swallowed (`MtimeDB.__write_to_disk`'s `except OSError`), so a non-writable root still forces and never fails the run. Record keys are the scanned path strings (`myfile.path`).

## fn parse_updates_file
- Ports `lib/portage/update.py::parse_updates` as a silent `(valid, error-free)` split: blank lines skipped; `move` needs exactly 2 bare-`cat/pkg` atoms; `slotmove` needs exactly 3 fields with a parseable non-blocker package atom and valid slot names; anything else marks the file errored. Silent by design -- a malformed updates file never fails a merge (and the #551 "no `ERROR:` line at all" rule stays: Portage's `ERROR:` stderr lines are not mirrored). One deliberate looseness, matching Portage exactly: a `slotmove` package may carry a version (Portage only rejects blockers there); `bare_cp` matches Portage's `atom != atom.cp` check for `move` atoms.

## fn bare_cp
- Shared validator with `portage-repo`'s updates application (`parse_updates_content`'s own `bare_cp`): a bare `cat/pkg`, else `None`. Duplicated here rather than imported because that helper is `pub(crate)` to `portage-repo`.

## fn update_atom
- The `slotmove`-package half of `parse_updates`: parseable and not a blocker. Versioned atoms pass (see `parse_updates_file`).

## fn valid_slot_name
- `[A-Za-z0-9_][A-Za-z0-9+_.-]*`, i.e. Portage `_slot` (`versions.py`) with no `/` alternative -- the EAPI-5 subslot form is rejected by the same regex here, where Portage rejects it with a separate `/` check (`update.py`).

## fn file_mtime
- Whole-second `st_mtime` (`grab_updates` compares `mystat[stat.ST_MTIME]` ints); `None` when unreadable.

## tests
- `pretend::tests::global_updates`: fresh root fires + records; repeat run silent; `--package-moves=n` and `info`/`sync`/`version` skip without touching the disk; error file fires but stays unrecorded (fires again next run); directiveless files stay silent and uncommitted; running-root-only force never reads as target force (via `updates_force_for_roots`, no process env -- `PORTAGE_RUNNING_ROOT` is also written by unrelated tests on other threads); unwritable root still forces. The merge-gate OR itself is pinned at pmtest level (`test_applied_updates_force_*`: fresh-ROOT tail with AUTOCLEAN nowhere, repeat silence, `--package-moves=n` silence, explicit-`no` override).
