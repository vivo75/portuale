//! `--list-sets` and `--search`.

use crate::pretend::*;

/// The highest-version candidate, a version tie broken toward the
/// higher-priority repo: `run_search`'s "Latest version" pick.
/// Portage: portdbapi.xmatch
pub(crate) fn search_best_candidate(
    cands: &[portage_repo::Candidate],
) -> Option<portage_repo::Candidate> {
    cands
        .iter()
        .max_by(|a, b| {
            portage_versions::vercmp(&a.version, &b.version)
                .unwrap_or(0)
                .cmp(&0)
                .then_with(|| a.repo_priority.cmp(&b.repo_priority))
        })
        .cloned()
}
