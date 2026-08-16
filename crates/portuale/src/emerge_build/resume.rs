//! `--resume` entries and their repositories.

use crate::emerge_build::*;

/// Arguments of [`resume_entry`].
pub(crate) struct ResumeEntryArgs<'a> {
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) source: CandidateSource,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) binary_repo: Option<&'a str>,
}

/// A minimal source `GraphEntry` for `emerge --resume`: the saved `mtimedb`
/// resume list records only `cat/pkg-ver`, so display/USE/blocker fields are
/// empty and `required_by` is empty (the resume mergelist is already in
/// dependency-first merge order).
///
/// `repo_name` is re-derived from `repos` (highest-priority repo carrying
/// the version); `None` when no repo carries it anymore. For a resumed
/// *binary* entry the caller passes the binary's own repository in
/// `binary_repo`, which wins over the ebuild lookup.
pub(crate) fn resume_entry(args: ResumeEntryArgs<'_>) -> GraphEntry {
    let ResumeEntryArgs {
        category,
        package,
        version,
        source,
        repos,
        binary_repo,
    } = args;
    let repo_name = if source == CandidateSource::Binary {
        binary_repo.map(str::to_string).or_else(|| {
            portage_repo::list_candidates(repos, category, package)
                .ok()
                .and_then(|cs| {
                    cs.iter()
                        .filter(|c| c.version == version)
                        .max_by_key(|c| c.repo_priority)
                        .map(|c| c.repo_name.clone())
                })
        })
    } else {
        portage_repo::list_candidates(repos, category, package)
            .ok()
            .and_then(|cs| {
                cs.iter()
                    .filter(|c| c.version == version)
                    .max_by_key(|c| c.repo_priority)
                    .map(|c| c.repo_name.clone())
            })
    };
    GraphEntry {
        discovery: 0,
        category: category.to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name,
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}
