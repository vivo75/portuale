use super::*;

#[test]
fn autounmask_only_reshows_the_merge_list_under_pretend() {
    // Portage: depgraph.py, _display_autounmask. Re-shows only with changes
    // on the `--pretend --autounmask-only` early-return path.
    // (pretend, autounmask_only, has_changes) -> re-shown.
    assert!(autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: true,
            autounmask_only: true,
            has_changes: true
        }
    ));
    assert!(!autounmask_only_reshows_merge_list(
        AutounmaskOnlyReshowsMergeListArgs {
            pretend: true,
            autounmask_only: true,
            has_changes: false
        }
    ));
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
    // Non-pretend keeps the old suppression (a cut: unprobed this slice).
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
    // Portage: actions.py. The early return fires only inside the display-flag
    // branch of a non-pretend run, and only when `mergecount == 0`.
    // (pretend, show_merge_list, mergecount) -> early return.
    assert!(nothing_to_merge(false, true, 0));
    assert!(!nothing_to_merge(true, true, 0));
    assert!(!nothing_to_merge(false, false, 0));
    assert!(!nothing_to_merge(false, true, 1));
    assert!(!nothing_to_merge(false, true, 7));
    assert!(!nothing_to_merge(true, false, 3));
}

#[test]
fn selective_noop_deferred_only_for_selective_non_oneshot_world_runs() {
    // Portage: actions.py. Deferred only for a selective run with world
    // candidates and neither `--oneshot` nor `--onlydeps`.
    // (selective, oneshot_or_onlydeps, has_world_candidates) -> deferred.
    assert!(selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: true,
        oneshot_or_onlydeps: false,
        has_world_candidates: true
    }));
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
    assert!(!selective_noop_deferred(SelectiveNoopDeferredArgs {
        selective: false,
        oneshot_or_onlydeps: false,
        has_world_candidates: true
    }));
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
    // Portage: depgraph.py, _start_resolution_display. The header ignores
    // `--nodeps`; the notice and timing need neither `--nodeps` nor `--quiet`.
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
