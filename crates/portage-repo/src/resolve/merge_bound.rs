//! Lookups over the entries a run will merge.

use crate::*;

/// The version a merge-bound outcome would install, or `None` for the outcomes
/// that merge nothing. Exactly the four outcomes `PassState::resolved_slots` indexes.
pub(crate) fn merge_bound_version(outcome: &PretendOutcome) -> Option<&String> {
    match outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// The `category/package-version` a merge-bound entry would install, or `None` for
/// entries that never participate in a build-time cycle.
pub(crate) fn merge_bound_cpv(entry: &GraphEntry) -> Option<String> {
    let version = merge_bound_version(&entry.outcome)?;
    Some(format!("{}/{}-{version}", entry.category, entry.package))
}
