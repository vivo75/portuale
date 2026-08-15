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

/// One Portage `--autounmask` change: the implicit `package.accept_keywords` /
/// `package.use` line plus its own `#required by …` dep-chain comment.
// Portage: depgraph.py, _display_autounmask; _get_dep_chain_as_comment
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutounmaskChange {
    /// The left-hand atom of the change line, op prefix included: `=<cpv>` for a
    /// keyword change, `>=<cpv>` / `>=<cpv>:<slot>` / `=<cpv>` for a USE change.
    // Portage: autounmask_unrestricted_atoms default n; check_if_latest
    pub atom: String,
    /// The token(s) to add after `atom`: a `~<arch>` keyword, or a space-joined
    /// `flag`/`-flag` list for a USE change.
    pub token: String,
    /// The `#required by …` comment lines, top-to-bottom, WITHOUT the leading `# `
    /// (the renderer adds it): `required by <cpv>::<repo>` for a package parent,
    /// `required by <atom> (argument)` for a command-line argument.
    // Portage: _get_dep_chain_as_comment
    pub dep_chain: Vec<String>,
    /// The `(category, package)` whose atom mismatch forced this USE flip. `None`
    /// for keyword/license/mask changes (rendered without the unsatisfied filter) and
    /// for flips whose trigger is the change owner itself (parent-flip rescues). The
    /// post-loop fill starts the chain there instead of at the first requirer; a stale
    /// trigger falls back to the legacy start. Travels with the record across
    /// backtrack passes (it names the flip site, which the re-resolve reproduces).
    // Portage: _get_dep_chain unsatisfied_dependency first parent
    pub trigger: Option<(String, String)>,
}
