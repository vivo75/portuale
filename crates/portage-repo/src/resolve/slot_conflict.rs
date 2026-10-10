//! Slot conflicts: records, merging and rendering.

use crate::*;

/// A genuine slot conflict: two different dependency atoms, both landing
/// on the same category/package/slot, whose independently-resolved
/// "best" versions are incompatible -- the atom that reached this slot
/// *second* doesn't actually accept the version the *first* one already
/// caused to be resolved (and recursed into). This is distinct from two
/// atoms simply requesting *different* slots of the same package (e.g.
/// `dev-lang/python:3.11` and `dev-lang/python:3.12`), which real portage
/// -- and portuale, see `resolve_pretend_graph`'s doc comment -- treats
/// as normal, valid coexistence, not a conflict at all.
///
/// Purely informational, the same "report, don't enforce" spirit as
/// `BlockerConflict`: real portage's own depgraph treats an unresolved
/// slot conflict as fatal and refuses to proceed; portuale instead
/// reports it and keeps going (using whichever version was resolved
/// first), consistent with the rest of this follow-up series --
/// `--pretend` itself never touches anything real, so nothing here is
/// truly "fatal" to calculate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflict {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The version already resolved (and recursed into) for this slot,
    /// by whichever atom reached it first.
    pub resolved_version: String,
    /// The atom text that does NOT accept `resolved_version`.
    pub conflicting_atom: String,
    /// Every distinct version of `category/package:slot` pulled into the
    /// graph (>= 2), each with the parents that pulled it -- real
    /// `slot_collision_handler`'s per-`slot_atom` `(pkg, parent_atoms)`
    /// data, used to render the `!!! Multiple package instances ...`
    /// block. `instances[0]` is always `resolved_version`.
    pub instances: Vec<SlotConflictInstance>,
}

/// One version of a slot-conflicted `category/package:slot`, plus the
/// parents that pulled it in. See `SlotConflict`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictInstance {
    pub version: String,
    pub sub_slot: String,
    pub repo_name: String,
    /// Real `pkg_use_display(pkg, ...)` for this scheduled-for-merge
    /// instance -- the `USE="…"` (+ one `VAR="…"` per non-hidden
    /// `USE_EXPAND` group) pairs rendered on its own `(… ebuild
    /// scheduled for merge) USE="…" pulled in by` header line. Every
    /// `IUSE` flag, enabled-first, `( )`-wrapped for profile force/mask
    /// (same `build_use_expand_display` the `--info` block uses). Empty
    /// slice → the header still renders a bare `USE=""`. For an
    /// installed instance the same pairs built from the vdb's own
    /// `IUSE`/`USE` (see `installed_use_display_for`).
    pub use_display: Vec<(String, String)>,
    pub parents: Vec<SlotConflictParent>,
    /// Real's installed nomerge node: an already-installed version kept
    /// in the graph by a residual consumer pin (see
    /// `build_residual_slot_conflicts`), rendered `(… installed in
    /// '<root>')` instead of `(… ebuild scheduled for merge)`. Always
    /// false for merge-vs-merge conflicts.
    pub installed: bool,
}

/// One parent that pulled a `SlotConflictInstance` into the graph. See
/// `SlotConflictInstance::parents`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictParent {
    /// `cat/pkg-ver:slot/sub_slot::repo` for a dependency, or empty for a
    /// top-level atom (rendered `<atom> (Argument)`).
    pub parent_cpv: String,
    pub atom: String,
    /// Real `pkg_use_display(parent, ...)` -- the parent package's own
    /// resolved `USE="…"` on its `<atom> required by (<parent_cpv>, …)
    /// USE="…"` line. Empty for a top-level `(Argument)` parent (real
    /// renders a bare `""` for a non-`Package` parent). For an installed
    /// parent the vdb's own `IUSE`/`USE` (see
    /// `installed_use_display_for`).
    pub use_display: Vec<(String, String)>,
    /// Real's installed nomerge parent: an already-installed package
    /// whose recorded atom the merge breaks, rendered `required by
    /// (<parent_cpv>, installed in '<root>')`. Always false for
    /// merge-vs-merge conflicts.
    pub installed: bool,
}

/// `(sub_slot, repo_name, slot)` of `cat/pkg`'s own `version` -- the
/// highest-`repo_priority` candidate carrying that exact version, the
/// same re-lookup `slot_changed` / `deps_changed` already do. All-empty
/// if the version is no longer in any repo. Slice 4's slot-collision
/// block uses it to print the real `(<cpv>:<slot>/<sub>::<repo>, ...)`
/// form for every conflicting instance and every parent.
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

/// Which conflict party (if either) is an already-installed graph node
/// rather than a scheduled-for-merge one -- #57's installed-instance
/// collisions (real `_add_pkg` feeds installed nomerge nodes to
/// `_package_tracker` too, so one slot holding an installed node plus a
/// merge node is an ordinary slot collision). `Existing` is
/// `build_slot_conflict`'s instance A (already in the graph), `Current`
/// its instance B (what the detecting atom resolved to).
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

/// Assembles a `SlotConflict` (real `slot_collision_handler`'s
/// `(pkg, parent_atoms)` per `slot_atom`): instance A is `existing_version`
/// (already in the graph), instance B is `current_version` (what the
/// atom that triggered detection resolved to). Each `slot_pullers` entry
/// for `cat/pkg` is filed under whichever instance its atom matches
/// (under A when it matches both -- a bare atom pulled the resolved one).
///
/// `installed_side` (#57) names the party that is an installed nomerge
/// node: its sub-slot/repo, `USE=` display and use-dep flag sets come
/// from the vdb (`installed_refs` / `installed_use_display_for` /
/// `installed_pkg_iuse_and_use`, exactly what `build_residual_slot_conflicts`
/// uses) instead of from the repo candidate carrying the same version,
/// and its instance renders `(… installed in '<root>')`. `None` for the
/// merge-vs-merge conflicts this function has always built.
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
    // An installed instance's own vdb `(sub_slot, repo)`: the repo the
    // *installed* copy came from can differ from whatever currently
    // provides that version, and a version no longer in any repo has no
    // candidate at all -- both of which `slot_conflict_meta` would render
    // as empty strings.
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
    // The match strings carry the sub-slot (real matches puller atoms
    // against full packages, not slot-only strings): without it, a
    // *built* slot-operator puller (`cat/pkg:S/SS=`) matches neither
    // instance and vanishes from the notice entirely -- which also
    // starves the `need_rebuild` scan below of exactly the parents it
    // exists for. A missing sub-slot (version gone from every repo)
    // keeps the old slot-only shape.
    let with_sub = |v: &str, sub: &str| {
        if sub.is_empty() {
            format!("{category}/{package}-{v}:{slot}")
        } else {
            format!("{category}/{package}-{v}:{slot}/{sub}")
        }
    };
    let a_match = with_sub(existing_version, &a_sub);
    let b_match = with_sub(current_version, &b_sub);
    // USE-aware filing: a puller files under the instance its atom
    // *fully* satisfies (version+slot+USE), not just the string match.
    // Real lists every puller and display-filters by reason; portuale's
    // two-instance model files by match instead, so a USE-mismatching
    // parent must land on the instance it actually pulls (e.g.
    // `>=T-1.0[x]` under 1.0, never under the x-off 2.0 it version-
    // matches). Without this, USE parents file under the wrong instance
    // and vanish from the notice (their atom satisfies the other side).
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
            let parsed = portage_dep::parse_atom(atom);
            let use_ok = |iuse: &HashSet<String>, use_: &HashSet<String>| match &parsed {
                Some(a) => match &a.use_deps {
                    Some(ud) => portage_dep::use_deps_satisfied(ud, iuse, use_),
                    None => true,
                },
                // Unparseable atoms (soname and friends never reach
                // pullers, but be permissive): file by string match.
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

/// `` `cat/pkg-ver:slot/sub_slot::repo` `` for a conflicting parent --
/// the real `(<cpv>:<slot>/<sub>::<repo>, …)` form, empty for a
/// top-level `(Argument)` parent. Shared by `build_slot_conflict` and
/// the residual installed-instance report.
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

/// Records a freshly built `SlotConflict`, merging into an existing
/// record for the same `(category, package, slot, existing, current)`
/// triple instead of appending a duplicate block. Real keeps one
/// collision handler per conflicting slot, accumulating every puller;
/// portuale pushes per mismatching atom, so a second distinct atom
/// hitting the same pair (e.g. two USE-deps pulling the older version)
/// must re-render the record from the pullers recorded so far -- which
/// `build_slot_conflict` already does from the live `slot_pullers` map,
/// making the merge a pure replace. Records that differ in any key
/// element are unrelated and still append (the pre-existing multi-slot
/// behavior is untouched).
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

/// #142 S2 F1: merge `SlotConflict` records sharing one
/// `(category, package, slot)` into a single joint record before the
/// direct solve. `record_slot_conflict` files one record per
/// mismatching atom, so a three-version contention lands as two
/// two-instance records (pg0's `b c a` order: `(X-3,X-2)` +
/// `(X-3,X-1)`); solving each independently keeps a version a later
/// atom rejects, and the repoint loop's second removal finds no entry
/// and vanishes silently — the row settles at X-2 against B's `<X-2`
/// bound. The joint record carries every contending instance with
/// per-instance parents re-filed from the live pullers (a puller files
/// under *every* instance its atom fully satisfies — the pairwise
/// `hits_a else hits_b` rule of `build_slot_conflict` generalized to
/// n-way), so the solver sees the whole contention exactly like real's
/// per-slot collision handler and keeps the uniquely
/// jointly-satisfying version from any walk order. Singleton groups
/// pass through untouched; a merged record inherits the first
/// record's `resolved_version`/`conflicting_atom` (render fields, only
/// read when the conflict survives the solve).
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
                let parsed = portage_dep::parse_atom(atom);
                for (ii, inst) in instances.iter().enumerate() {
                    // The same structural probe
                    // `direct_solve_atom_matches` matches puller atoms
                    // against (see its doc comment): tree
                    // slot/sub-slot, USE checked against the
                    // instance's own resolved flags.
                    // #291: an installed instance carries its own vdb
                    // sub-slot (`build_slot_conflict` read it there); the
                    // tree's metadata for the same version can differ, or
                    // be absent, and would drop a built `:S/SS=` puller
                    // from the instance it actually pulls.
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
                    // USE-aware filing, same as `build_slot_conflict`:
                    // a puller files under the instance its atom
                    // *fully* satisfies (version+slot+USE).
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

/// Backlog #148: graph the shadowed second instances of *surviving*
/// slot conflicts beside their records.
///
/// The walk graphs one entry per `(category, package, slot)` (the
/// `resolved_slots` conflict arm records the `SlotConflict` and moves
/// on), so a surviving conflict's other instances have a notice but no
/// merge row -- while real prints a row per instance (bed
/// `l0-fx-20260922T192350Z`, cell `-D dev-libs/slotusegroup`: real
/// schedules both `slotusetarget-1.0` and `-2.0`, portuale only `1.0`,
/// notices identical, rc 1 both sides).
///
/// Runs after the direct solve, over its survivors only, so the
/// solver's inputs -- and every solved cell -- are byte-identical to
/// before: a dissolved conflict has no row owed (the removal
/// machinery already repoints or deletes the loser's row), and only a
/// conflict real would print before giving up gains rows.
///
/// A synthesized entry is display-complete but walk-light. Slot /
/// sub-slot / repo / USE come from the same tree-candidate reads the
/// notice uses (`refresh_entry_use_display` on a one-entry slice, the
/// #141 precedent -- existing entries are untouched), `required_by`
/// from the same cp-union the post-walk fill gives its siblings, and
/// scheduler edges resolve to it through the USE-narrowed
/// `match_candidates`, so it orders like a walked node. Deliberately
/// *not* carried over: the instance's own deps (walking them would
/// need a second walk -- the same documented non-leaf staleness as
/// the #142 S2 F3 keeper-blocker flattening), its blockers (a
/// second-instance blocker is a new number, not this item), and the
/// ebuild-metadata row decorations (`interactive`, fetch-restrict /
/// `download_files`, `keyword_mask`, `provenance`, binary `source`)
/// -- all default to the plain-undecorated `New` shape. Each is a
/// filed-later residue if a bed ever shows a twin that needs it, not
/// silent scope growth here.
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
                deps: Vec::new(),
            };
            refresh_entry_use_display(
                std::slice::from_mut(&mut entry),
                repos,
                root,
                &(sc.category.clone(), sc.package.clone()),
                config,
            );
            // File beside its siblings: real prints the conflicting
            // instances adjacently (`1.0` then `2.0` at the head of the
            // S0.1 capture), and discovery order settles scheduler ties.
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

/// Residual installed-instance slot conflicts for dropped reverse-dep
/// pins (see `RevDepPin`): real `_complete_graph`'s end-of-walk
/// unsatisfied-dep loop (`depgraph.py:8770+`) adds the installed package
/// satisfying a broken deep dependency as a nomerge node, so the run
/// reports it as a slot collision against the merged version instead of
/// silently breaking it -- merge instance vs installed instance, exit 1.
///
/// Portuale reports the same shape without carrying the nodes: for every
/// dropped pin whose consumer still names an installed version the final
/// merge-bound version fails, one record pairing the merge instance
/// (parents: this pass's hard pullers) against the installed instance
/// (parents: hard pullers matching it, plus every dropped consumer pin
/// for it). Verified live against real 3.0.82.2 (`=paired-2.0` over a
/// keeper pinning `=paired-1.0`, and the needer/othermod triangle): real
/// prints the merge list with the upgrade *and* the conflict block, exit
/// 1 -- where portuale used to mask the version into invisibility
/// (`!!! no visible ebuild`) or fail the top-level atom outright.
///
/// A pin whose final version satisfies it after all (a later pass picked
/// a compatible version anyway), one matching no installed version
/// (the consumer was already broken -- real's `initially_unsatisfied`
/// skip), or one whose installed version lives in a different slot than
/// the merge (slots coexist; nothing breaks) yields no record. One
/// record per `(package, merge version, installed version)`; parents
/// unioned across pins. Sorted for determinism.
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
    // (category, package, slot, merge version, installed version) ->
    // installed-parent index into the record being assembled.
    let mut grouped: HashMap<(String, String, String, String, String), SlotConflict> =
        HashMap::new();
    let mut order: Vec<(String, String, String, String, String)> = Vec::new();
    // `dropped` accumulates across backtrack passes, so a pin can outlive
    // the reason it was recorded: its consumer may be merge-bound in the
    // final graph (e.g. a `:=` rebuild seeded by a later pass). Apply
    // `reverse_dependency_constraints`'s own "consumer being replaced"
    // skip against the final `entries` (real `depgraph.py:2512-2522`).
    let replaced: HashSet<(&str, &str)> = entries
        .iter()
        .filter(|e| merge_bound_cpv(e).is_some())
        .map(|e| (e.category.as_str(), e.package.as_str()))
        .collect();
    let dropped: Vec<&RevDepPin> = dropped
        .iter()
        .filter(|p| !replaced.contains(&(p.consumer.0.as_str(), p.consumer.1.as_str())))
        .collect();
    for pin in dropped.iter().copied() {
        let (category, package) = (pin.cp.0.clone(), pin.cp.1.clone());
        // The final merge-bound version of this package, if any.
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
            continue;
        };
        // The pin holds after all -- a later pass picked a compatible
        // version. Nothing to report (real reconciles silently too).
        // Backlog #222: a `[use]`-carrying pin (a USE-break dropped pin)
        // version-matches the merge by construction -- `match_from_list`
        // never reads use-deps, same as real's -- so the holds check
        // must also evaluate the pin's use-deps against the merge's own
        // decided USE (real `violated_conditionals`, the same check that
        // fired the pin). A merge whose USE still breaks the pin stays
        // reported; an uncomputable merge USE falls back to the
        // version-only check, today's behaviour.
        let merge_str =
            format!("{category}/{package}-{merge_ver}:{slot}/{merge_sub}::{merge_repo}");
        let use_still_broken = portage_dep::parse_atom(&pin.atom)
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
            continue;
        }
        // The installed version the pin preserves: the highest installed
        // one it matches (real's `matches[-1]`). None -- the consumer was
        // already broken before this run -- means no instance to show.
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
        let Some(inst) = inst else { continue };
        // Different slots coexist -- the merge breaks nothing.
        if inst.slot != slot {
            continue;
        }
        let key = (
            category.clone(),
            package.clone(),
            slot.clone(),
            merge_ver.clone(),
            inst.version.clone(),
        );
        if grouped.contains_key(&key) {
            continue;
        }
        // Merge parents: this pass's hard pullers matching the merged
        // version (plus every top-level Argument puller -- real files an
        // `(Argument)` parent under a reason exactly when the other side
        // is installed, which a residual always is). Installed parents:
        // hard pullers matching the installed version instead, plus the
        // dropped consumer pins below. Mirror `build_slot_conflict`'s
        // match-first filing and USE-aware check on both sides.
        let (m_sub, _m_repo, _) = slot_conflict_meta(repos, &category, &package, &merge_ver);
        let merge_match = format!("{category}/{package}-{merge_ver}:{slot}/{m_sub}");
        let inst_match = format!(
            "{category}/{package}-{}:{}/{}",
            inst.version, inst.slot, inst.sub_slot
        );
        let (m_iuse, m_use) =
            slot_conflict_flag_sets(repos, config, &category, &package, &merge_ver);
        let (i_iuse, i_use) = installed_pkg_iuse_and_use(root, &category, &package, &inst.version);
        let use_ok_for = |atom_str: &str, iuse: &HashSet<String>, use_: &HashSet<String>| -> bool {
            match portage_dep::parse_atom(atom_str) {
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
        let record = SlotConflict {
            category: category.clone(),
            package: package.clone(),
            slot: slot.clone(),
            resolved_version: merge_ver.clone(),
            conflicting_atom: pin.atom.clone(),
            instances: vec![
                SlotConflictInstance {
                    version: merge_ver.clone(),
                    sub_slot: merge_sub.clone(),
                    repo_name: merge_repo.clone(),
                    use_display: pkg_use_display_for(
                        repos, config, &category, &package, &merge_ver,
                    ),
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
                        &category,
                        &package,
                        &inst.version,
                    ),
                    parents: inst_parents,
                    installed: true,
                },
            ],
        };
        order.push(key.clone());
        grouped.insert(key, record);
    }
    // Fold every dropped consumer pin for the group into the installed
    // instance's parents (real lists each installed consumer beside the
    // hard pullers already filed above). A pin belongs to a record only
    // when it guards that record's own installed version.
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
