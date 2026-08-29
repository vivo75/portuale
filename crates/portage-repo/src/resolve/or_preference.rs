//! Which alternative of an `||` group to prefer.

use crate::*;

/// Arguments of [`alternative_downgrade_demoted`].
pub(crate) struct AlternativeDowngradeDemotedArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) self_cp: &'a RootCp,
    pub(crate) constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) atoms: &'a [String],
    pub(crate) queued: &'a [QueueItem],
    pub(crate) update: bool,
}

/// The `version` field of one `highest_available_candidate_ignoring_use` candidate
/// string: the per-atom seed of an alternative's `cp_map`.
pub(crate) fn candidate_version(
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = candidate_str.strip_prefix(&format!("{category}/{package}-"))?;
    rest.split(':').next().map(str::to_string)
}
