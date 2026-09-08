//! The remote entry points: resolve, plan, and run each unit.

use crate::remote::*;

// --- Resolve-path support ----------------------------------------------------------

/// Repo position for one merged entry: `(repo name, commit hash)` from
/// the repo checkout the entry resolved from (`unknown` when the repo is
/// not git or git is unavailable).
pub(crate) fn repo_position(
    repos: &[portage_repo::RepoConfig],
    repo_name: &Option<String>,
) -> (String, String) {
    let name = repo_name
        .clone()
        .unwrap_or_else(|| "__unknown__".to_string());
    let commit = repos
        .iter()
        .find(|repo| repo.name == name)
        .and_then(|repo| {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(&repo.location)
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|hash| hash.trim().to_string())
                .filter(|hash| !hash.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    (name, commit)
}

/// BFS-drop the transitive dependents of a failed package (Portage
/// `_calc_resume_list`): every skipped cp records the failed cpv that
/// doomed it, for the `skipped (… failed)` trailer.
pub(crate) fn drop_dependents(
    dependents: &std::collections::HashMap<(String, String), Vec<(String, String)>>,
    skip: &mut std::collections::HashMap<(String, String), String>,
    failed_cp: (String, String),
    failed_cpv: &str,
) {
    let mut queue = vec![failed_cp];
    while let Some(x) = queue.pop() {
        if let Some(deps) = dependents.get(&x) {
            for p in deps {
                if skip.insert(p.clone(), failed_cpv.to_string()).is_none() {
                    queue.push(p.clone());
                }
            }
        }
    }
}

/// Arguments of [`run_remote_plan`].
pub(crate) struct RunRemotePlanArgs<'a> {
    pub(crate) entries: &'a [portage_repo::GraphEntry],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a std::path::Path,
    pub(crate) pkgdir: &'a std::path::Path,
    pub(crate) portage_tmpdir: &'a std::path::Path,
    pub(crate) ctx: &'a RemoteContext,
    pub(crate) keep_going: bool,
}

/// Arguments of [`run_one_remote_unit`].
pub(crate) struct RunOneRemoteUnitArgs<'a> {
    pub(crate) entry: &'a portage_repo::GraphEntry,
    pub(crate) version: &'a str,
    pub(crate) cpv: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a std::path::Path,
    pub(crate) pkgdir: &'a std::path::Path,
    pub(crate) ctx: &'a RemoteContext,
    pub(crate) control: Option<&'a std::path::Path>,
    pub(crate) shadow: &'a VdbShadow,
    pub(crate) server_ledger_base: &'a std::path::Path,
    pub(crate) install_mask: &'a str,
    pub(crate) install_mask_prunes_usr_share: bool,
    pub(crate) regen_features: Option<&'a str>,
    pub(crate) regen_bzip2: Option<&'a str>,
}
