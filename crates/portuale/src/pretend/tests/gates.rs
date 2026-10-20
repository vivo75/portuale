use super::*;

#[test]
fn nonpretend_merge_list_shown_only_when_portage_shows_it() {
    // Portage: actions.py, mergelist_shown branch. Non-pretend shows only with
    // `--ask`/`--tree`/`--verbose`, never under `--quiet` unless `--ask` too.
    // (pretend, ask, tree, verbose, quiet) -> shown
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: true,
        ask: false,
        tree: false,
        verbose: false,
        quiet: false,
        fetching: false,
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: true,
        ask: false,
        tree: false,
        verbose: false,
        quiet: true,
        fetching: false,
    }));
    assert!(!merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: false,
        quiet: false,
        fetching: false,
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: true,
        tree: false,
        verbose: false,
        quiet: false,
        fetching: false,
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: true,
        verbose: false,
        quiet: false,
        fetching: false,
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: true,
        quiet: false,
        fetching: false,
    }));
    assert!(!merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: false,
        quiet: true,
        fetching: false,
    }));
    assert!(!merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: false,
        tree: false,
        verbose: true,
        quiet: true,
        fetching: false,
    }));
    assert!(merge_list_shown(MergeListShownArgs {
        pretend: false,
        ask: true,
        tree: false,
        verbose: false,
        quiet: true,
        fetching: false,
    }));
}

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

/// Plan #351 S7 (351-ABORT): a non-`--pretend` run that aborts on a
/// top-level all-masked target prints the resolution notice on its way
/// out; the `--pretend` arm stays silent (Q6) and every other resolve
/// error shape is out of scope. Oracle: Portage 3.0.82.2 A cells
/// `LLM/evidence/2026-10-15-346-s0/a/` (`a01` and `a02` -p both print; backlog #406).
#[test]
fn top_level_all_masked_abort_prints_the_notice_on_and_off_pretend() {
    // A cell a01's report shape (the `for <root>.` suffix Portage adds
    // to the header line is omitted per the fixture-miss-message
    // convention, but the matched prefix is identical on both sides).
    let masked = "\n!!! All ebuilds that could satisfy \
        \"dev-libs/bareacceptkeywordspkg\" have been masked.\n\
        !!! One of the following masked packages is required to complete your request:\n\
        - dev-libs/bareacceptkeywordspkg-1.0::testrepo (masked by: ~amd64 keyword)\n\
        \n\
        For more information, see the MASKED PACKAGES section in the emerge\n\
        man page or refer to the Gentoo Handbook.\n";
    assert!(top_level_abort_notice_shown(masked));
    // The neighbouring top-level abort shapes keep today's silence.
    assert!(!top_level_abort_notice_shown(
        "there are no ebuilds to satisfy \"dev-libs/foo\"."
    ));
    assert!(!top_level_abort_notice_shown(
        "there are no ebuilds built with USE flags to satisfy \"dev-libs/foo\"."
    ));
}

/// Backlog #453 D: the mid-resolve abort preamble opens with Portage's
/// leading blank line (`_start_resolution_display` writes `"\n" + banner`).
/// Oracle: Portage 3.0.82.2 non-tty capture of `--pretend
/// dev-libs/missingdep` (staged fixtures).
#[test]
fn abort_preamble_opens_with_the_banner_blank_line() {
    let plain = Colorizer::new(false);
    assert_eq!(
        abort_preamble(false, 0, 20, &plain),
        "\nThese are the packages that would be merged, in order:\n\n\
         Calculating dependencies ... done!\n\
         Dependency resolution took (backtrack: 0/20).\n\n"
    );
    assert_eq!(
        abort_preamble(true, 0, 20, &plain),
        "\nThese are the packages that would be merged, in order:\n\n\
         Calculating dependencies ... done!\n"
    );
    // Backlog #453 A: with colour on, the banner is `darkgreen` and the cut
    // seconds keep Portage's empty escape pair (determinism: the seconds
    // themselves are still omitted). Oracle: the `--color=y` pipe capture
    // of `--pretend dev-libs/maskedpkg`.
    let colour = Colorizer::new(true);
    assert_eq!(
        abort_preamble(false, 0, 20, &colour),
        "\n\x1b[32mThese are the packages that would be merged, in order:\x1b[39;49;00m\n\n\
         Calculating dependencies ... done!\n\
         Dependency resolution took \x1b[32m\x1b[39;49;00m s (backtrack: 0/20).\n\n"
    );
}

/// `--pretend --fetchonly` prints URIs instead of the merge list, whatever else is asked;
/// a real fetch with `--ask`/`--verbose` still shows it. Portage: actions.py:541-543.
#[test]
fn pretend_fetchonly_shows_no_merge_list() {
    let args = |pretend, verbose, fetching| MergeListShownArgs {
        pretend,
        ask: false,
        tree: false,
        verbose,
        quiet: false,
        fetching,
    };
    assert!(!merge_list_shown(args(true, false, true)));
    assert!(!merge_list_shown(args(true, true, true)));
    assert!(merge_list_shown(args(false, true, true)));
    assert!(merge_list_shown(args(true, false, false)));
}
