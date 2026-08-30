//! `--debug` merge-order dumps and node-selection traces.

use crate::merge_order::*;

/// The version an entry is resolved at. `None` for `NoVisibleCandidate` and
/// `Uninstall` removals, which are never merge targets.
pub(crate) fn outcome_version(e: &GraphEntry) -> Option<&str> {
    match &e.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::AlreadyInstalled { version } => Some(version),
        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
    }
}
