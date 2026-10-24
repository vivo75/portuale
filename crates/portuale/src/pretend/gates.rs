//! Small predicates deciding what a run shows.

/// Arguments of [`merge_list_shown`].
pub(crate) struct MergeListShownArgs {
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) tree: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    /// `--fetchonly`/`--fetch-all-uri`.
    pub(crate) fetching: bool,
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
        fetching,
    } = args;
    // `--pretend --fetchonly` lists URIs instead of the merge list (actions.py:541-543).
    if pretend && fetching {
        return false;
    }
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

/// Arguments of [`pretend_changes_show_header`].
pub(crate) struct PretendChangesShowHeaderArgs {
    pub(crate) pretend: bool,
    pub(crate) json: bool,
    pub(crate) complete: bool,
    /// The merge list is displayed at all (`--autounmask-only` without
    /// changes prints nothing, header included).
    pub(crate) list_shown: bool,
}

/// Whether a `--pretend` run whose resolve completed prints the
/// resolution header before the rows: Portage's `display()` (header +
/// rows) runs for every completed `--pretend` resolve, with or without
/// autounmask changes (backlog #462, #466). `--json` keeps stdout
/// machine-readable (no header); an aborted resolve keeps its own preamble
/// arms (backlog #406, the abort-preamble shape).
// Portage: _emerge/depgraph.py, _display_autounmask (_show_merge_list)
pub(crate) fn pretend_changes_show_header(args: PretendChangesShowHeaderArgs) -> bool {
    let PretendChangesShowHeaderArgs {
        pretend,
        json,
        complete,
        list_shown,
    } = args;
    pretend && !json && complete && list_shown
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
/// out: the top-level all-masked abort does, `--pretend` or not (backlog
/// #406, owner 2026-10-19: Portage prints it in both). Every other error
/// shape stays silent (out of scope).
// Portage: _emerge/depgraph.py, _start_resolution_display
pub(crate) fn top_level_abort_notice_shown(msg: &str) -> bool {
    portage_repo::is_all_masked_abort_report(msg) || is_plain_miss_report(msg)
}

/// Whether `msg` is a REQUIRED_USE unmet-requirements report (`!!! The
/// ebuild selected to satisfy ... has unmet requirements.` plus the
/// `(dependency required by ...)` chain): like every other `--pretend`
/// run that reaches resolution, Portage prints the preamble first
/// (backlog #519).
// Portage: _emerge/depgraph.py, _start_resolution_display
pub(crate) fn is_required_use_report(msg: &str) -> bool {
    msg.starts_with("\n!!! The ebuild selected to satisfy \"")
        && msg.contains("has unmet requirements.")
}

/// A resolve error whose first line is Portage's `there are no ebuilds (or
/// binary packages) to satisfy "<atom>"`: it prints the resolution notice
/// first too, and a blank line ahead of the `emerge:` line (backlog #470).
pub(crate) fn is_plain_miss_report(msg: &str) -> bool {
    msg.starts_with("there are no ebuilds to satisfy \"")
        || msg.starts_with("there are no binary packages to satisfy \"")
}
