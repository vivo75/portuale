//! `--resume` entries and their repositories.

use crate::emerge_build::*;

/// This entry's own `mtimedb["resume"]` mergelist item (`ResumeCpv`), or
/// `None` for an entry real's `_save_resume_list` would never record
/// (`Scheduler.py:2414-2418` only keeps `Package`s with
/// `operation == "merge"` -- an already-installed no-op, a blocker
/// removal, or an entry with no visible candidate merges nothing). The
/// `ResumeEntryKind` tag comes straight from the resolved source, the
/// same tag `entries_not_merged` (`pretend.rs`) already writes on the
/// failure path -- so the up-front save and the per-merge shrink below
/// name every entry identically.
pub(crate) fn resume_cpv(entry: &GraphEntry) -> Option<crate::mtimedb::ResumeCpv> {
    let version = entry_version(&entry.outcome)?;
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

/// The entry's `::repo` for the progress lines: `repo_name` is always
/// `Some` for a merging entry (`GraphEntry::repo_name`'s own doc
/// comment); an empty fallback keeps a would-be `None` loud (a visibly
/// wrong `cpv::`) rather than silently reshaping the line.
pub(crate) fn entry_repo(entry: &GraphEntry) -> &str {
    entry.repo_name.as_deref().unwrap_or("")
}

/// A resumed *binary* entry's `::repo` for the merge progress lines.
/// Real re-resolves a `["binary", root, cpv, "merge"]` resume item from
/// its bintree (`depgraph.py::_loadResumeCommand` matches
/// `pkg_type == "binary"` against the binary tree only), so the resumed
/// Package -- and the `(N of M) cpv::repo` line (`PackageMerge._make_msg`,
/// `PackageMerge.py:25`, `pkg.cpv + _repo_separator + pkg.repo`) --
/// carries the *binary's* repository (the `Packages` index `REPO` field
/// real `bintree` surfaces as the package's `repository`), never the
/// ebuild repo. Mirrors the resolver's own pool order: the local
/// `$PKGDIR` index first (real `bintree.isremote` prefers a local file),
/// then the configured remote binhosts in order (`find_remote_binpkg`).
/// A record without `REPO` yields `"__unknown__"` -- real
/// `portage.versions._unknown_repo`, and exactly what
/// `binary_candidates_from_index` (hence a fresh binary merge) reports
/// for the same record. `None` when no binary pool lists the version at
/// all: the caller keeps the ebuild-repo fallback (today's display; the
/// merge itself then reports it cannot locate the entry, same as a
/// non-resume run).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn resume_binary_repo(
    local: &portage_repo::BinaryIndex,
    binrepos: &[portage_profile::BinRepo],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<String> {
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

/// A minimal source `GraphEntry` for `emerge --resume` (`pretend.rs`):
/// the saved `mtimedb` resume list only records `cat/pkg-ver`, so the
/// display/USE/blocker fields are all empty. `required_by` is empty too --
/// the resume mergelist is already in dependency-first merge order, so
/// `run_merge_loop`'s `--keep-going` dependent-drop has nothing to key on
/// (and `--resume` doesn't pass `--keep-going` anyway). `source` is the
/// resumed mergelist entry's own recorded `ResumeEntryKind` (real's own
/// `type` tag): `CandidateSource::Binary` always resolves from the local
/// `$PKGDIR` (`remote_binary: false`) -- see `mtimedb.rs`'s own module
/// doc comment for why re-deriving "was this fetched remotely" isn't
/// attempted.
///
/// `repo_name` is re-derived from `repos` (the highest-priority repo
/// carrying this exact version -- the same tie-break
/// `locate_candidate` uses), because the merge progress lines print
/// real's `cpv::repo` shape (backlog #177) and the saved resume list
/// records only `cat/pkg-ver`. `None` when no repo carries the version
/// anymore (the merge then reports it cannot locate the ebuild, same as
/// a non-resume run). For a resumed *binary* entry the caller passes the
/// binary's own repository (`resume_binary_repo`, real's bintree
/// re-resolution, backlog #186) in `binary_repo`, which wins over the
/// ebuild lookup; `None` keeps the ebuild lookup as the fallback.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn resume_entry(
    category: &str,
    package: &str,
    version: &str,
    source: CandidateSource,
    repos: &[RepoConfig],
    binary_repo: Option<&str>,
) -> GraphEntry {
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

/// Real portage's `PORTAGE_LOG_FILE` (`PORTAGE_LOGDIR` unset →
/// `${T}/build.log`, i.e. `${PORTAGE_BUILDDIR}/temp/build.log`; with
/// `FEATURES=compress-build-logs`, `${T}/build.log.gz` --
/// `prepare_build_dirs.py`'s own `f"build.log{compress_log_ext}"`).
/// The resolved `FEATURES` list for build-path decisions: `MergeOptions::
/// features` when the caller resolved a config (#37 S2), else the raw
/// process env (standalone `ebuild <file>` / tests, where `features` is
/// empty). The same precedence `ebuild_phases::features_string` uses for
/// the phase-execution gates.
///
/// `pub(crate)` for the binary-merge digest-failure path
/// (`emerge_getbinpkg`): real `Binpkg` prepares the same builddir before
/// `BinpkgVerifier` runs, so the failure log lives at this same path.
pub(crate) fn resolved_features(options: &ebuild_merge::MergeOptions) -> String {
    if options.features.is_empty() {
        std::env::var("FEATURES").unwrap_or_default()
    } else {
        options.features.clone()
    }
}
