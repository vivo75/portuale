//! Small predicates deciding what a run shows.

/// Backlog #185: real's merge-list gating (real
/// `_emerge/actions.py:464-469`, the `mergelist_shown` branch): a
/// non-`--pretend` run displays the list before merging only when
/// `--ask`, `--tree` or `--verbose` is given, and never when `--quiet`
/// is given without `--ask` (`--quiet --ask` still shows it). A
/// `--pretend` run always displays it. Pure predicate so the shapes
/// stay unit-pinned without spawning a merge; the abort-path and
/// `--autounmask-only` carve-outs live at the `show_merge_list` call
/// site, not here.
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
pub(crate) fn merge_list_shown(
    pretend: bool,
    ask: bool,
    tree: bool,
    verbose: bool,
    quiet: bool,
) -> bool {
    pretend || ((ask || tree || verbose) && !(quiet && !ask))
}

/// Backlog #271: real's `--autounmask-only` re-show (real
/// `_emerge/depgraph.py:10625` `_display_autounmask`, via
/// `_show_merge_list` at `:10488-10495` called from each change loop at
/// `:10686`/`:10733`/`:10767`/`:10803`, on the `actions.py:456-458`
/// early-return path that skips the normal `display()`): the merge list
/// prints ahead of the change blocks whenever there are changes to
/// report, even under `--autounmask-only`. Ported for `--pretend` runs
/// (grounded on the fresh host 3.0.82.2 staged-fixture probe,
/// `LLM/evidence/2026-09-30-271/`); a non-`--pretend` run keeps the old
/// suppression -- real has no pretend gate here, so that half is a cut,
/// unprobed this slice. Pure predicate so the matrix stays unit-pinned;
/// the empty-changes arm (a plain package prints nothing, rc 0) falls
/// out of `has_changes=false`, exactly like real's empty change loops.
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
pub(crate) fn autounmask_only_reshows_merge_list(
    pretend: bool,
    autounmask_only: bool,
    has_changes: bool,
) -> bool {
    pretend && autounmask_only && has_changes
}

/// Backlog #225: real `_emerge/actions.py:496-521` (`action_build`):
/// inside the display-flag branch (the same gate `show_merge_list`
/// carries for a non-`--pretend` run -- aborts and `--autounmask-only`
/// never reach the merge driver), a plan with no merge-bound entries
/// (`mergecount == 0`; real counts `operation == "merge"` packages, and
/// `emerge_build::resume_cpv` is `Some` for exactly that set) prints
/// `Nothing to merge; quitting.` on stdout and returns `EX_OK` before
/// the `resume_backup` rotation and before `Scheduler` -- the resume
/// list and its backup stay untouched (except the
/// selective-without-`--oneshot` deferral, which still rotates -- see
/// `selective_noop_deferred`). Pure predicate so the
/// `--pretend` exclusion (real's `--pretend` branch returns after the
/// display with no message) and the plain-run exclusion (`mergecount`
/// stays `None` without display flags, so `Scheduler` still writes --
/// possibly empty -- per #179) stay unit-pinned; the
/// resume-list-untouched half is pinned end to end in pmtest
/// (`test_portuale.py`, backlog #225).
pub(crate) fn nothing_to_merge(pretend: bool, show_merge_list: bool, mergecount: usize) -> bool {
    !pretend && show_merge_list && mergecount == 0
}

/// Backlog #232: real `_emerge/actions.py:496-521` (`action_build`),
/// the `mergecount == 0` arm's own deferral (`:514-516`): when the run
/// is `selective` (real `create_depgraph_params.py` -- `-u`/`-N`/
/// `--noreplace`/...) without `--oneshot`/`--onlydeps` and a
/// world-candidate favorite exists, real does NOT print `Nothing to
/// merge; quitting.` -- the prompt just moves into
/// `depgraph.saveNomergeFavorites` while the run falls through to the
/// `resume_backup` rotation (`:664-672`) and the world-file record,
/// skipping only `Scheduler` (`:674-675`). The second parameter is the
/// folded gate variable itself: real computes `oneshot =
/// "--oneshot" in myopts or "--onlydeps" in myopts`
/// (`actions.py:257`), and both the `:499` world-candidates guard and
/// the `:514` deferral test that folded value -- so `--onlydeps` keeps
/// the deferral at bay exactly like `--oneshot` (the #225 shape), and
/// only `--buildpkgonly` still suppresses at the record level below.
/// `selective` is portuale's own real-faithful computation (see its
/// site); a world candidate is any non-`@` favorite (real keeps every
/// non-`SETPREFIX` string, `actions.py:498-513`) or user `@set` (real
/// defaults `usersets` to `world-candidate = true`,
/// `_sets/__init__.py:133`, honoring a `sets.conf` override -- the
/// call site filters through `usersets_world_candidate`, so an
/// overridden set neither triggers here nor gets recorded; the
/// built-in `@world`/`@system`/`@profile` filter out on both sides
/// since real never marks them world candidates). Pure predicate so the gate
/// matrix stays unit-pinned; the rotation half is pinned end to end in
/// pmtest (`test_portuale.py`, backlog #232).
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
pub(crate) fn selective_noop_deferred(
    selective: bool,
    oneshot_or_onlydeps: bool,
    has_world_candidates: bool,
) -> bool {
    selective && !oneshot_or_onlydeps && has_world_candidates
}

/// Backlog #185 fix round 1: real's resolution-phase header gating
/// (real `_emerge/depgraph.py:12089-12094`, `_start_resolution_display`):
/// the `These are the packages that would be ...` header prints when
/// `--quiet` is absent and (`--ask`/`--tree`/`--verbose`) is given --
/// `--nodeps` is NOT consulted (the `spinner.mode = QUIET` assignment
/// at `depgraph.py:12129-12130` comes after the header block). Pure
/// predicate so the `--nodeps` + display-flag shapes stay unit-pinned.
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
pub(crate) fn resolution_header_shown(ask: bool, tree: bool, verbose: bool, quiet: bool) -> bool {
    !quiet && (ask || tree || verbose)
}

/// Backlog #185 fix round 1: real's spinner-notice gating (real
/// `_emerge/depgraph.py:12129-12132` + `stdout_spinner.py:117-139`):
/// `--quiet` or `--nodeps` forces `spinner.mode = QUIET`, so
/// `begin_notice("Calculating dependencies")` queues nothing,
/// `end_notice()` reports nothing to complete (no `... done!` line),
/// and `backtrack_depgraph`'s `finally:` prints no timing report.
/// Pure predicate so the `--nodeps` + display-flag shapes stay
/// unit-pinned.
pub(crate) fn resolution_notice_shown(quiet: bool, nodeps: bool) -> bool {
    !quiet && !nodeps
}
