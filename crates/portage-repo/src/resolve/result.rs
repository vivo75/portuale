//! The resolver's result and how a run aborts.

use crate::*;

/// `--changed-deps-report`: an installed package, still in the graph at `version`,
/// whose vdb-recorded dependency strings differ from the repo's current ebuild for
/// that exact version -- but reported, not reinstalled. `repo_name` is the repo that
/// currently provides `version`, standing in for Portage's `pkg.repo` (no vdb
/// `REPOSITORY` reader; Portage's `if pkg.repo != ebuild.repo: continue` filter
/// requires equality first, so nothing is lost).
// Portage: depgraph.py, _changed_deps_report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedDepsReportEntry {
    pub category: String,
    pub package: String,
    pub version: String,
    pub repo_name: String,
}
