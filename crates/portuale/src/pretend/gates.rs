//! Small predicates deciding what a run shows.

/// Whether a non-`--pretend` run with display flags and an empty plan prints
/// `Nothing to merge; quitting.` and exits 0 before the resume rotation
/// and the scheduler. A `--pretend` run never takes this arm.
// Portage: _emerge/actions.py, action_build
pub(crate) fn nothing_to_merge(pretend: bool, show_merge_list: bool, mergecount: usize) -> bool {
    !pretend && show_merge_list && mergecount == 0
}
