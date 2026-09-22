//! Small predicates deciding what a run shows.

/// Whether a non-`--pretend` run with display flags and an empty plan prints
/// `Nothing to merge; quitting.` and exits 0 before the resume rotation
/// and the scheduler. A `--pretend` run never takes this arm.
// Portage: _emerge/actions.py, action_build
pub(crate) fn nothing_to_merge(pretend: bool, show_merge_list: bool, mergecount: usize) -> bool {
    !pretend && show_merge_list && mergecount == 0
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
