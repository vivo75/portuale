//! Real `MergeEngine` adapters: the director seam executing the actual
//! source/binary merges.
//!
//! Grounding: real `_emerge/MergeListItem.py::_start` dispatches on
//! `pkg.type_name` -- a source package through the `EbuildBuild` chain,
//! a `"binary"` one through `_emerge/Binpkg.py`. The `mrg-director` crate
//! pins that kind routing (`SourceMergeEngine`/`BinaryMergeEngine`: own
//! kind `Skipped`, other kind `Failed`); these adapters are the
//! binary-crate halves that actually build and merge. Each carries the
//! same context its underlying merge function already threads (repos,
//! root, tmpdir, options, binhost config, `$PKGDIR`), and each
//! [`MergeUnit`] is derived from its resolved [`GraphEntry`] by
//! [`merge_unit_for_entry`] -- the unit carries every merge-relevant
//! field (version, kind, repo, slot/sub-slot and resolved USE for the
//! build-phase env, `remote_binary`/`build_id` for binpkg location), so
//! rebuilding the entry inside `execute` loses nothing the merge
//! functions read.
//!
//! Live paths: `emerge_getbinpkg::run_merge_plan` executes every entry's
//! unit through one of these engines (mixed source+binary merge), and
//! `emerge_build::run_source_merge`'s serial loop merges through
//! [`SourceEngine::merge_entry`]. The `-jN` scheduler keeps calling its
//! own build-then-merge split directly (build-half concurrency plus a
//! serialized vdb merge, like real portage) -- the engine seam owns
//! per-unit execution, not DAG scheduling.

use mrg_director::{MergeContext, MergeEngine, MergeKind, MergeOutcome, MergeUnit};
use portage_repo::{CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// One resolved [`GraphEntry`] as a director [`MergeUnit`]: the merge
/// version out of the outcome, the kind out of `entry.source`, the repo,
/// slot/sub-slot, resolved USE, and binary location out of the entry.
/// `None` for `AlreadyInstalled` (a silent no-op, like the merge
/// functions' own early return) and `NoVisibleCandidate` (nothing to
/// merge -- the caller reports it, same as before).
pub fn merge_unit_for_entry(entry: &GraphEntry, root: &Path) -> Option<MergeUnit> {
    let version = match &entry.outcome {
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate => {
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
    })
}

/// Rebuild the [`GraphEntry`] a merge function needs from a [`MergeUnit`].
/// The outcome is always `New { version }`: the merge functions only ever
/// read the version out of it (`New`/`Reinstall` share the `version`
/// arm, `Upgrade`/`Downgrade` the `to` arm), and display-only fields
/// (blockers, oldbest, `required_by`, counters) are never consulted
/// mid-merge. `None` when the unit's `cpv` does not parse.
fn rebuild_entry(unit: &MergeUnit, source: CandidateSource) -> Option<GraphEntry> {
    let candidate = portage_dep::parse_candidate(&unit.cpv)?;
    // `parse_candidate` splits the `-rN` revision off `version` (real
    // `catpkgsplit`); the merge reads the full `package-version[-rN]`.
    let version = match &candidate.revision {
        Some(r) => format!("{}-r{r}", candidate.version),
        None => candidate.version.clone(),
    };
    Some(GraphEntry {
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

/// The source-build `MergeEngine`: real `MergeListItem`'s `"ebuild"` arm
/// (`emerge_build::merge_one_source_entry` -- phase chain + vdb write).
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
}

impl SourceEngine<'_> {
    /// Merge one resolved entry from source (the `Source`-entry arm of
    /// the mixed dispatcher, and `run_source_merge`'s serial loop).
    pub fn merge_entry(&self, entry: &GraphEntry) -> Result<(), String> {
        let bp = self.buildpkg.filter(|opts| {
            crate::emerge_build::entry_buildpkg_wanted(
                entry,
                self.repos,
                self.buildpkg_exclude,
                opts.buildpkg_live,
            )
        });
        crate::emerge_build::merge_one_source_entry(
            entry,
            self.repos,
            self.root,
            self.portage_tmpdir,
            self.options,
            bp,
        )
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
            Some(entry) => match self.merge_entry(&entry) {
                Ok(()) => MergeOutcome::Merged,
                Err(e) => MergeOutcome::Failed(e),
            },
            None => MergeOutcome::Failed(format!("{}: cannot parse merge unit cpv", unit.cpv)),
        }
    }
}

/// The binary-unpack `MergeEngine`: real `MergeListItem`'s `"binary"` arm
/// (`emerge_getbinpkg::merge_one_binary_entry` -- locate/fetch + unpack +
/// vdb write).
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
}

impl BinaryEngine<'_> {
    /// Merge one resolved binary entry (the `Binary`-entry arm of the
    /// mixed dispatcher).
    pub fn merge_entry(&self, entry: &GraphEntry) -> Result<(), String> {
        crate::emerge_getbinpkg::merge_one_binary_entry(
            entry,
            self.config,
            self.root,
            self.pkgdir,
            self.portage_tmpdir,
            self.options,
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
            Some(entry) => match self.merge_entry(&entry) {
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

    /// `merge_unit_for_entry` derives the unit the director executes:
    /// version out of the outcome (`to` for an upgrade), kind out of the
    /// source, slot/USE/repo carried for the build-phase env; installed
    /// and unresolvable entries map to "nothing to merge".
    #[test]
    fn merge_unit_for_entry_carries_everything_the_merge_reads() {
        let root = Path::new("/root");
        let unit = merge_unit_for_entry(&test_entry(), root).unwrap();
        assert_eq!(unit.cpv, "dev-libs/example-2.0");
        assert_eq!(unit.kind, MergeKind::Source);
        assert_eq!(unit.repo.as_deref(), Some("main"));
        assert_eq!(unit.slot.as_deref(), Some("0"));
        assert_eq!(unit.use_flags, vec![("flag".to_string(), true)],);
        assert!(!unit.remote_binary);

        let mut installed = test_entry();
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "2.0".to_string(),
        };
        assert!(merge_unit_for_entry(&installed, root).is_none());
        let mut unresolvable = test_entry();
        unresolvable.outcome = PretendOutcome::NoVisibleCandidate;
        assert!(merge_unit_for_entry(&unresolvable, root).is_none());

        // A binary entry keeps its location bits (remote fetch, build
        // id) on the unit, so the binary engine can locate it.
        let mut binary = test_entry();
        binary.source = CandidateSource::Binary;
        binary.remote_binary = true;
        binary.build_id = Some("3".to_string());
        let unit = merge_unit_for_entry(&binary, root).unwrap();
        assert_eq!(unit.kind, MergeKind::Binary);
        assert!(unit.remote_binary);
        assert_eq!(unit.build_id.as_deref(), Some("3"));

        // A `-rN` revision survives the unit round-trip (real
        // `catpkgsplit` splits it off; the merge reads it back whole).
        let mut revised = test_entry();
        revised.outcome = PretendOutcome::New {
            version: "1.2-r1".to_string(),
        };
        let unit = merge_unit_for_entry(&revised, root).unwrap();
        assert_eq!(unit.cpv, "dev-libs/example-1.2-r1");
        let rebuilt = rebuild_entry(&unit, CandidateSource::Ebuild).unwrap();
        assert_eq!(
            rebuilt.outcome,
            PretendOutcome::New {
                version: "1.2-r1".to_string()
            }
        );
    }

    /// The adapters refuse the other kind (the same `type_name` routing
    /// real `MergeListItem._start` performs), and report an unparseable
    /// unit as failed rather than panicking. Real merging itself is
    /// covered by the existing `run_merge_plan` end-to-end tests.
    #[test]
    fn engines_refuse_the_other_kind_and_a_bad_cpv() {
        let ctx = MergeContext {
            root: PathBuf::from("/root"),
            builddir: PathBuf::from("/var/tmp/portage"),
            jobs: 1,
            keep_going: false,
        };
        let repos: Vec<RepoConfig> = Vec::new();
        let options = crate::ebuild_merge::MergeOptions::default();
        let source = SourceEngine {
            repos: &repos,
            root: Path::new("/root"),
            portage_tmpdir: Path::new("/var/tmp/portage"),
            options: &options,
            buildpkg: None,
            buildpkg_exclude: &[],
        };
        let binary_unit = MergeUnit::binary("dev-libs/example-1.0", Path::new("/root"));
        assert!(matches!(
            source.execute(&binary_unit, &ctx),
            MergeOutcome::Failed(_)
        ));
        let bad = MergeUnit::source("not-a-cpv", Path::new("/root"));
        assert!(matches!(
            source.execute(&bad, &ctx),
            MergeOutcome::Failed(_)
        ));

        let config = portage_profile::Config::default();
        let binary = BinaryEngine {
            config: &config,
            root: Path::new("/root"),
            pkgdir: Path::new("/var/cache/binpkgs"),
            portage_tmpdir: Path::new("/var/tmp/portage"),
            options: &options,
        };
        let source_unit = MergeUnit::source("dev-libs/example-1.0", Path::new("/root"));
        assert!(matches!(
            binary.execute(&source_unit, &ctx),
            MergeOutcome::Failed(_)
        ));
    }
}
