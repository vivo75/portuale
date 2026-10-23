//! Slot conflicts: records, merging and rendering.

use crate::*;
use portage_profile::Config;
use std::ops::ControlFlow;

/// A genuine slot conflict: two different atoms landing on the same
/// category/package/slot whose independently-resolved best versions are
/// incompatible. Purely informational ("report, don't enforce"): unlike
/// Portage's fatal depgraph error, portuale reports it and keeps the
/// first-resolved version. Different slots coexisting is not a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflict {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The version already resolved (and recursed into), by whichever atom
    /// reached this slot first.
    pub resolved_version: String,
    /// The atom text that does NOT accept `resolved_version`.
    pub conflicting_atom: String,
    /// Every distinct version pulled into the graph (>= 2) with its parents;
    /// `instances[0]` is always `resolved_version`.
    pub instances: Vec<SlotConflictInstance>,
}

/// One version of a slot-conflicted `category/package:slot`, plus its pullers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictInstance {
    pub version: String,
    pub sub_slot: String,
    pub repo_name: String,
    /// The `USE="…"` pairs for this instance's header line (empty renders
    /// `USE=""`); vdb-based for an installed instance.
    pub use_display: Vec<(String, String)>,
    pub parents: Vec<SlotConflictParent>,
    /// Portage's installed nomerge node: kept by a residual consumer pin,
    /// rendered `(… installed in '<root>')`. Always false for merge-vs-merge.
    pub installed: bool,
}

/// One parent that pulled a `SlotConflictInstance` into the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictParent {
    /// `cat/pkg-ver:slot/sub_slot::repo`, or empty for a top-level atom.
    pub parent_cpv: String,
    pub atom: String,
    /// The parent's own resolved `USE="…"` for its `required by` line; empty
    /// for a top-level `(Argument)` parent, vdb-based for an installed one.
    pub use_display: Vec<(String, String)>,
    /// An installed parent whose recorded atom the merge breaks.
    pub installed: bool,
}

/// `(sub_slot, repo_name, slot)` for `cat/pkg`'s `version`: the highest-priority
/// candidate with that version; all-empty when versionless.
pub(crate) fn slot_conflict_meta(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> (String, String, String) {
    match list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .cloned()
        }) {
        Some(c) => (c.sub_slot, c.repo_name, c.slot),
        None => (String::new(), String::new(), String::new()),
    }
}

/// Which conflict party is installed rather than merge-bound: `Existing` is
/// instance A (already in the graph), `Current` instance B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstalledSide {
    Existing,
    Current,
}

/// Arguments of [`build_slot_conflict`].
pub(crate) struct BuildSlotConflictArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) existing_version: &'a str,
    pub(crate) current_atom: &'a str,
    pub(crate) current_version: &'a str,
    pub(crate) slot_pullers: &'a SlotPullers,
    pub(crate) installed_side: Option<InstalledSide>,
}

/// Assembles a `SlotConflict`: instance A is `existing_version` (already in the
/// graph), instance B is `current_version`. Each puller files under whichever
/// instance its atom fully satisfies (version+slot+USE; under A when both).
/// `installed_side` names an installed-nomerge party, whose metadata comes
/// from the vdb instead of the repo; `None` for merge-vs-merge.
// Portage: depgraph.py, slot_collision_handler
pub(crate) fn build_slot_conflict(args: BuildSlotConflictArgs<'_>) -> SlotConflict {
    let BuildSlotConflictArgs {
        repos,
        config,
        root,
        category,
        package,
        slot,
        existing_version,
        current_atom,
        current_version,
        slot_pullers,
        installed_side,
    } = args;
    let a_installed = installed_side == Some(InstalledSide::Existing);
    let b_installed = installed_side == Some(InstalledSide::Current);
    // An installed instance's vdb `(sub_slot, repo)` may differ from the tree's.
    let vdb_meta = |version: &str| -> (String, String) {
        installed_refs(root, category, package)
            .into_iter()
            .find(|r| r.version == version)
            .map(|r| (r.sub_slot, r.repo))
            .unwrap_or_default()
    };
    let (a_sub, a_repo) = if a_installed {
        vdb_meta(existing_version)
    } else {
        let (s, r, _) = slot_conflict_meta(repos, category, package, existing_version);
        (s, r)
    };
    let (b_sub, b_repo) = if b_installed {
        vdb_meta(current_version)
    } else {
        let (s, r, _) = slot_conflict_meta(repos, category, package, current_version);
        (s, r)
    };
    // Match strings carry the sub-slot, or built slot-operator pullers vanish.
    let with_sub = |v: &str, sub: &str| {
        if sub.is_empty() {
            format!("{category}/{package}-{v}:{slot}")
        } else {
            format!("{category}/{package}-{v}:{slot}/{sub}")
        }
    };
    let a_match = with_sub(existing_version, &a_sub);
    let b_match = with_sub(current_version, &b_sub);
    // A puller files under the instance its atom *fully* satisfies.
    let (a_iuse, a_use) = if a_installed {
        installed_pkg_iuse_and_use(root, category, package, existing_version)
    } else {
        slot_conflict_flag_sets(repos, config, category, package, existing_version)
    };
    let (b_iuse, b_use) = if b_installed {
        installed_pkg_iuse_and_use(root, category, package, current_version)
    } else {
        slot_conflict_flag_sets(repos, config, category, package, current_version)
    };
    let mut parents_a: Vec<SlotConflictParent> = Vec::new();
    let mut parents_b: Vec<SlotConflictParent> = Vec::new();
    if let Some(pullers) = slot_pullers.get(&(category.to_string(), package.to_string())) {
        for (pc, pp, pv, atom) in pullers {
            let parsed = parse_dep_atom(atom, "slot_conflict");
            let use_ok = |iuse: &HashSet<String>, use_: &HashSet<String>| match &parsed {
                Some(a) => match &a.use_deps {
                    Some(ud) => portage_dep::use_deps_satisfied(ud, iuse, use_),
                    None => true,
                },
                // Unparseable atoms file by string match.
                None => true,
            };
            let hits_a = portage_dep::match_from_list(atom, &[a_match.as_str()])
                .is_some_and(|m| !m.is_empty())
                && use_ok(&a_iuse, &a_use);
            let hits_b = portage_dep::match_from_list(atom, &[b_match.as_str()])
                .is_some_and(|m| !m.is_empty())
                && use_ok(&b_iuse, &b_use);
            let entry = SlotConflictParent {
                parent_cpv: slot_conflict_puller_cpv(repos, pc, pp, pv),
                atom: atom.clone(),
                use_display: if pc.is_empty() {
                    Vec::new()
                } else {
                    pkg_use_display_for(repos, config, pc, pp, pv)
                },
                installed: false,
            };
            if hits_a {
                if !parents_a.contains(&entry) {
                    parents_a.push(entry);
                }
            } else if hits_b && !parents_b.contains(&entry) {
                parents_b.push(entry);
            }
        }
    }
    SlotConflict {
        category: category.to_string(),
        package: package.to_string(),
        slot: slot.to_string(),
        resolved_version: existing_version.to_string(),
        conflicting_atom: current_atom.to_string(),
        instances: vec![
            SlotConflictInstance {
                version: existing_version.to_string(),
                sub_slot: a_sub,
                repo_name: a_repo,
                use_display: if a_installed {
                    installed_use_display_for(root, config, category, package, existing_version)
                } else {
                    pkg_use_display_for(repos, config, category, package, existing_version)
                },
                parents: parents_a,
                installed: a_installed,
            },
            SlotConflictInstance {
                version: current_version.to_string(),
                sub_slot: b_sub,
                repo_name: b_repo,
                use_display: if b_installed {
                    installed_use_display_for(root, config, category, package, current_version)
                } else {
                    pkg_use_display_for(repos, config, category, package, current_version)
                },
                parents: parents_b,
                installed: b_installed,
            },
        ],
    }
}

/// `` `cat/pkg-ver:slot/sub_slot::repo` `` for a conflicting parent; empty for
/// a top-level parent. Shared by both conflict report builders.
pub(crate) fn slot_conflict_puller_cpv(
    repos: &[RepoConfig],
    pc: &str,
    pp: &str,
    pv: &str,
) -> String {
    if pc.is_empty() {
        return String::new();
    }
    let (psub, prepo, pslot) = slot_conflict_meta(repos, pc, pp, pv);
    format!("{pc}/{pp}-{pv}:{pslot}/{psub}::{prepo}")
}

/// Records a fresh `SlotConflict`, replacing the existing record for the same
/// `(category, package, slot, existing, current)` triple.
pub(crate) fn record_slot_conflict(slot_conflicts: &mut Vec<SlotConflict>, sc: SlotConflict) {
    if let Some(prev) = slot_conflicts.iter_mut().find(|p| {
        p.category == sc.category
            && p.package == sc.package
            && p.slot == sc.slot
            && p.resolved_version == sc.resolved_version
            && p.instances.get(1).map(|i| &i.version) == sc.instances.get(1).map(|i| &i.version)
    }) {
        *prev = sc;
    } else {
        slot_conflicts.push(sc);
    }
}

/// Arguments of [`merge_same_slot_conflicts`].
pub(crate) struct MergeSameSlotConflictsArgs<'a> {
    pub(crate) conflicts: &'a [SlotConflict],
    pub(crate) pullers: &'a SlotPullers,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) refile_singletons: bool,
}

/// Merges records sharing one `(category, package, slot)` into a single joint
/// record before the direct solve, so the solver sees the whole contention at
/// once. Parents re-file from the live pullers under *every* instance the atom
/// fully satisfies. Singleton groups pass through untouched (unless
/// `refile_singletons`); a merged record inherits the first record's display
/// fields.
// Portage: depgraph.py, slot_collision_handler (per-slot handler)
pub(crate) fn merge_same_slot_conflicts(args: MergeSameSlotConflictsArgs<'_>) -> Vec<SlotConflict> {
    let MergeSameSlotConflictsArgs {
        conflicts,
        pullers,
        repos,
        config,
        root,
        refile_singletons,
    } = args;
    let mut order: Vec<(String, String, String)> = Vec::new();
    let mut groups: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    for (idx, c) in conflicts.iter().enumerate() {
        let key = (c.category.clone(), c.package.clone(), c.slot.clone());
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(idx);
    }
    let mut merged = Vec::with_capacity(conflicts.len());
    for key in order {
        let idxs = &groups[&key];
        if idxs.len() == 1 && !refile_singletons {
            merged.push(conflicts[idxs[0]].clone());
            continue;
        }
        // Ordered instance union by (version, installed); the first
        // record's display payload wins per instance.
        let mut instances: Vec<SlotConflictInstance> = Vec::new();
        for &idx in idxs {
            for inst in &conflicts[idx].instances {
                if !instances
                    .iter()
                    .any(|e| e.version == inst.version && e.installed == inst.installed)
                {
                    instances.push(inst.clone());
                }
            }
        }
        let (category, package, slot) = (key.0.as_str(), key.1.as_str(), key.2.as_str());
        let mut parents_per_inst: Vec<Vec<SlotConflictParent>> = vec![Vec::new(); instances.len()];
        if let Some(cp_pullers) = pullers.get(&(key.0.clone(), key.1.clone())) {
            for (pc, pp, pv, atom) in cp_pullers {
                let parsed = parse_dep_atom(atom, "slot_conflict");
                for (ii, inst) in instances.iter().enumerate() {
                    // Same probe as `direct_solve_atom_matches` (tree slot/sub-slot,
                    // USE against resolved flags); installed instances use vdb.
                    let sub = if inst.installed {
                        inst.sub_slot.clone()
                    } else {
                        slot_conflict_meta(repos, category, package, &inst.version).0
                    };
                    let match_str = if sub.is_empty() {
                        format!("{category}/{package}-{}:{slot}", inst.version)
                    } else {
                        format!("{category}/{package}-{}:{slot}/{sub}", inst.version)
                    };
                    let (iuse, use_) = if inst.installed {
                        installed_pkg_iuse_and_use(root, category, package, &inst.version)
                    } else {
                        slot_conflict_flag_sets(repos, config, category, package, &inst.version)
                    };
                    // A puller files under the instance its atom *fully* satisfies.
                    let use_ok = match &parsed {
                        Some(a) => match &a.use_deps {
                            Some(ud) => portage_dep::use_deps_satisfied(ud, &iuse, &use_),
                            None => true,
                        },
                        None => true,
                    };
                    if !use_ok {
                        continue;
                    }
                    if portage_dep::match_from_list(atom, &[match_str.as_str()])
                        .is_some_and(|m| !m.is_empty())
                    {
                        let entry = SlotConflictParent {
                            parent_cpv: slot_conflict_puller_cpv(repos, pc, pp, pv),
                            atom: atom.clone(),
                            use_display: if pc.is_empty() {
                                Vec::new()
                            } else {
                                pkg_use_display_for(repos, config, pc, pp, pv)
                            },
                            installed: false,
                        };
                        if !parents_per_inst[ii].contains(&entry) {
                            parents_per_inst[ii].push(entry);
                        }
                    }
                }
            }
        }
        for (ii, inst) in instances.iter_mut().enumerate() {
            inst.parents = std::mem::take(&mut parents_per_inst[ii]);
        }
        merged.push(SlotConflict {
            category: key.0.clone(),
            package: key.1.clone(),
            slot: key.2.clone(),
            resolved_version: conflicts[idxs[0]].resolved_version.clone(),
            conflicting_atom: conflicts[idxs[0]].conflicting_atom.clone(),
            instances,
        });
    }
    merged
}

/// Arguments of [`synthesize_surviving_conflict_entries`].
pub(crate) struct SynthesizeSurvivingConflictEntriesArgs<'a> {
    pub(crate) entries: &'a mut Vec<GraphEntry>,
    pub(crate) conflicts: &'a [SlotConflict],
    pub(crate) required_by_map: &'a HashMap<(String, String), HashSet<(String, String)>>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
}

/// Graphs the shadowed second instances of *surviving* slot conflicts beside
/// their records, so every conflicting instance gets a merge row like
/// Portage's. Runs after the direct solve over its survivors only, so solved
/// cells are untouched. A synthesized entry is display-complete but walk-light:
/// slot/sub-slot/repo/USE from the tree reads, `required_by` from the sibling
/// union, scheduler edges through the USE-narrowed match. Its own deps,
/// blockers and row decorations are deliberately not carried over.
pub(crate) fn synthesize_surviving_conflict_entries(
    args: SynthesizeSurvivingConflictEntriesArgs<'_>,
) {
    let SynthesizeSurvivingConflictEntriesArgs {
        entries,
        conflicts,
        required_by_map,
        repos,
        root,
        config,
    } = args;
    for sc in conflicts {
        for inst in &sc.instances {
            if inst.installed {
                continue;
            }
            let already = entries.iter().any(|e| {
                e.category == sc.category
                    && e.package == sc.package
                    && merge_bound_version(&e.outcome).is_some_and(|v| v == &inst.version)
            });
            if already {
                continue;
            }
            let owners: Vec<(String, String)> = required_by_map
                .get(&(sc.category.clone(), sc.package.clone()))
                .map(|set| {
                    let mut owners: Vec<(String, String)> = set.iter().cloned().collect();
                    owners.sort();
                    owners
                })
                .unwrap_or_default();
            let mut entry = GraphEntry {
                discovery: 0,
                category: sc.category.clone(),
                package: sc.package.clone(),
                outcome: PretendOutcome::New {
                    version: inst.version.clone(),
                },
                blockers: Vec::new(),
                slot: None,
                sub_slot: None,
                repo_name: None,
                oldbest: Vec::new(),
                use_flags_display: Vec::new(),
                use_expand_display: Vec::new(),
                use_expand_display_p: Vec::new(),
                keyword_mask: None,
                new_slot: !installed_candidates(root, &sc.category, &sc.package).is_empty(),
                interactive: false,
                fetch_restrict: false,
                fetch_restrict_satisfied: false,
                download_files: Vec::new(),
                required_by: owners,
                source: CandidateSource::Ebuild,
                provenance: VisibilityProvenance::default(),
                keyword_suggestion: None,
                use_suggestion: None,
                parent_use_suggestion: None,
                targets_running_root: false,
                remote_binary: false,
                build_id: None,
                build_time: None,
                deps: Vec::new(),
            };
            refresh_entry_use_display(
                std::slice::from_mut(&mut entry),
                repos,
                root,
                &(sc.category.clone(), sc.package.clone()),
                config,
            );
            // File beside its siblings: conflicting instances print adjacently,
            // and discovery order settles scheduler ties.
            let at = entries
                .iter()
                .rposition(|e| {
                    e.category == sc.category
                        && e.package == sc.package
                        && merge_bound_version(&e.outcome).is_some()
                })
                .map(|i| i + 1)
                .unwrap_or(entries.len());
            entries.insert(at, entry);
        }
    }
}

/// Arguments of [`build_residual_slot_conflicts`].
pub(crate) struct BuildResidualSlotConflictsArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) slot_pullers: &'a SlotPullers,
    pub(crate) dropped: &'a [RevDepPin],
}

/// Residual installed-instance slot conflicts for dropped reverse-dep pins:
/// for every dropped pin whose consumer still names an installed version the
/// final merge-bound version fails, one record pairing the merge instance
/// (parents: this pass's hard pullers) against the installed instance
/// (parents: hard pullers matching it, plus every dropped consumer pin for
/// it). A pin the final version satisfies after all, one matching no
/// installed version, or one in a different slot than the merge yields no
/// record. One record per `(package, merge version, installed version)`.
/// Sorted for determinism.
// Portage: depgraph.py, _complete_graph (end-of-walk unsatisfied-dep loop)
pub(crate) fn build_residual_slot_conflicts(
    args: BuildResidualSlotConflictsArgs<'_>,
) -> Vec<SlotConflict> {
    let BuildResidualSlotConflictsArgs {
        repos,
        config,
        root,
        entries,
        slot_pullers,
        dropped,
    } = args;
    let mut grouped: HashMap<(String, String, String, String, String), SlotConflict> =
        HashMap::new();
    let mut order: Vec<(String, String, String, String, String)> = Vec::new();
    // `dropped` accumulates across passes: skip pins whose consumer is
    // merge-bound in the final graph.
    let replaced: HashSet<(&str, &str)> = entries
        .iter()
        .filter(|e| merge_bound_cpv(e).is_some())
        .map(|e| (e.category.as_str(), e.package.as_str()))
        .collect();
    let dropped: Vec<&RevDepPin> = dropped
        .iter()
        .filter(|p| !replaced.contains(&(p.consumer.0.as_str(), p.consumer.1.as_str())))
        .collect();
    'pin: for pin in dropped.iter().copied() {
        if residual_conflict_for_pin(ResidualConflictForPinArgs {
            pin,
            entries,
            repos,
            config,
            root,
            grouped: &mut grouped,
            slot_pullers,
            order: &mut order,
        })
        .is_break()
        {
            continue 'pin;
        }
    }
    // Fold every dropped consumer pin into the installed instance's parents.
    for pin in dropped.iter().copied() {
        for key in order.iter() {
            if key.0 != pin.cp.0 || key.1 != pin.cp.1 {
                continue;
            }
            let Some(record) = grouped.get_mut(key) else {
                continue;
            };
            let Some(inst) = record.instances.iter().find(|i| i.installed) else {
                continue;
            };
            let inst_str = format!(
                "{}/{}-{}:{}/{}",
                record.category, record.package, inst.version, record.slot, inst.sub_slot
            );
            if portage_dep::match_from_list(&pin.atom, &[inst_str.as_str()])
                .is_none_or(|m| m.is_empty())
            {
                continue;
            }
            let (cc, cp2, cv) = &pin.consumer;
            let consumer_ref = installed_refs(root, cc, cp2)
                .into_iter()
                .find(|r| &r.version == cv);
            let (cslot, csub, crepo) = match &consumer_ref {
                Some(r) => (r.slot.clone(), r.sub_slot.clone(), r.repo.clone()),
                None => ("0".to_string(), "0".to_string(), "__unknown__".to_string()),
            };
            let entry = SlotConflictParent {
                parent_cpv: format!("{cc}/{cp2}-{cv}:{cslot}/{csub}::{crepo}"),
                atom: pin.atom.clone(),
                use_display: installed_use_display_for(root, config, cc, cp2, cv),
                installed: true,
            };
            if let Some(inst) = record.instances.iter_mut().find(|i| i.installed)
                && !inst.parents.contains(&entry)
            {
                inst.parents.push(entry);
            }
        }
    }
    order.sort();
    order
        .into_iter()
        .filter_map(|k| grouped.remove(&k))
        .collect()
}

/// The values [`residual_conflict_for_pin`] reads from its caller.
pub(crate) struct ResidualConflictForPinArgs<'a> {
    pub(crate) pin: &'a RevDepPin,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) grouped: &'a mut HashMap<(String, String, String, String, String), SlotConflict>,
    pub(crate) slot_pullers: &'a SlotPullers,
    pub(crate) order: &'a mut Vec<(String, String, String, String, String)>,
}

/// Record the residual installed-instance slot conflict one dropped reverse-dependency pin leaves.
pub(crate) fn residual_conflict_for_pin(args: ResidualConflictForPinArgs<'_>) -> ControlFlow<()> {
    let ResidualConflictForPinArgs {
        pin,
        entries,
        repos,
        config,
        root,
        grouped,
        slot_pullers,
        order,
    } = args;
    let ResidualPinMergeInstanceOut {
        category,
        package,
        merge_ver,
        slot,
        merge_sub,
        merge_repo,
    } = match residual_pin_merge_instance(pin, entries) {
        ControlFlow::Continue(out) => out,
        ControlFlow::Break(()) => return ControlFlow::Break(()),
    };
    // A `[use]`-carrying pin version-matches by construction, so the holds
    // check must also evaluate its use-deps against the merge's decided
    // USE (uncomputable USE falls back to the version-only check).
    let merge_str = format!("{category}/{package}-{merge_ver}:{slot}/{merge_sub}::{merge_repo}");
    let use_still_broken = parse_dep_atom(&pin.atom, "slot_conflict")
        .and_then(|parsed| parsed.use_deps)
        .filter(|deps| !deps.is_empty())
        .is_some_and(|deps| {
            merge_use_state(repos, config, &category, &package, &merge_ver).is_some_and(
                |(merge_use, merge_iuse)| {
                    let parent_use = read_vdb_flag_set(
                        root,
                        &pin.consumer.0,
                        &pin.consumer.1,
                        &pin.consumer.2,
                        "USE",
                    );
                    portage_dep::use_deps_violated(&deps, &parent_use, &merge_use, &merge_iuse)
                },
            )
        });
    if !use_still_broken
        && portage_dep::match_from_list(&pin.atom, &[merge_str.as_str()])
            .is_some_and(|m| !m.is_empty())
    {
        return ControlFlow::Break(());
    }
    // The installed version the pin preserves: the highest installed one it
    // matches. None (already broken before this run) means no instance.
    let refs = installed_refs(root, &category, &package);
    let mut inst: Option<&InstalledRef> = None;
    for r in &refs {
        let s = format!(
            "{category}/{package}-{}:{}/{}",
            r.version, r.slot, r.sub_slot
        );
        if portage_dep::match_from_list(&pin.atom, &[s.as_str()]).is_some_and(|m| !m.is_empty())
            && inst.is_none_or(|best: &InstalledRef| {
                vercmp_ordering(&r.version, &best.version) == std::cmp::Ordering::Greater
            })
        {
            inst = Some(r);
        }
    }
    let Some(inst) = inst else {
        return ControlFlow::Break(());
    };
    // Different slots coexist -- the merge breaks nothing.
    if inst.slot != slot {
        return ControlFlow::Break(());
    }
    let key = (
        category.clone(),
        package.clone(),
        slot.clone(),
        merge_ver.clone(),
        inst.version.clone(),
    );
    if grouped.contains_key(&key) {
        return ControlFlow::Break(());
    }
    // Merge parents: hard pullers matching the merged version. Installed
    // parents: hard pullers matching the installed version, plus the
    // dropped consumer pins below.
    let (m_sub, _m_repo, _) = slot_conflict_meta(repos, &category, &package, &merge_ver);
    let merge_match = format!("{category}/{package}-{merge_ver}:{slot}/{m_sub}");
    let inst_match = format!(
        "{category}/{package}-{}:{}/{}",
        inst.version, inst.slot, inst.sub_slot
    );
    let (m_iuse, m_use) = slot_conflict_flag_sets(repos, config, &category, &package, &merge_ver);
    let (i_iuse, i_use) = installed_pkg_iuse_and_use(root, &category, &package, &inst.version);
    let use_ok_for = |atom_str: &str, iuse: &HashSet<String>, use_: &HashSet<String>| -> bool {
        match parse_dep_atom(atom_str, "slot_conflict") {
            Some(a) => match &a.use_deps {
                Some(ud) => portage_dep::use_deps_satisfied(ud, iuse, use_),
                None => true,
            },
            None => true,
        }
    };
    let mut merge_parents: Vec<SlotConflictParent> = Vec::new();
    let mut inst_parents: Vec<SlotConflictParent> = Vec::new();
    if let Some(pullers) = slot_pullers.get(&(category.clone(), package.clone())) {
        for (pc, pp, pv, atom) in pullers {
            let hits_merge = portage_dep::match_from_list(atom, &[merge_match.as_str()])
                .is_some_and(|m| !m.is_empty())
                && use_ok_for(atom, &m_iuse, &m_use);
            let hits_inst = portage_dep::match_from_list(atom, &[inst_match.as_str()])
                .is_some_and(|m| !m.is_empty())
                && use_ok_for(atom, &i_iuse, &i_use);
            let entry = SlotConflictParent {
                parent_cpv: slot_conflict_puller_cpv(repos, pc, pp, pv),
                atom: atom.clone(),
                use_display: if pc.is_empty() {
                    Vec::new()
                } else {
                    pkg_use_display_for(repos, config, pc, pp, pv)
                },
                installed: false,
            };
            if hits_merge {
                if !merge_parents.contains(&entry) {
                    merge_parents.push(entry);
                }
            } else if hits_inst && !inst_parents.contains(&entry) {
                inst_parents.push(entry);
            }
        }
    }
    let record = residual_slot_conflict_record(ResidualSlotConflictRecordArgs {
        category: &category,
        package: &package,
        slot: &slot,
        merge_ver: &merge_ver,
        pin,
        merge_sub: &merge_sub,
        merge_repo: &merge_repo,
        repos,
        config,
        merge_parents,
        inst,
        root,
        inst_parents,
    });
    order.push(key.clone());
    grouped.insert(key, record);
    ControlFlow::Continue(())
}

/// What [`residual_pin_merge_instance`] hands back to its caller.
pub(crate) struct ResidualPinMergeInstanceOut {
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) merge_ver: String,
    pub(crate) slot: String,
    pub(crate) merge_sub: String,
    pub(crate) merge_repo: String,
}

/// Find the merge-bound instance of the pinned package; Break when the package is not merged.
pub(crate) fn residual_pin_merge_instance(
    pin: &RevDepPin,
    entries: &[GraphEntry],
) -> ControlFlow<(), ResidualPinMergeInstanceOut> {
    let (category, package) = (pin.cp.0.clone(), pin.cp.1.clone());
    let mut merge: Option<(String, String, String, String)> = None;
    for e in entries {
        if e.category != category || e.package != package {
            continue;
        }
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version.clone())
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some(to.clone())
            }
            _ => None,
        };
        if let Some(version) = version {
            merge = Some((
                version,
                e.slot.clone().unwrap_or_else(|| "0".to_string()),
                e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                e.repo_name.clone().unwrap_or_default(),
            ));
        }
    }
    let Some((merge_ver, slot, merge_sub, merge_repo)) = merge else {
        return ControlFlow::Break(());
    };
    // The pin holds after all -- a later pass picked a compatible version.
    ControlFlow::Continue(ResidualPinMergeInstanceOut {
        category,
        package,
        merge_ver,
        slot,
        merge_sub,
        merge_repo,
    })
}

/// The values [`residual_slot_conflict_record`] reads from its caller.
pub(crate) struct ResidualSlotConflictRecordArgs<'a> {
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) merge_ver: &'a str,
    pub(crate) pin: &'a RevDepPin,
    pub(crate) merge_sub: &'a str,
    pub(crate) merge_repo: &'a str,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) merge_parents: Vec<SlotConflictParent>,
    pub(crate) inst: &'a InstalledRef,
    pub(crate) root: &'a Path,
    pub(crate) inst_parents: Vec<SlotConflictParent>,
}

/// Build the residual slot-conflict record for the installed instance and its merge-bound partner.
pub(crate) fn residual_slot_conflict_record(
    args: ResidualSlotConflictRecordArgs<'_>,
) -> SlotConflict {
    let ResidualSlotConflictRecordArgs {
        category,
        package,
        slot,
        merge_ver,
        pin,
        merge_sub,
        merge_repo,
        repos,
        config,
        merge_parents,
        inst,
        root,
        inst_parents,
    } = args;

    SlotConflict {
        category: category.to_string(),
        package: package.to_string(),
        slot: slot.to_string(),
        resolved_version: merge_ver.to_string(),
        conflicting_atom: pin.atom.clone(),
        instances: vec![
            SlotConflictInstance {
                version: merge_ver.to_string(),
                sub_slot: merge_sub.to_string(),
                repo_name: merge_repo.to_string(),
                use_display: pkg_use_display_for(repos, config, category, package, merge_ver),
                parents: merge_parents,
                installed: false,
            },
            SlotConflictInstance {
                version: inst.version.clone(),
                sub_slot: inst.sub_slot.clone(),
                repo_name: inst.repo.clone(),
                use_display: installed_use_display_for(
                    root,
                    config,
                    category,
                    package,
                    &inst.version,
                ),
                parents: inst_parents,
                installed: true,
            },
        ],
    }
}
