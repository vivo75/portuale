//! Merge-list row attributes: columns, version decoration, USE suffix, sizes.

use crate::pretend::*;

/// `COLUMNWIDTH` from the environment (default 130); an unparsable value
/// warns and falls back to the default. Only the environment is read, not
/// `make.conf`.
/// Portage: output_helpers.py, MergeListItem
pub(crate) fn columnwidth_from_env() -> i64 {
    match std::env::var("COLUMNWIDTH") {
        Ok(value) => match value.parse::<i64>() {
            Ok(width) => width,
            Err(_) => {
                eprintln!("!!! Unable to parse COLUMNWIDTH={value:?}");
                130
            }
        },
        Err(_) => 130,
    }
}

/// The version `entry`'s own package line displays; `None` for the
/// non-merging outcomes. `merge_order`'s own `entry_version` is the same
/// mapping, but it is private to `portage-repo`.
pub(crate) fn merge_bound_version(entry: &GraphEntry) -> Option<&str> {
    match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        // A removal is not a merge target.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// The version the blocker parents text uses -- the version the owner's own
/// package line shows (for an installed, nomerge owner: the installed
/// version; for an absent-owner row: the removal's own cpv).
/// Portage: output.py, _blockers
pub(crate) fn entry_display_version(entry: &GraphEntry) -> &str {
    merge_bound_version(entry).unwrap_or(match &entry.outcome {
        PretendOutcome::AlreadyInstalled { version } => version,
        // An absent-owner nomerge row rides its own removal entry.
        PretendOutcome::Uninstall { version } => version,
        _ => "",
    })
}
