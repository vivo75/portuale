//! Record one installed version for the changed-deps report.

use crate::resolve::pass::*;

/// Record one installed version for the changed-deps report.
pub(crate) fn record_changed_deps_report(
    ctx: &ResolveCtx<'_>,
    outcome: &PretendOutcome,
    key: &(String, String),
    state: &mut PassState,
) {
    if ctx.changed_deps_report && !ctx.changed_deps {
        let installed_version = match &outcome {
            PretendOutcome::AlreadyInstalled { version }
            | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
            _ => None,
        };
        if let Some(version) = installed_version {
            let dedup_key = (key.0.clone(), key.1.clone(), version.clone());
            if state.changed_deps_report_seen.insert(dedup_key)
                && deps_changed(DepsChangedArgs {
                    root: ctx.root,
                    category: &key.0,
                    package: &key.1,
                    version: &version,
                    with_bdeps: ctx.with_bdeps,
                    gate: ctx.gate(),
                })
                && let Ok(repo_candidates) = ctx.repo_candidates(&key.0, &key.1)
                && let Some(c) = repo_candidates.iter().find(|c| c.version == version)
            {
                state
                    .changed_deps_report_entries
                    .push(ChangedDepsReportEntry {
                        category: key.0.clone(),
                        package: key.1.clone(),
                        version,
                        repo_name: c.repo_name.clone(),
                    });
            }
        }
    }
}
