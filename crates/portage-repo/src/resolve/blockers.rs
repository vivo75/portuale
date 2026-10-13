//! Blocker collection and resolution.

use crate::*;
use portage_dep::UseDep;
use std::cell::OnceCell;
use std::ops::ControlFlow;

/// A blocker atom from one package's dependency strings, not yet matched --
/// collected during the BFS and resolved in one post-pass once the whole
/// graph's New/Upgrade set is known, so a match never depends on discovery
/// order.
pub(crate) struct PendingBlocker {
    pub(crate) atom_str: String,
    pub(crate) strong: bool,
    /// Whether the atom came from a build-time key (`DEPEND`/`BDEPEND`).
    /// Under `--buildpkgonly` Portage clears every blocker's uninstall edge
    /// unless it is hard (`!!`) *and* buildtime.
    pub(crate) buildtime: bool,
    pub(crate) target_category: String,
    pub(crate) target_package: String,
    pub(crate) owner_key: (String, String),
    pub(crate) owner_version: String,
    /// Whether the producing owner was walked as a merge. Refined from the
    /// owner's final outcome when the owner is in the graph.
    pub(crate) owner_merging: bool,
    /// Whether this blocker came from the all-installed-packages scan: that
    /// owner is always `nomerge`, so its state must never be overridden from
    /// a same-cp merge entry.
    pub(crate) owner_installed: bool,
}

/// A resolved blocker with the owner version its renderer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiledBlocker {
    pub owner: (String, String),
    pub owner_version: String,
    pub conflict: BlockerConflict,
}

/// An unresolved blocker row whose owner has no display entry anywhere --
/// filed on `GraphResult::orphan_blockers` and printed in the trailing
/// blocker group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanBlocker {
    pub owner_category: String,
    pub owner_package: String,
    pub owner_version: String,
    pub conflict: BlockerConflict,
}

/// Arguments of [`set_seed_holds_instance`].
pub(crate) struct SetSeedHoldsInstanceArgs<'a> {
    pub(crate) set_seed_atoms: &'a [String],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
}

/// Whether a set seed atom selects an installed instance: it matches it and
/// no instance this run merges for the same cp. Entries carry main-slot
/// data only, so sub-slot defaults to the main slot.
pub(crate) fn set_seed_holds_instance(args: SetSeedHoldsInstanceArgs<'_>) -> bool {
    let SetSeedHoldsInstanceArgs {
        set_seed_atoms,
        entries,
        category,
        package,
        version,
        slot,
        sub_slot,
    } = args;
    let candidate = format!("{category}/{package}-{version}:{slot}/{sub_slot}");
    let merged: Vec<String> = entries
        .iter()
        .filter(|e| e.category == category && e.package == package)
        .filter_map(|e| {
            let v = merge_bound_version(&e.outcome)?;
            Some(format!(
                "{category}/{package}-{v}:{}/{}",
                e.slot.as_deref().unwrap_or("0"),
                e.sub_slot.as_deref().or(e.slot.as_deref()).unwrap_or("0")
            ))
        })
        .collect();
    let matches = |atom: &str, cpv: &str| {
        portage_dep::match_from_list(atom, &[cpv]).is_some_and(|m| !m.is_empty())
    };
    set_seed_atoms.iter().any(|a| {
        let Some(parsed) = portage_dep::parse_atom(a) else {
            return false;
        };
        parsed.blocker == portage_dep::Blocker::None
            && parsed.category == category
            && parsed.package == package
            && matches(a, &candidate)
            && !merged.iter().any(|m| matches(a, m))
    })
}

/// [`resolve_blockers_with_seeds`] without set seeds (the solver bridge and
/// the unit tests carry none).
pub(crate) fn resolve_blockers(
    root: &Path,
    pending: &[PendingBlocker],
    entries: &[GraphEntry],
    blocker_retry_closure: &HashSet<(String, String)>,
    buildpkgonly: bool,
) -> Vec<FiledBlocker> {
    resolve_blockers_with_seeds(ResolveBlockersWithSeedsArgs {
        root,
        pending,
        entries,
        blocker_retry_closure,
        buildpkgonly,
        set_seed_atoms: &[],
    })
}

/// Arguments of [`graph_has_parent`].
pub(crate) struct GraphHasParentArgs<'a> {
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) exclude: &'a (String, String),
    pub(crate) entries: &'a [GraphEntry],
    /// Merge-bound slots per cp this run: a merging parent rebinds a bare `:=`
    /// through its own walk to the resolved slot, so its edge no longer
    /// parents an installed instance in another slot.
    pub(crate) merge_slots: &'a HashMap<(String, String), HashSet<String>>,
}

/// Whether some other walked entry depends on `category/package`'s
/// `version:slot/sub_slot` instance. Every outcome counts; `exclude` is the
/// blocker owner itself.
// Portage: depgraph.py, _validate_blockers (digraph parent test)
pub(crate) fn graph_has_parent(args: GraphHasParentArgs<'_>) -> bool {
    let GraphHasParentArgs {
        category,
        package,
        version,
        slot,
        sub_slot,
        exclude,
        entries,
        merge_slots,
    } = args;
    let candidate = format!("{category}/{package}-{version}:{slot}/{sub_slot}");
    entries.iter().any(|e| {
        if (&e.category, &e.package) == (&exclude.0, &exclude.1) {
            return false;
        }
        if e.category == category && e.package == package {
            return false;
        }
        e.deps.iter().any(|d| {
            if d.category != category || d.package != package {
                return false;
            }
            // Rebound edge: this parent merges, its atom is a bare `:=`, and the
            // dep's cp merges in another slot -- no longer a parent here.
            if merge_bound_version(&e.outcome).is_some()
                && let Some(atom) = portage_dep::parse_atom(&d.atom)
                && atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
                && atom.slot.is_none()
                && merge_slots
                    .get(&(d.category.clone(), d.package.clone()))
                    .is_some_and(|slots| !slots.is_empty() && slots.iter().all(|s| s != slot))
            {
                return false;
            }
            portage_dep::match_from_list(&d.atom, &[candidate.as_str()])
                .is_some_and(|m| !m.is_empty())
        })
    })
}

/// The owner-side half of the "a digraph node with parents" test: whether the
/// blocker owner is in the required-set closure. The set is cp-keyed, which
/// is exact for this single-instance arm.
// Portage: depgraph.py, _validate_blockers (step 4)
pub(crate) fn owner_set_parent(
    owner_key: &(String, String),
    blocker_retry_closure: &HashSet<(String, String)>,
) -> bool {
    blocker_retry_closure.contains(owner_key)
}

/// The `GraphEntry` for an installed package a blocker removes: version is
/// the removed installed version, `required_by` the ordering anchors.
/// `deps` stays empty -- the removal has no dependency walk.
// Portage: depgraph.py, _validate_blockers (uninstall task)
pub(crate) fn uninstall_entry(
    root: &Path,
    category: String,
    package: String,
    version: String,
    anchors: Vec<(String, String)>,
) -> GraphEntry {
    // The removed instance's own slot/sub-slot/repo, so `-pv` decorates the
    // row like the vardb-backed uninstall package does.
    let (slot, sub_slot) = read_vdb_slot(root, &category, &package, &version);
    let repo_name = installed_pkg_repo(root, &category, &package, &version);
    GraphEntry {
        discovery: 0,
        category,
        package,
        outcome: PretendOutcome::Uninstall { version },
        blockers: Vec::new(),
        slot: Some(slot),
        sub_slot: Some(sub_slot),
        repo_name: Some(repo_name),
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
        required_by: anchors,
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

/// Working set for `file_blocker_conflicts`: one removed `cpv`, its ordering
/// anchors and the blocker rows that ride it when the owner has no entry.
pub(crate) struct PendingRemoval {
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) anchors: Vec<(String, String)>,
    /// Rows whose owner is absent from `entries`: the removal entry carries
    /// them so they print inline right after its `[uninstall]` row.
    pub(crate) blockers: Vec<BlockerConflict>,
}

/// Files each blocker conflict on its owner entry and appends the `Uninstall`
/// removal entries the satisfied rows describe. One removal entry per removed
/// `cpv`. A conflict whose absent owner resolves by no removal is filed as an
/// orphan; any other absent-owner row is still dropped.
pub(crate) fn file_blocker_conflicts(
    entries: &mut Vec<GraphEntry>,
    root: &Path,
    conflicts: Vec<FiledBlocker>,
    orphans: &mut Vec<OrphanBlocker>,
) {
    let mut removals: Vec<PendingRemoval> = Vec::new();
    for filed in conflicts {
        let FiledBlocker {
            owner: owner_key,
            owner_version,
            conflict,
        } = filed;
        let removal = match &conflict.satisfied_by {
            Some(BlockerSatisfiedBy::Uninstall { cpv, anchor }) => {
                Some((cpv.clone(), anchor.clone()))
            }
            _ => None,
        };
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| (e.category.clone(), e.package.clone()) == owner_key)
        {
            entry.blockers.push(conflict);
        } else if let Some((cpv, _)) = &removal {
            // No owner entry -- attach the row to its removal.
            let Some((category, package, version)) = split_cpv(cpv) else {
                continue;
            };
            if let Some(removal) = removals
                .iter_mut()
                .find(|r| r.category == category && r.package == package && r.version == version)
            {
                removal.blockers.push(conflict);
            } else {
                removals.push(PendingRemoval {
                    category,
                    package,
                    version,
                    anchors: Vec::new(),
                    blockers: vec![conflict],
                });
            }
        } else if conflict.unsolvable {
            // No owner entry and no removal: an unresolved row of a
            // scan-collected owner. Portage still reports it; a `Replacement`
            // row can never land here since it always carries `satisfied_by`.
            orphans.push(OrphanBlocker {
                owner_category: owner_key.0.clone(),
                owner_package: owner_key.1.clone(),
                owner_version,
                conflict,
            });
            continue;
        } else {
            continue;
        }
        let Some((cpv, anchor)) = removal else {
            continue;
        };
        let Some((category, package, version)) = split_cpv(&cpv) else {
            continue;
        };
        match removals
            .iter_mut()
            .find(|r| r.category == category && r.package == package && r.version == version)
        {
            Some(removal) => {
                if !removal.anchors.contains(&anchor) {
                    removal.anchors.push(anchor);
                }
            }
            None => removals.push(PendingRemoval {
                category,
                package,
                version,
                anchors: vec![anchor],
                blockers: Vec::new(),
            }),
        }
    }
    entries.extend(removals.into_iter().map(|r| {
        let mut entry = uninstall_entry(root, r.category, r.package, r.version, r.anchors);
        entry.blockers = r.blockers;
        entry
    }));
}

/// Arguments of [`collect_unwalked_installed_blockers`].
pub(crate) struct CollectUnwalkedInstalledBlockersArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) dynamic_deps: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) disj_constraints: &'a HashMap<(String, String), Vec<String>>,
    /// No `circular_dependency` map: this scan only processes installed
    /// packages whose cp was never walked, while the map is keyed by
    /// merge-bound cycle-node cps only -- the lookup always misses, so the
    /// empty map is exactly faithful here.
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) pending_blockers: &'a mut Vec<PendingBlocker>,
    pub(crate) installed_meta_memo: &'a mut HashMap<(String, String, String, String), String>,
}

/// Pulls in blockers from every installed instance the walk never reached,
/// so they are respected. For each, the recorded runtime deps are flattened
/// and every blocker token becomes a [`PendingBlocker`] owned by that
/// nomerge instance.
// Portage: depgraph.py, _validate_blockers (installed collection)
pub(crate) fn collect_unwalked_installed_blockers(args: CollectUnwalkedInstalledBlockersArgs<'_>) {
    let CollectUnwalkedInstalledBlockersArgs {
        repos,
        root,
        config,
        dynamic_deps,
        ignore_built_slot_operator_deps,
        disj_constraints,
        root_deps_running_root,
        entries,
        pending_blockers,
        installed_meta_memo,
    } = args;
    // With nothing merge-bound the scan cannot contribute a row, so it is
    // skipped: every blocker it produces resolves against merge-bound
    // matches, and neither arm can fire without one.
    if !entries
        .iter()
        .any(|e| merge_bound_version(&e.outcome).is_some())
    {
        return;
    }
    // Hoisted cp-keyed walked test (same test, out of the per-package loop).
    let walked: HashSet<(&str, &str)> = entries
        .iter()
        .map(|e| (e.category.as_str(), e.package.as_str()))
        .collect();
    for pkg in all_installed_packages(root) {
        // Already walked (any outcome): the walk's own blockers are
        // pending, and the cp-keyed owner lookup in `resolve_blockers`
        // ties them to the right entry.
        if walked.contains(&(pkg.category.as_str(), pkg.package.as_str())) {
            continue;
        }
        // The live view comes from the vdb-recorded repo, never a priority search.
        let live =
            live_metadata_for_installed(repos, root, &pkg.category, &pkg.package, &pkg.version);
        let use_flags = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
        let mut depstr = String::new();
        for key in ["IDEPEND", "PDEPEND", "RDEPEND"] {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            depstr.push_str(&installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category: &pkg.category,
                package: &pkg.package,
                version: &pkg.version,
                live: live.as_deref(),
                key,
                layer,
                memo: installed_meta_memo,
            }));
            depstr.push(' ');
        }
        // A dep string with no `!` cannot produce a blocker (`use_reduce` never
        // introduces one), so the reduce is skipped outright.
        if !depstr.contains('!') {
            continue;
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let self_cp = (pkg.category.clone(), pkg.package.clone());
        // The node key for the resolver map (installed owner).
        let self_node: RootCp = (false, pkg.category.clone(), pkg.package.clone());
        let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
            portage_use_reduce::UseReduceFlatDisjunctiveArgs {
                tokens: &tokens,
                uselist: &use_flags,
                mode: portage_use_reduce::MatchMode::Normal, // Installed-scan defaults.
                empty_groups_always_true: false,
                tie_break: &mut |alts: &[Vec<String>]| {
                    promote_tied_alternative(PromoteTiedAlternativeArgs {
                        repos,
                        config,
                        root,
                        entries,
                        constraints: disj_constraints,
                        alts,
                        queued: &[],
                    })
                },
            },
            &mut |atoms: &[String]| {
                disjunction_preference(DisjunctionPreferenceArgs {
                    repos,
                    config,
                    root,
                    entries,
                    self_cp: &self_node,
                    constraints: disj_constraints, // The empty map (see the parameter doc).
                    circular_dependency: &HashMap::new(),
                    root_deps_running_root,
                    atoms,
                    queued: &[],
                    update: false,
                    gate_entries: entries,
                    gate_queued: &[],
                })
            },
        ) else {
            continue;
        };
        for tok in flat_deps {
            if tok == "||" {
                continue;
            }
            if let Some(dep_atom) = portage_dep::parse_atom(&tok)
                && dep_atom.blocker != portage_dep::Blocker::None
            {
                let duplicate = pending_blockers.iter().any(|p| {
                    p.owner_key == self_cp && p.owner_version == pkg.version && p.atom_str == tok
                });
                if !duplicate {
                    pending_blockers.push(PendingBlocker {
                        atom_str: tok,
                        strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                        // The scan reads runtime keys only, never a build-time key.
                        buildtime: false,
                        target_category: dep_atom.category,
                        target_package: dep_atom.package,
                        owner_key: self_cp.clone(),
                        owner_version: pkg.version.clone(),
                        owner_merging: false,
                        owner_installed: true,
                    });
                }
            }
        }
    }
}
/// Arguments of [`resolve_blockers_with_seeds`].
pub(crate) struct ResolveBlockersWithSeedsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) pending: &'a [PendingBlocker],
    pub(crate) entries: &'a [GraphEntry],
    /// `ResolveCtx::blocker_retry_closure`: the required sets' forward-installed
    /// closure. A blocked installed instance reachable from it is a graph node
    /// with the set as parent; the owner-side test reads the same set.
    pub(crate) blocker_retry_closure: &'a HashSet<(String, String)>,
    /// Under `--buildpkgonly`, drops soft and runtime-keyed blockers silently
    /// and keeps only strong-buildtime ones, resolved by uninstall.
    pub(crate) buildpkgonly: bool,
    /// The set seed atoms (`@world`/`@selected` members) Portage adds as
    /// `SetArg` parents in its complete graph; see [`set_seed_holds_instance`].
    pub(crate) set_seed_atoms: &'a [String],
}

pub(crate) fn resolve_blockers_with_seeds(
    args: ResolveBlockersWithSeedsArgs<'_>,
) -> Vec<FiledBlocker> {
    let ResolveBlockersWithSeedsArgs {
        root,
        pending,
        entries,
        blocker_retry_closure,
        buildpkgonly,
        set_seed_atoms,
    } = args;
    let mut conflicts = Vec::new();
    // cp -> merge-bound slots this run: a cp re-resolved into another slot
    // leaves its old instance parentless (merging parents rebind live `:=`).
    let merge_slots: HashMap<(String, String), HashSet<String>> = {
        let mut m: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for e in entries {
            if merge_bound_version(&e.outcome).is_some()
                && let Some(slot) = e.slot.clone()
            {
                m.entry((e.category.clone(), e.package.clone()))
                    .or_default()
                    .insert(slot);
            }
        }
        m
    };
    // Lazily built seed extension: the recorded atoms of required-set-reached
    // installed packages the walk never visited (never from a cp this run
    // merges). One vdb scan, only when a blocker reaches that test.
    let hold_atoms: std::cell::OnceCell<Vec<String>> = std::cell::OnceCell::new();
    for pb in pending {
        // Under `--buildpkgonly` a blocker that is not strong-buildtime never
        // pairs an uninstall.
        if buildpkgonly && !(pb.strong && pb.buildtime) {
            continue;
        }
        let target_key = (pb.target_category.clone(), pb.target_package.clone());
        let installed_matches = installed_candidates(root, &pb.target_category, &pb.target_package);
        let mut candidates = installed_matches.clone();
        for entry in entries {
            if entry.category != pb.target_category || entry.package != pb.target_package {
                continue;
            }
            let version = match &entry.outcome {
                PretendOutcome::New { version } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            let (Some(version), Some(slot)) = (version, entry.slot.clone()) else {
                continue;
            };
            if !candidates
                .iter()
                .any(|(v, s, _ss)| *v == version && *s == slot)
            {
                candidates.push((version, slot.clone(), slot));
            }
        }
        let candidate_strs: Vec<String> = candidates
            .iter()
            .map(|(v, s, ss)| format!("{}/{}-{v}:{s}/{ss}", pb.target_category, pb.target_package))
            .collect();
        let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
        let Some(matched) = portage_dep::match_from_list(&pb.atom_str, &refs) else {
            continue;
        };
        let by_str: HashMap<&str, &(String, String, String)> = candidate_strs
            .iter()
            .map(String::as_str)
            .zip(candidates.iter())
            .collect();
        // The blocker atom's own `[use]` deps must match the blocked package:
        // `match_from_list` sees only a cpv string, so a satisfied no-op
        // blocker is dropped against recorded (installed) / effective USE.
        let blocker_use_deps: Vec<portage_dep::UseDep> = portage_dep::parse_atom(&pb.atom_str)
            .and_then(|a| a.use_deps)
            .unwrap_or_default();
        // An installed instance replaced in its own slot by a merge-bound entry
        // at another version resolves to a `Replacement` row, kept for the
        // stuck-serialization display and hidden otherwise.
        'm: for m in matched {
            if classify_blocker_match(ClassifyBlockerMatchArgs {
                by_str: &by_str,
                m,
                blocker_use_deps: &blocker_use_deps,
                entries,
                pb,
                root,
                installed_matches: &installed_matches,
                conflicts: &mut conflicts,
                target_key: target_key.clone(),
                merge_slots: &merge_slots,
                blocker_retry_closure,
                hold_atoms: &hold_atoms,
                set_seed_atoms,
            })
            .is_break()
            {
                continue 'm;
            }
        }
    }
    conflicts
}

/// The values [`classify_blocker_match`] reads from its caller.
pub(crate) struct ClassifyBlockerMatchArgs<'a> {
    pub(crate) by_str: &'a HashMap<&'a str, &'a (String, String, String)>,
    pub(crate) m: &'a str,
    pub(crate) blocker_use_deps: &'a [UseDep],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) pb: &'a PendingBlocker,
    pub(crate) root: &'a Path,
    pub(crate) installed_matches: &'a [(String, String, String)],
    pub(crate) conflicts: &'a mut Vec<FiledBlocker>,
    pub(crate) target_key: (String, String),
    pub(crate) merge_slots: &'a HashMap<(String, String), HashSet<String>>,
    pub(crate) blocker_retry_closure: &'a HashSet<(String, String)>,
    pub(crate) hold_atoms: &'a OnceCell<Vec<String>>,
    pub(crate) set_seed_atoms: &'a [String],
}

/// File a conflict for one installed or merge-bound match of a blocker, unless the match is harmless.
pub(crate) fn classify_blocker_match(args: ClassifyBlockerMatchArgs<'_>) -> ControlFlow<()> {
    let ClassifyBlockerMatchArgs {
        by_str,
        m,
        blocker_use_deps,
        entries,
        pb,
        root,
        installed_matches,
        conflicts,
        target_key,
        merge_slots,
        blocker_retry_closure,
        hold_atoms,
        set_seed_atoms,
    } = args;
    let Some((version, slot, sub_slot)) = by_str.get(m).copied() else {
        return ControlFlow::Break(());
    };
    if !use_deps_apply(version, blocker_use_deps, entries, pb, root) {
        return ControlFlow::Break(());
    }
    let installed_match = installed_matches
        .iter()
        .any(|(v, s, _ss)| v == version && s == slot);
    let merge_bound_match = entries.iter().any(|e| {
        e.category == pb.target_category
            && e.package == pb.target_package
            && e.slot.as_deref() == Some(slot.as_str())
            && merge_bound_version(&e.outcome).is_some_and(|v2| v2 == version)
    });
    let replaced_in_slot = installed_match
        && entries.iter().any(|e| {
            e.category == pb.target_category
                && e.package == pb.target_package
                && e.slot.as_deref() == Some(slot.as_str())
                && merge_bound_version(&e.outcome).is_some_and(|v2| v2 != version)
        });
    if replaced_in_slot {
        conflicts.push(FiledBlocker {
            owner: pb.owner_key.clone(),
            owner_version: pb.owner_version.clone(),
            conflict: BlockerConflict {
                atom_str: pb.atom_str.clone(),
                strong: pb.strong,
                matched_category: pb.target_category.clone(),
                matched_package: pb.target_package.clone(),
                matched_version: version.clone(),
                unsolvable: false,
                satisfied_by: Some(BlockerSatisfiedBy::Replacement {
                    cp: (pb.target_category.clone(), pb.target_package.clone()),
                    slot: slot.clone(),
                }),
                tree_scheduled_uninstall: false,
            },
        });
        return ControlFlow::Break(());
    }
    // The owner entry decides `owner_merging`: a merge outcome merges, an
    // installed node is `nomerge`; the producer bit is only a fallback
    // for an off-graph owner. A scan-sourced owner is always `nomerge`.
    // The owner entry is matched by merged version, not just cp: a
    // merge-bound cp entry at another version means the declaring
    // instance lost and its blockers are stale.
    let owner_entry = entries.iter().find(|e| {
        (e.category.as_str(), e.package.as_str())
            == (pb.owner_key.0.as_str(), pb.owner_key.1.as_str())
            && merge_bound_version(&e.outcome).is_some_and(|v| v == &pb.owner_version)
    });
    if !pb.owner_installed
        && owner_entry.is_none()
        && entries.iter().any(|e| {
            (e.category.as_str(), e.package.as_str())
                == (pb.owner_key.0.as_str(), pb.owner_key.1.as_str())
                && merge_bound_version(&e.outcome).is_some()
        })
    {
        return ControlFlow::Break(());
    }
    // Fallback for owners with no merge-bound entry: a walked
    // non-merge node (same cp, no merge version), else the
    // producer bit (the bridge path).
    let owner_entry = owner_entry.or_else(|| {
        entries.iter().find(|e| {
            (e.category.as_str(), e.package.as_str())
                == (pb.owner_key.0.as_str(), pb.owner_key.1.as_str())
                && merge_bound_version(&e.outcome).is_none()
        })
    });
    let owner_merging = if pb.owner_installed {
        false
    } else {
        owner_entry
            .map(|e| merge_bound_version(&e.outcome).is_some())
            .unwrap_or(pb.owner_merging)
    };
    // The owner instance's own slot, from its entry when walked,
    // from the vdb when the scan produced it (the owner may have
    // no entry at all).
    let owner_slot = if pb.owner_installed {
        Some(read_vdb_slot(root, &pb.owner_key.0, &pb.owner_key.1, &pb.owner_version).0)
    } else {
        owner_entry.and_then(|e| e.slot.clone())
    };
    // Soft same-slot skip: a soft blocker whose target slot is the
    // owner's own slot; a strong `!!` blocker is exempt.
    if !pb.strong && target_key == pb.owner_key && owner_slot.as_deref() == Some(slot.as_str()) {
        return ControlFlow::Break(());
    }
    // An installed parent's installed-only matches are ignored outright.
    if !owner_merging && installed_match && !merge_bound_match {
        return ControlFlow::Break(());
    }
    let unsolvable = blocker_unsolvable(BlockerUnsolvableArgs {
        owner_merging,
        merge_slots,
        target_key: &target_key,
        slot,
        merge_bound_match,
        installed_match,
        pb,
        version,
        sub_slot,
        entries,
        blocker_retry_closure,
        hold_atoms,
        set_seed_atoms,
        root,
        owner_entry,
    });
    conflicts.push(FiledBlocker {
        owner: pb.owner_key.clone(),
        owner_version: pb.owner_version.clone(),
        conflict: BlockerConflict {
            atom_str: pb.atom_str.clone(),
            strong: pb.strong,
            matched_category: pb.target_category.clone(),
            matched_package: pb.target_package.clone(),
            matched_version: version.clone(),
            // A satisfied merging-owner row removes the matched installed
            // instance; a satisfied nomerge-owner row removes the owner
            // itself, ordered after the merge-bound blocked instance.
            satisfied_by: if unsolvable {
                None
            } else if owner_merging {
                Some(BlockerSatisfiedBy::Uninstall {
                    cpv: format!("{}/{}-{version}", pb.target_category, pb.target_package),
                    anchor: pb.owner_key.clone(),
                })
            } else {
                Some(BlockerSatisfiedBy::Uninstall {
                    cpv: format!("{}/{}-{}", pb.owner_key.0, pb.owner_key.1, pb.owner_version),
                    anchor: (pb.target_category.clone(), pb.target_package.clone()),
                })
            },
            unsolvable,
            tree_scheduled_uninstall: false,
        },
    });
    ControlFlow::Continue(())
}

/// The values [`blocker_unsolvable`] reads from its caller.
pub(crate) struct BlockerUnsolvableArgs<'a> {
    pub(crate) owner_merging: bool,
    pub(crate) merge_slots: &'a HashMap<(String, String), HashSet<String>>,
    pub(crate) target_key: &'a (String, String),
    pub(crate) slot: &'a String,
    pub(crate) merge_bound_match: bool,
    pub(crate) installed_match: bool,
    pub(crate) pb: &'a PendingBlocker,
    pub(crate) version: &'a String,
    pub(crate) sub_slot: &'a String,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) blocker_retry_closure: &'a HashSet<(String, String)>,
    pub(crate) hold_atoms: &'a OnceCell<Vec<String>>,
    pub(crate) set_seed_atoms: &'a [String],
    pub(crate) root: &'a Path,
    pub(crate) owner_entry: Option<&'a GraphEntry>,
}

/// Whether the blocker stays unresolved: the instance to unmerge is a walked node with parents.
pub(crate) fn blocker_unsolvable(args: BlockerUnsolvableArgs<'_>) -> bool {
    let BlockerUnsolvableArgs {
        owner_merging,
        merge_slots,
        target_key,
        slot,
        merge_bound_match,
        installed_match,
        pb,
        version,
        sub_slot,
        entries,
        blocker_retry_closure,
        hold_atoms,
        set_seed_atoms,
        root,
        owner_entry,
    } = args;
    // The tracker match is uninstall-ordered; it stays unresolved only
    // when the instance to unmerge is a walked node with parents.

    if owner_merging {
        // A merge-bound match with a merging parent is unresolved
        // outright.
        //
        // The closure veto lifts when the run re-resolves this cp into
        // another slot -- the old instance is parentless then; a cp
        // that does not move keeps the veto.
        let cp_moves_slot = merge_slots
            .get(target_key)
            .is_some_and(|slots| slots.iter().any(|s| s != slot));
        merge_bound_match
            || (installed_match
                && (graph_has_parent(GraphHasParentArgs {
                    category: &pb.target_category,
                    package: &pb.target_package,
                    version,
                    slot,
                    sub_slot,
                    exclude: &pb.owner_key,
                    entries,
                    merge_slots,
                }) || (blocker_retry_closure.contains(target_key) && !cp_moves_slot)
                    || set_seed_holds_instance(SetSeedHoldsInstanceArgs {
                        set_seed_atoms: hold_seeds(
                            hold_atoms,
                            entries,
                            set_seed_atoms,
                            root,
                            blocker_retry_closure,
                        ),
                        entries,
                        category: &pb.target_category,
                        package: &pb.target_package,
                        version,
                        slot,
                        sub_slot,
                    })))
    } else {
        // A nomerge parent is uninstall-ordered; it is unresolved when
        // the *owner* is a node with parents: a walked entry depends
        // on it, or it is in the required sets' parent membership. A
        // scan-sourced owner has no entry, so its slot comes from
        // the vdb.
        let owner_has_parent = if pb.owner_installed {
            let (slot, sub_slot) =
                read_vdb_slot(root, &pb.owner_key.0, &pb.owner_key.1, &pb.owner_version);
            graph_has_parent(GraphHasParentArgs {
                category: &pb.owner_key.0,
                package: &pb.owner_key.1,
                version: &pb.owner_version,
                slot: &slot,
                sub_slot: &sub_slot,
                exclude: &pb.owner_key,
                entries,
                merge_slots,
            })
        } else {
            owner_entry.is_some_and(|e| {
                graph_has_parent(GraphHasParentArgs {
                    category: &pb.owner_key.0,
                    package: &pb.owner_key.1,
                    version: &pb.owner_version,
                    slot: e.slot.as_deref().unwrap_or_default(),
                    sub_slot: e.sub_slot.as_deref().unwrap_or_default(),
                    exclude: &pb.owner_key,
                    entries,
                    merge_slots,
                })
            })
        };
        merge_bound_match
            && (owner_has_parent || owner_set_parent(&pb.owner_key, blocker_retry_closure))
    }
}

/// The recorded atoms of required-set-reached installed packages the walk never
/// visited, built once (one vdb scan) the first time a blocker reaches that test.
fn hold_seeds<'a>(
    hold_atoms: &'a std::cell::OnceCell<Vec<String>>,
    entries: &[GraphEntry],
    set_seed_atoms: &[String],
    root: &Path,
    blocker_retry_closure: &HashSet<(String, String)>,
) -> &'a Vec<String> {
    hold_atoms.get_or_init(|| {
        let merge_bound: HashSet<(String, String)> = entries
            .iter()
            .filter(|e| merge_bound_version(&e.outcome).is_some())
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect();
        let mut atoms: Vec<String> = set_seed_atoms.to_vec();
        for pkg in all_installed_packages(root) {
            let cp = (pkg.category.clone(), pkg.package.clone());
            if !blocker_retry_closure.contains(&cp) || merge_bound.contains(&cp) {
                continue;
            }
            let use_flags =
                read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
            for key in ["RDEPEND", "PDEPEND", "IDEPEND"] {
                let dep = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key);
                if let Some(found) = flat_dep_atoms(&dep, &use_flags) {
                    atoms.extend(found);
                }
            }
        }
        atoms
    })
}

/// Whether the blocker atom's own `[use]` deps match the blocked version.
fn use_deps_apply(
    version: &str,
    blocker_use_deps: &[portage_dep::UseDep],
    entries: &[GraphEntry],
    pb: &PendingBlocker,
    root: &Path,
) -> bool {
    if blocker_use_deps.is_empty() {
        return true;
    }
    let (enabled, iuse) = entries
        .iter()
        .find(|e| {
            e.category == pb.target_category
                && e.package == pb.target_package
                && matches!(&e.outcome,
                    PretendOutcome::New { version: v }
                    | PretendOutcome::Reinstall { version: v, .. } if v == version)
                || (e.category == pb.target_category
                    && e.package == pb.target_package
                    && matches!(&e.outcome,
                        PretendOutcome::Upgrade { to, .. }
                        | PretendOutcome::Downgrade { to, .. } if to == version))
        })
        .map(|e| {
            let en: HashSet<String> = e
                .use_flags_display
                .iter()
                .filter(|(_, on)| *on)
                .map(|(f, _)| f.clone())
                .collect();
            let iu: HashSet<String> = e.use_flags_display.iter().map(|(f, _)| f.clone()).collect();
            (en, iu)
        })
        .unwrap_or_else(|| {
            (
                read_vdb_flag_set(
                    root,
                    &pb.target_category,
                    &pb.target_package,
                    version,
                    "USE",
                ),
                read_vdb_flag_set(
                    root,
                    &pb.target_category,
                    &pb.target_package,
                    version,
                    "IUSE",
                ),
            )
        });
    portage_dep::use_deps_satisfied(blocker_use_deps, &iuse, &enabled)
}
