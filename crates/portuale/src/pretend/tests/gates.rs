use super::*;

#[test]
fn nonpretend_merge_list_shown_only_when_portage_shows_it() {
    // Backlog #185: real `_emerge/actions.py:464-469`
    // (`mergelist_shown` branch) plus the m185 container probe (real
    // 3.0.81.3, `emerge --oneshot --usepkgonly probe/probe-a` in six
    // shapes; gating identical in 3.0.82.2 by source read): a
    // non-`--pretend` run shows the merge list only with `--ask`,
    // `--tree` or `--verbose`, and never with `--quiet` unless
    // `--ask` is also given. `--pretend` always shows it.
    // (pretend, ask, tree, verbose, quiet) -> shown
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: true,
        ask: false,
        tree: false,
        verbose: false,
        quiet: false
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: true,
        ask: false,
        tree: false,
        verbose: false,
        quiet: true
    }));
    assert!(!merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: false,
        quiet: false
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: true,
        tree: false,
        verbose: false,
        quiet: false
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: true,
        verbose: false,
        quiet: false
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: true,
        quiet: false
    }));
    assert!(!merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: false,
        quiet: true
    }));
    assert!(!merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: true,
        quiet: true
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: true,
        tree: false,
        verbose: false,
        quiet: true
    }));
}

#[test]
fn autounmask_only_reshows_the_merge_list_under_pretend() {
    // Backlog #271: real `_display_autounmask` re-shows the merge
    // list via `_show_merge_list` (`depgraph.py:10625`, `:10686`,
    // `:10488`) even on the `--autounmask-only` early-return path
    // (`actions.py:456-458`) -- whenever it has changes to report.
    // (pretend, autounmask_only, has_changes) -> re-shown.
    assert!(autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: true,
            autounmask_only: true,
            has_changes: true
        }
    ));
    // No changes (a plain package): still nothing, like real (no
    // `_show_merge_list` call fires from an empty change loop).
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: true,
            autounmask_only: true,
            has_changes: false
        }
    ));
    // The control path shows the list through the normal gate, not
    // this carve-out.
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: true,
            autounmask_only: false,
            has_changes: true
        }
    ));
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: true,
            autounmask_only: false,
            has_changes: false
        }
    ));
    // Non-`--pretend` keeps the old suppression (unprobed this
    // slice; real has no pretend gate here, so this is a cut).
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: false,
            autounmask_only: true,
            has_changes: true
        }
    ));
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: false,
            autounmask_only: true,
            has_changes: false
        }
    ));
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: false,
            autounmask_only: false,
            has_changes: true
        }
    ));
}

#[test]
fn nothing_to_merge_fires_only_for_a_shown_nonpretend_empty_plan() {
    // Backlog #225: real `_emerge/actions.py:464-521` -- the early
    // return fires only inside the display-flag branch of a real
    // (non-`--pretend`) run, and only when `mergecount == 0`.
    // (pretend, show_merge_list, mergecount) -> early return.
    assert!(nothing_to_merge(false, true, 0));
    // `--pretend` displays and returns with no message.
    assert!(!nothing_to_merge(true, true, 0));
    // A plain run (no display flags) never takes this branch:
    // `mergecount` stays `None` and `Scheduler` still writes (per
    // #179, possibly empty).
    assert!(!nothing_to_merge(false, false, 0));
    // Anything merge-bound proceeds to the prompt / scheduler.
    assert!(!nothing_to_merge(false, true, 1));
    assert!(!nothing_to_merge(false, true, 7));
    assert!(!nothing_to_merge(true, false, 3));
}

#[test]
fn selective_noop_deferred_only_for_selective_non_oneshot_world_runs() {
    // Backlog #232: real `_emerge/actions.py:514-516` -- the
    // deferral (no message, rotation + world record, no `Scheduler`)
    // fires only when the run is `selective` without `--oneshot` /
    // `--onlydeps` and a world-candidate favorite exists. The second
    // parameter is real's folded gate variable (`actions.py:257`:
    // `--oneshot` or `--onlydeps`), so one `true` covers both flags.
    // (selective, oneshot_or_onlydeps, has_world_candidates) -> deferred.
    assert!(selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: true,
        oneshot_or_onlydeps: false,
        has_world_candidates: true
    }));
    // `--oneshot` -- or `--onlydeps`, which real folds into the same
    // gate variable (`actions.py:257`) -- keeps the deferral at bay
    // (the #225 shape), with or without world candidates.
    assert!(!selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: true,
        oneshot_or_onlydeps: true,
        has_world_candidates: true
    }));
    assert!(!selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: true,
        oneshot_or_onlydeps: true,
        has_world_candidates: false
    }));
    // A non-selective all-noop run is the plain `Nothing to merge`
    // shape even with world candidates around.
    assert!(!selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: false,
        oneshot_or_onlydeps: false,
        has_world_candidates: true
    }));
    // No world candidate (bare args, or only `@world`/`@system`) is
    // the plain `Nothing to merge` shape even when selective.
    assert!(!selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: true,
        oneshot_or_onlydeps: false,
        has_world_candidates: false
    }));
    assert!(!selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: false,
        oneshot_or_onlydeps: true,
        has_world_candidates: false
    }));
}

#[test]
fn nonpretend_resolution_display_splits_header_from_notice_under_nodeps() {
    // Backlog #185 fix round 1: real
    // `_emerge/depgraph.py::_start_resolution_display`
    // (`depgraph.py:12086-12132`) prints the header without consulting
    // `--nodeps` (the `spinner.mode = QUIET` assignment at
    // `:12129-12130` comes after the header block), while the
    // `Calculating dependencies ... done!` notice and the timing
    // report are suppressed under `--nodeps` (QUIET spinner:
    // `end_notice()` reports nothing to complete) or `--quiet`.
    // (ask, tree, verbose, quiet) -> header shown.
    assert!(resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: false,
        verbose: true,
        quiet: false
    }));
    assert!(!resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: false,
        verbose: true,
        quiet: true
    }));
    assert!(!resolution_header_shown(ResolutionHeaderShownArgs {
        ask: true,
        tree: false,
        verbose: false,
        quiet: true
    }));
    assert!(resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: true,
        verbose: false,
        quiet: false
    }));
    assert!(!resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: false,
        verbose: false,
        quiet: false
    }));
    // (quiet, nodeps) -> notice + timing shown.
    assert!(resolution_notice_shown(false, false));
    assert!(!resolution_notice_shown(false, true));
    assert!(!resolution_notice_shown(true, false));
    assert!(!resolution_notice_shown(true, true));
    // The three review shapes (`--verbose` in each): `--nodeps`
    // non-pretend -> header shown, calc/timing hidden; plain ->
    // both shown; `--quiet` -> neither.
    assert!(resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: false,
        verbose: true,
        quiet: false
    }));
    assert!(!resolution_notice_shown(false, true));
    assert!(resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: false,
        verbose: true,
        quiet: false
    }));
    assert!(resolution_notice_shown(false, false));
    assert!(!resolution_header_shown(ResolutionHeaderShownArgs {
        ask: false,
        tree: false,
        verbose: true,
        quiet: true
    }));
    assert!(!resolution_notice_shown(true, false));
}
