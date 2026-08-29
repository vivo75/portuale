//! The counts and summary lines under the merge list, and the redisplay before a merge.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The notice for targets satisfied by package.provided.
pub(crate) fn show_pprovided_notice(result: &GraphResult, color: &Colorizer) {
    // Printed to stderr, before the merge list. The "pulled in by" ref is
    // always `'args'` (no `SetArg` tracking); the `@world`/`@selected`
    // solution text is never reached (see `GraphEntry::pprovided_atoms`).
    // Portage: _emerge/depgraph.py, display_problems
    if !result.pprovided_atoms.is_empty() {
        eprint!("{}", color.c("BAD", "\nWARNING: "));
        if result.pprovided_atoms.len() > 1 {
            eprintln!("Requested packages will not be merged because they are listed in");
        } else {
            eprintln!("A requested package will not be merged because it is listed in");
        }
        eprintln!("package.provided:\n");
        for atom in &result.pprovided_atoms {
            eprintln!("  {} pulled in by 'args'", color.c("INFORM", atom));
        }
        eprintln!();
    }
}
