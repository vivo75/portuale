# crates/portuale/src/pretend/gates.rs
Pure predicates for what a run shows: merge-list gating, the empty-plan arms, and resolution-phase display.

## module
- Pure predicates so the gate matrices stay unit-pinned without spawning a merge; the rotation/record halves are pinned end to end in pmtest (`test_portuale.py`).

## fn merge_list_shown
- Backlog #185: ports the `mergelist_shown` branch (`_emerge/actions.py:464-469`).
- The abort-path and `--autounmask-only` carve-outs live at the `show_merge_list` call site in `mod.rs`, not here.
- Backlog #185: Portage's merge-list gating (Portage `_emerge/actions.py:464-469`, the `mergelist_shown` branch): a non-`--pretend` run displays the list before merging only when `--ask`, `--tree` or `--verbose` is given, and never when `--quiet` is given without `--ask` (`--quiet --ask` still shows it). A `--pretend` run always displays it.

## fn autounmask_only_reshows_merge_list
- Backlog #271: ports `_display_autounmask` (`_emerge/depgraph.py:10625`, via `_show_merge_list` at `:10488-10495` called from each change loop at `:10686`/`:10733`/`:10767`/`:10803`), on the `actions.py:456-458` early-return path that skips the normal `display()`.
- Ported for `--pretend` runs only (grounded on the host 3.0.82.2 staged-fixture probe, `LLM/evidence/2026-09-30-271/`); a non-`--pretend` run keeps the old suppression -- Portage has no pretend gate here, so that half is a cut, unprobed this slice.
- The empty-changes arm (a plain package prints nothing, rc 0) falls out of `has_changes=false`, exactly like Portage's empty change loops.

## fn nothing_to_merge
- Aborts and `--autounmask-only` never reach the merge driver; the `--pretend` branch returns after the display with no message.
- The plain-run exclusion: `mergecount` stays `None` without display flags, so `Scheduler` still writes -- possibly empty -- per backlog #179.
- The resume-list-untouched half is pinned end to end in pmtest (`test_portuale.py`, backlog #225).
- The selective-without-`--oneshot` deferral still rotates; see `selective_noop_deferred`.
- Backlog #225: Portage `_emerge/actions.py:496-521` (`action_build`): inside the display-flag branch (the same gate `show_merge_list` carries for a non-`--pretend` run -- aborts and `--autounmask-only` never reach the merge driver), a plan with no merge-bound entries (`mergecount == 0`; Portage counts `operation == "merge"` packages, and `emerge_build::resume_cpv` is `Some` for exactly that set) prints `Nothing to merge; quitting.` on stdout and returns `EX_OK` before the `resume_backup` rotation and before `Scheduler` -- the resume list and its backup stay untouched (except the selective-without-`--oneshot` deferral, which still rotates -- see `selective_noop_deferred`).

## fn selective_noop_deferred
- Backlog #232: ports the `mergecount == 0` arm's own deferral (`actions.py:514-516`): the prompt moves into `depgraph.saveNomergeFavorites` while the run falls through to the `resume_backup` rotation (`:664-672`) and the world-file record, skipping only `Scheduler` (`:674-675`).
- `selective` is portuale's own Portage-faithful computation (see its site; Portage's `create_depgraph_params.py` -- `-u`/`-N`/`--noreplace`/...) .
- The second parameter is the folded gate variable itself: Portage computes `oneshot = "--oneshot" in myopts or "--onlydeps" in myopts` (`actions.py:257`), and both the `:499` world-candidates guard and the `:514` deferral test that folded value -- so `--onlydeps` keeps the deferral at bay exactly like `--oneshot` (the backlog #225 shape), and only `--buildpkgonly` still suppresses at the record level below.
- A world candidate is any non-`@` favorite (Portage keeps every non-`SETPREFIX` string, `actions.py:498-513`) or user `@set` (Portage defaults `usersets` to `world-candidate = true`, `_sets/__init__.py:133`, honoring a `sets.conf` override -- the call site filters through `usersets_world_candidate`, so an overridden set neither triggers here nor gets recorded; the built-in `@world`/`@system`/`@profile` filter out on both sides since Portage never marks them world candidates).
- The rotation half is pinned end to end in pmtest (`test_portuale.py`, backlog #232).

## fn resolution_header_shown
- Backlog #185 fix round 1: ports `_start_resolution_display` (`_emerge/depgraph.py:12089-12094`). The `spinner.mode = QUIET` assignment at `depgraph.py:12129-12130` comes after the header block, which is why `--nodeps` is not consulted.

## fn resolution_notice_shown
- Backlog #185 fix round 1: ports the spinner gating (`_emerge/depgraph.py:12129-12132` + `stdout_spinner.py:117-139`): `--quiet` or `--nodeps` forces `spinner.mode = QUIET`, so `begin_notice("Calculating dependencies")` queues nothing, `end_notice()` reports nothing to complete (no `... done!` line), and `backtrack_depgraph`'s `finally:` prints no timing report.

## fn top_level_abort_notice_shown
- Backlog #351 S7: the top-level all-masked abort prints the resolution notice on its way out only for a non-`--pretend` run (`!pretend && is_all_masked_abort_report(msg)`); the `--pretend` arm stays silent per the plan's Q6 ruling, and every other resolve error shape (plain miss, USE miss, REQUIRED_USE) keeps today's silence as out of scope. The resulting incoherence is Portage-faithful on each arm taken alone: Portage prints the preamble on `--pretend` aborts too (S0 a02/a03), while portuale's `--pretend` success path never prints it (pinned convention) — unifying the two is a decision of its own (plan Q6 default: leave it).

## fn pretend_changes_show_header
- Backlog #462: a `--pretend` run whose resolve completed with autounmask changes prints the resolution header before the rows (Portage's `_display_autounmask` runs `_show_merge_list` per change, `depgraph.py:10726` etc.). `--json` keeps stdout machine-readable (Portage has no `--json` pretend path; portuale's JSON early-returns before any display); an aborted resolve keeps its own preamble arms; the non-pretend path goes through the existing `!pretend` arm.
