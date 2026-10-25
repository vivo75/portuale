//! `--resume` entries and their repositories.

use crate::emerge_build::*;

/// This entry's `mtimedb["resume"]` mergelist item, or `None` for an entry
/// that never merges (an installed no-op, a blocker removal, or an entry
/// with no visible candidate). The kind tag matches what `entries_not_merged`
/// writes on the failure path.
// Portage: Scheduler.py, _save_resume_list
pub(crate) fn resume_cpv(entry: &GraphEntry) -> Option<crate::mtimedb::ResumeCpv> {
    let version = entry_version(entry)?;
    let kind = match entry.source {
        CandidateSource::Ebuild => crate::mtimedb::ResumeEntryKind::Ebuild,
        CandidateSource::Binary => crate::mtimedb::ResumeEntryKind::Binary,
    };
    Some((
        kind,
        entry.category.clone(),
        entry.package.clone(),
        version.to_string(),
    ))
}

/// The entry's `::repo` for the progress lines. An empty fallback keeps a
/// would-be `None` loud (a visibly wrong `cpv::`) rather than silently
/// reshaping the line.
pub(crate) fn entry_repo(entry: &GraphEntry) -> &str {
    entry.repo_name.as_deref().unwrap_or("")
}

/// Arguments of [`resume_binary_repo`].
pub(crate) struct ResumeBinaryRepoArgs<'a> {
    pub(crate) local: &'a portage_repo::BinaryIndex,
    pub(crate) binrepos: &'a [portage_profile::BinRepo],
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
}

/// A resumed *binary* entry's `::repo` for the merge progress lines: the
/// *binary's* repository, never the ebuild repo. Mirrors the resolver's
/// pool order (local `$PKGDIR` index first, then remote binhosts); a record
/// without `REPO` yields `"__unknown__"`. `None` when no binary pool lists
/// the version: the caller keeps the ebuild-repo fallback.
// Portage: depgraph.py, _loadResumeCommand
pub(crate) fn resume_binary_repo(args: ResumeBinaryRepoArgs<'_>) -> Option<String> {
    let ResumeBinaryRepoArgs {
        local,
        binrepos,
        root,
        category,
        package,
        version,
    } = args;
    if let Some(cand) = portage_repo::list_binary_candidates(local, category, package)
        .into_iter()
        .find(|c| c.version == version)
    {
        return Some(cand.repo_name);
    }
    let (_, record) = portage_repo::find_remote_binpkg(binrepos, root, category, package, version)?;
    Some(
        record
            .get("REPO")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "__unknown__".to_string()),
    )
}

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
        build_time: None,
        deps: Vec::new(),
    }
}

/// Portage's `PORTAGE_LOG_FILE` resolution: `${T}/build.log` (`${T}` gains
/// the `.gz` suffix under `compress-build-logs`). Takes the resolved
/// `FEATURES` when the caller resolved a config, else the raw process env.
/// Also used by the binary-merge digest-failure path.
pub(crate) fn resolved_features(options: &ebuild_merge::MergeOptions) -> String {
    if options.features.is_empty() {
        std::env::var("FEATURES").unwrap_or_default()
    } else {
        options.features.clone()
    }
}
