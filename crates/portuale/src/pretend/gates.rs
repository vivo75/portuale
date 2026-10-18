//! Small predicates deciding what a run shows.

/// Arguments of [`merge_list_shown`].
pub(crate) struct MergeListShownArgs {
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) tree: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
}

/// Whether the merge list prints: always under `--pretend`; otherwise only
/// with `--ask`/`--tree`/`--verbose`, and never under `--quiet` without `--ask`.
// Portage: _emerge/actions.py, action_build
pub(crate) fn merge_list_shown(args: MergeListShownArgs) -> bool {
    let MergeListShownArgs {
        pretend,
        ask,
        tree,
        verbose,
        quiet,
    } = args;
    pretend || ((ask || tree || verbose) && !(quiet && !ask))
}

/// Arguments of [`autounmask_only_reshows_merge_list`].
pub(crate) struct AutounmaskOnlyReshowsMergeListArgs {
    pub(crate) pretend: bool,
    pub(crate) autounmask_only: bool,
    pub(crate) has_changes: bool,
}

/// Whether `--autounmask-only` re-shows the merge list: only a `--pretend`
/// run that has changes to report does; an empty change set prints nothing.
// Portage: _emerge/depgraph.py, _display_autounmask
pub(crate) fn autounmask_only_reshows_merge_list(args: AutounmaskOnlyReshowsMergeListArgs) -> bool {
    let AutounmaskOnlyReshowsMergeListArgs {
        pretend,
        autounmask_only,
        has_changes,
    } = args;
    pretend && autounmask_only && has_changes
}

/// Whether a non-`--pretend` run with display flags and an empty plan prints
/// `Nothing to merge; quitting.` and exits 0 before the resume rotation
/// and the scheduler. A `--pretend` run never takes this arm.
// Portage: _emerge/actions.py, action_build
pub(crate) fn nothing_to_merge(pretend: bool, show_merge_list: bool, mergecount: usize) -> bool {
    !pretend && show_merge_list && mergecount == 0
}

/// Arguments of [`selective_noop_deferred`].
pub(crate) struct SelectiveNoopDeferredArgs {
    pub(crate) selective: bool,
    pub(crate) oneshot_or_onlydeps: bool,
    pub(crate) has_world_candidates: bool,
}

/// Whether an empty plan skips the quit message and still rotates the resume
/// list and records world favorites: a selective run without
/// `--oneshot`/`--onlydeps` that has a world-candidate favorite. Only
/// `Scheduler` is skipped.
// Portage: _emerge/actions.py, action_build
pub(crate) fn selective_noop_deferred(args: SelectiveNoopDeferredArgs) -> bool {
    let SelectiveNoopDeferredArgs {
        selective,
        oneshot_or_onlydeps,
        has_world_candidates,
    } = args;
    selective && !oneshot_or_onlydeps && has_world_candidates
}

/// Arguments of [`resolution_header_shown`].
pub(crate) struct ResolutionHeaderShownArgs {
    pub(crate) ask: bool,
    pub(crate) tree: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
}

/// Whether the `These are the packages that would be ...` header prints:
/// `--quiet` absent and one of `--ask`/`--tree`/`--verbose` given.
/// `--nodeps` is not consulted.
// Portage: _emerge/depgraph.py, _start_resolution_display
pub(crate) fn resolution_header_shown(args: ResolutionHeaderShownArgs) -> bool {
    let ResolutionHeaderShownArgs {
        ask,
        tree,
        verbose,
        quiet,
    } = args;
    !quiet && (ask || tree || verbose)
}

/// Whether the `Calculating dependencies` notice prints: neither `--quiet`
/// nor `--nodeps` given (either forces the spinner into QUIET mode, which
/// also suppresses the `... done!` completion and the timing report).
// Portage: _emerge/depgraph.py, backtrack_depgraph
pub(crate) fn resolution_notice_shown(quiet: bool, nodeps: bool) -> bool {
    !quiet && !nodeps
}

/// Whether a resolve-phase error prints the resolution notice on its way
/// out: only the top-level all-masked abort of a non-`--pretend` run does.
/// The `--pretend` arm stays silent (plan Q6); every other error shape is
/// out of scope.
// Portage: _emerge/depgraph.py, _start_resolution_display
pub(crate) fn top_level_abort_notice_shown(pretend: bool, msg: &str) -> bool {
    !pretend && portage_repo::is_all_masked_abort_report(msg)
}
