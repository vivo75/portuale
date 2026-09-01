//! [`MergeEngine`] adapters executing the actual source/binary merges: a
//! source package through the `EbuildBuild` chain, a `"binary"` one through
//! binpkg unpacking. Each [`MergeUnit`] derives from its resolved
//! [`GraphEntry`] via [`merge_unit_for_entry`]; the engine seam owns
//! per-unit execution, not DAG scheduling.
// Portage: MergeListItem.py, _start

use mrg_director::{MergeContext, MergeEngine, MergeKind, MergeOutcome, MergeUnit};
use portage_repo::{CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// One resolved [`GraphEntry`] as a director [`MergeUnit`]. `None` for
/// `AlreadyInstalled` (a silent no-op) and `NoVisibleCandidate` (nothing to
/// merge; the caller reports it).
pub fn merge_unit_for_entry(
    entry: &GraphEntry,
    root: &Path,
    progress: mrg_director::MergeProgress,
) -> Option<MergeUnit> {
    let version = match &entry.outcome {
        // A blocker-removal task is not a merge unit.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => {
            return None;
        }
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
    };
    let kind = match entry.source {
        CandidateSource::Binary => MergeKind::Binary,
        CandidateSource::Ebuild => MergeKind::Source,
    };
    Some(MergeUnit {
        cpv: format!("{}/{}-{version}", entry.category, entry.package),
        kind,
        repo: entry.repo_name.clone(),
        root: root.to_path_buf(),
        replaces_same_slot: None,
        remote_binary: entry.remote_binary,
        build_id: entry.build_id.clone(),
        slot: entry.slot.clone(),
        sub_slot: entry.sub_slot.clone(),
        use_flags: entry.use_flags_display.clone(),
        progress,
    })
}

/// Rebuild the [`GraphEntry`] a merge function needs from a [`MergeUnit`].
/// The outcome is always `New { version }`: the merge functions only read
/// the version out of it, and display-only fields are never consulted
/// mid-merge. `None` when the unit's `cpv` does not parse.
fn rebuild_entry(unit: &MergeUnit, source: CandidateSource) -> Option<GraphEntry> {
    let candidate = portage_dep::parse_candidate(&unit.cpv)?;
    // The `-rN` revision is split off `version`; the merge reads the full
    // `package-version[-rN]`.
    let version = match &candidate.revision {
        Some(r) => format!("{}-r{r}", candidate.version),
        None => candidate.version.clone(),
    };
    Some(GraphEntry {
        discovery: 0,
        category: candidate.category,
        package: candidate.package,
        outcome: PretendOutcome::New { version },
        blockers: Vec::new(),
        slot: unit.slot.clone(),
        sub_slot: unit.sub_slot.clone(),
        repo_name: unit.repo.clone(),
        oldbest: Vec::new(),
        use_flags_display: unit.use_flags.clone(),
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
        remote_binary: unit.remote_binary,
        build_id: unit.build_id.clone(),
        deps: Vec::new(),
    })
}

/// The source-build `MergeEngine` (`emerge_build::merge_one_source_entry`).
pub struct SourceEngine<'a> {
    /// Every repo the merge may read the ebuild from (for
    /// `locate_candidate`).
    pub repos: &'a [RepoConfig],
    /// The `${ROOT}` the engine merges into.
    pub root: &'a Path,
    /// `PORTAGE_TMPDIR`-adjacent build root.
    pub portage_tmpdir: &'a Path,
    /// Merge options (build env, sandbox, `package.env` atoms).
    pub options: &'a crate::ebuild_merge::MergeOptions,
    /// `--buildpkg` packaging, when the entry wants it.
    pub buildpkg: Option<&'a crate::ebuild_package::PackageOptions>,
    /// `--buildpkg-exclude` atoms.
    pub buildpkg_exclude: &'a [String],
    /// The run's status display for the `Emerging`/`Installing`/`Completed` lines.
    pub display: &'a crate::emerge_build::StatusDisplay,
}

impl SourceEngine<'_> {
    /// Merge one resolved entry from source. `progress` is the entry's own
    /// snapshot for the status lines.
    pub fn merge_entry(
        &self,
        entry: &GraphEntry,
        progress: mrg_director::MergeProgress,
    ) -> Result<(), String> {
        let bp = self.buildpkg.filter(|opts| {
            crate::emerge_build::entry_buildpkg_wanted(
                entry,
                self.repos,
                self.buildpkg_exclude,
                opts.buildpkg_live,
            )
        });
        crate::emerge_build::merge_one_source_entry(crate::emerge_build::MergeOneSourceEntryArgs {
            entry,
            repos: self.repos,
            root: self.root,
            portage_tmpdir: self.portage_tmpdir,
            options: self.options,
            buildpkg: bp,
            progress,
            display: self.display,
        })
    }
}

impl MergeEngine for SourceEngine<'_> {
    fn execute(&self, unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
        if unit.kind != MergeKind::Source {
            return MergeOutcome::Failed(format!(
                "SourceEngine cannot merge binary unit {}",
                unit.cpv
            ));
        }
        match rebuild_entry(unit, CandidateSource::Ebuild) {
            Some(entry) => match self.merge_entry(&entry, unit.progress) {
                Ok(()) => MergeOutcome::Merged,
                Err(e) => MergeOutcome::Failed(e),
            },
            None => MergeOutcome::Failed(format!("{}: cannot parse merge unit cpv", unit.cpv)),
        }
    }
}

/// The binary-unpack `MergeEngine` (`emerge_getbinpkg::merge_one_binary_entry`).
pub struct BinaryEngine<'a> {
    /// The resolved binhost configuration (remote fetch + `Packages`).
    pub config: &'a portage_profile::Config,
    /// The `${ROOT}` the engine merges into.
    pub root: &'a Path,
    /// The local `$PKGDIR` store.
    pub pkgdir: &'a Path,
    /// `PORTAGE_TMPDIR`-adjacent build root.
    pub portage_tmpdir: &'a Path,
    /// Merge options.
    pub options: &'a crate::ebuild_merge::MergeOptions,
    /// The run's status display for the `Emerging binary`/`Installing`/`Completed` lines.
    pub display: &'a crate::emerge_build::StatusDisplay,
}

impl BinaryEngine<'_> {
    /// Merge one resolved binary entry. `progress` is the entry's own
    /// snapshot for the status lines.
    pub fn merge_entry(
        &self,
        entry: &GraphEntry,
        progress: mrg_director::MergeProgress,
    ) -> Result<(), String> {
        crate::emerge_getbinpkg::merge_one_binary_entry(
            crate::emerge_getbinpkg::MergeOneBinaryEntryArgs {
                entry,
                config: self.config,
                root: self.root,
                pkgdir: self.pkgdir,
                portage_tmpdir: self.portage_tmpdir,
                merge_options: self.options,
                progress,
                display: self.display,
            },
        )
    }
}

impl MergeEngine for BinaryEngine<'_> {
    fn execute(&self, unit: &MergeUnit, _ctx: &MergeContext) -> MergeOutcome {
        if unit.kind != MergeKind::Binary {
            return MergeOutcome::Failed(format!(
                "BinaryEngine cannot merge source unit {}",
                unit.cpv
            ));
        }
        match rebuild_entry(unit, CandidateSource::Binary) {
            Some(entry) => match self.merge_entry(&entry, unit.progress) {
                Ok(()) => MergeOutcome::Merged,
                Err(e) => MergeOutcome::Failed(e),
            },
            None => MergeOutcome::Failed(format!("{}: cannot parse merge unit cpv", unit.cpv)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_entry() -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: "dev-libs".to_string(),
            package: "example".to_string(),
            outcome: PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            },
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: Some("main".to_string()),
            oldbest: Vec::new(),
            use_flags_display: vec![("flag".to_string(), true)],
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: Vec::new(),
            source: CandidateSource::Ebuild,
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
}
