//! `--root-deps` build entries for the running root.

use crate::*;

/// Metadata and effective USE for `category/package`'s own `version`: the
/// highest-`repo_priority` candidate with that version. `None` when
/// gone/unreadable; callers walk its deps with its own USE.
pub(crate) fn resolved_version_meta_and_use(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    config: &portage_profile::Config,
) -> Option<(HashMap<String, String>, HashSet<String>)> {
    let candidates = list_candidates(repos, category, package).ok()?;
    let resolved = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)?;
    let pf = format!("{package}-{version}");
    let metadata = repo_aux_metadata(&resolved.repo_location, category, &pf).ok()?;
    let (_iuse, use_flags) = candidate_iuse_and_use(resolved, category, package, config)?;
    // Cold path; the shared `Rc` isn't worth threading through.
    Some(((*metadata).clone(), use_flags.as_ref().clone()))
}
