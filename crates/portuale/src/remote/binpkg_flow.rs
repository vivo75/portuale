//! Checking the binary plan and running the binpkg flow.

use crate::remote::*;

// --- Vdb shadow + pre-ship collision pre-check ---------------------------------
///
/// The server reasons about installed-file ownership *before* shipping a
/// unit's tarball: a unit doomed by a foreign-owned collision fails here,
/// with zero client writes. The shadow is the placed vdb read directly
/// (`server:`) or pulled once at plan start (`client:`).
///
/// Deliberately lenient: only ownership by a *different* package fails
/// the pre-check. Same-package ownership (any version/slot) passes --
/// the client driver refines those against the live vdb, or fail-closed
/// without one.
///
// Installed-file ownership: absolute path -> owning `(category, pf)`.
pub(crate) struct VdbShadow {
    pub(crate) owners: std::collections::HashMap<String, (String, String)>,
}

/// `obj`/`sym` paths out of one vdb `CONTENTS` file (Portage
/// `dblink`/`vartree` shape: `obj <path> <md5> <mtime>`,
/// `sym <path> -> <target> <mtime>`). `dir` lines merge freely
/// client-side, so they never gate a pre-check.
pub(crate) fn contents_owned_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("obj"), Some(path)) | (Some("sym"), Some(path)) => Some(path.to_string()),
                _ => None,
            }
        })
        .collect()
}

/// Every merge-bound entry must be `Binary` in a remote plan (the
/// `--getbinpkgonly` resolve guarantees it); anything else is a usage
/// error naming the offenders (exit 2 at the call site).
pub(crate) fn check_binary_plan(entries: &[portage_repo::GraphEntry]) -> Result<(), String> {
    use portage_repo::{CandidateSource, PretendOutcome};
    let mut offenders = Vec::new();
    for entry in entries {
        let merge_bound = !matches!(
            entry.outcome,
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
        );
        if merge_bound && entry.source != CandidateSource::Binary {
            let version = match &entry.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version.clone()
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    to.clone()
                }
                _ => String::new(),
            };
            offenders.push(format!("{}/{}-{version}", entry.category, entry.package));
        }
    }
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "remote plan is not binary-only (needs --getbinpkgonly): {}",
            offenders.join(" ")
        ))
    }
}
