//! Solving slot conflicts directly, without backtracking.

use crate::*;

/// Backlog #90 (S2): real `_solve_non_slot_operator_slot_conflicts`
/// (`depgraph.py:1774-2106`), run unconditionally by
/// `_process_slot_conflicts` (`:2108-2116`) -- no backtracking gate
/// (the bt0 capture says `backtrack: 0/0`, zero retries, yet
/// reconciled). Portuale used to reconcile solvable shapes only
/// through its backtrack retry loop, so at `--backtrack=0` it recorded
/// the conflict (notice + rc 1 since #62) where real is silent.
///
/// A conflict package with no non-conflict parent edge is unforced and
/// removed (`_remove_pkg`), its parents re-walked (`_dep_stack` +
/// `_create_graph`), and the removed instance reported via
/// `_conflict_missed_update["slot conflict"]`. Portuale mirrors the
/// verdict computation (forced/non-forced sets, `or_tuple`
/// first-pulled, `non_matching_forced` protection, the `is_arg_parent`
/// installed exclusion, the `_slot_operator_replace_installed` skip)
/// over its own recorded `SlotConflict`s; removal drops the merge
/// entries and prunes single-instance survivors, and the re-walk is
/// folded into the settle (the walk already resolved every remaining
/// entry -- S2 cells carry no blockers on removed instances, so no
/// `_validate_blockers` rerun is owed).
///
/// Deliberate narrowings, all documented at the use sites:
/// - indirect conflict candidates (children reachable only through
///   conflict packages, `:1808-1829`) are not modelled -- every S2
///   oracle cell holds leaf instances, so the set is empty there.
/// - no orphan cascade (`_remove_pkg`'s parentless removal): removed S2
///   instances are leaves; lingering orphans, if any, stay listed.
/// - removal runs after the blocker filing (real validates blockers
///   after solving): blocker rows anchored to a removed entry are out
///   of scope -- no S2 cell has them.
pub(crate) struct DirectSolveInput<'a> {
    pub conflicts: &'a [SlotConflict],
    pub entries: &'a [GraphEntry],
    pub top_level: &'a HashSet<(String, String)>,
    pub excluded: &'a [String],
    pub selective: bool,
    pub replace_cps: &'a BTreeSet<(String, String)>,
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
    pub config: &'a portage_profile::Config,
}

pub(crate) struct DirectSolveOutput {
    /// Merge instances to drop, each with the surviving same-slot
    /// version that takes over its merge row (`(category, package,
    /// removed_version, kept_version)`). The row is repointed rather
    /// than deleted: real merges the kept instance (its row stays),
    /// while the removed one vanishes -- and portuale holds one row
    /// per cp anyway.
    pub removed: Vec<(String, String, String, String)>,
    pub skipped: Vec<SkippedUpdate>,
    /// Conflicts with >= 2 surviving instances (the rest dissolve with
    /// the removal -- their story is the skip notice, not a block).
    pub surviving: Vec<SlotConflict>,
}

/// A conflict-graph node: a package instance, the shared
/// non-conflict sink, or a multi-match `or_tuple`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DirectSolveNode {
    NonConflict,
    Instance(usize, usize),
    OrTuple(usize),
}

/// Whether `atom_text` accepts conflict `c`'s instance `inst_idx`,
/// real `(parent, atom)` matching (`atom.match(pkg.with_use(...))`,
/// `:1921-1930`) with portuale's display-grade metadata: the
/// structural probe carries the tree slot/sub-slot (the same
/// `slot_conflict_meta` convention the record path uses), and
/// `[use]` deps are checked against the instance's own resolved flags
/// (merge entry display, else tree-effective, else vdb -- see
/// `direct_solve_instance_use`). `for_skipped` selects real's
/// plain-match form used for the missed-update recording (`:2083-2094`,
/// no `is_arg_parent` gate there).
pub(crate) fn direct_solve_atom_matches(
    input: &DirectSolveInput<'_>,
    c: &SlotConflict,
    inst_idx: usize,
    atom_text: &str,
    arg_mode: bool,
) -> bool {
    let inst = &c.instances[inst_idx];
    if arg_mode && inst.installed {
        return false;
    }
    let Some(atom) = portage_dep::parse_atom(atom_text) else {
        return false;
    };
    // A bare `cat/pkg:=` matches its own conflict's every instance:
    // real matches the raw atom structurally (`atom.match(...)` has no
    // built-binding step -- the `:=` only narrows by slot linkage),
    // while portuale's list matcher does not implement the built
    // operator at all (it would match nothing and wrongly dissolve
    // the need_rebuild-trailer conflict). Slot and cp always agree
    // here (edges are per-conflict); version/USE never discriminate
    // a same-slot `:=` pull.
    let bare_equals = atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
        && atom.slot.is_none()
        && atom.sub_slot.is_none()
        && atom.category == c.category
        && atom.package == c.package;
    if bare_equals && atom.version.is_none() {
        return true;
    }
    let mut probe_atom: String = if atom.use_deps.as_ref().is_some_and(|d| !d.is_empty()) {
        portage_dep::without_use(atom_text).to_string()
    } else {
        atom_text.to_string()
    };
    // #270: a *versioned* bare `:=` atom (`>=cat/pkg-4.02:=`) still
    // discriminates by version in real (`atom.match` applies the
    // version; the `:=` only narrows by slot linkage, which this
    // per-conflict edge already shares), so drop the operator and match
    // the rest. Without this `mllabltk`'s `>=mlocaml-4.02:=` accepted
    // the installed `4.01.0` and never forced the `4.02.1` instance.
    if bare_equals && let Some(stripped) = probe_atom.strip_suffix(":=") {
        probe_atom = stripped.to_string();
    }
    let (sub, repo, slot_of_ver) =
        slot_conflict_meta(input.repos, &c.category, &c.package, &inst.version);
    let probe = format!(
        "{}/{}-{}:{}/{}::{}",
        c.category, c.package, inst.version, slot_of_ver, sub, repo
    );
    if portage_dep::match_from_list(&probe_atom, &[probe.as_str()]).is_none_or(|m| m.is_empty()) {
        return false;
    }
    let use_deps = atom.use_deps.unwrap_or_default();
    if use_deps.is_empty() {
        return true;
    }
    let (enabled, declared) = direct_solve_instance_use(input, &c.category, &c.package, inst);
    portage_dep::use_deps_satisfied(&use_deps, &valid_iuse(&declared, input.config), &enabled)
}

/// Resolved USE (enabled) + declared IUSE for a conflict instance --
/// real `pkg.with_use(self._pkg_use_enabled(pkg))`: a merge instance
/// with an entry reads its resolved display flags, a merge instance
/// without one (shadowed second instance) reads the tree candidate
/// effectively, an installed instance reads its vdb record.
pub(crate) fn direct_solve_instance_use(
    input: &DirectSolveInput<'_>,
    category: &str,
    package: &str,
    inst: &SlotConflictInstance,
) -> (HashSet<String>, HashSet<String>) {
    if inst.installed {
        let enabled = read_vdb_flag_set(input.root, category, package, &inst.version, "USE");
        let declared = list_candidates(input.repos, category, package)
            .ok()
            .and_then(|cs| {
                cs.iter().find(|c| c.version == inst.version).map(|c| {
                    c.iuse
                        .split_whitespace()
                        .map(|t| t.trim_start_matches(['+', '-']).to_string())
                        .collect()
                })
            })
            .unwrap_or_else(|| enabled.clone());
        return (enabled, declared);
    }
    if let Some(entry) = input.entries.iter().find(|e| {
        e.category.as_str() == category
            && e.package.as_str() == package
            && merge_bound_version(&e.outcome) == Some(&inst.version)
    }) {
        let enabled: HashSet<String> = entry
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let declared: HashSet<String> = entry
            .use_flags_display
            .iter()
            .map(|(f, _)| f.clone())
            .collect();
        return (enabled, declared);
    }
    let (declared, enabled) =
        slot_conflict_flag_sets(input.repos, input.config, category, package, &inst.version);
    (enabled, declared)
}

/// Real `_want_installed_pkg` (`depgraph.py:7280-7302`) folded to what
/// the direct solve needs: the installed instance is "argued" (excluded
/// from every parent's match list) exactly when the user explicitly
/// requested that cp (a top-level atom targets it) without `--exclude`
/// and outside selective mode. Excluded/selective runs want the
/// installed package (`True`), as does a cp nobody asked for.
pub(crate) fn direct_solve_arg_mode(input: &DirectSolveInput<'_>, c: &SlotConflict) -> bool {
    if !c.instances.iter().any(|i| i.installed) || input.selective {
        return false;
    }
    if !input
        .top_level
        .contains(&(c.category.clone(), c.package.clone()))
    {
        return false;
    }
    let inst_ver = c
        .instances
        .iter()
        .find(|i| i.installed)
        .map(|i| i.version.clone())
        .unwrap_or_default();
    let probe = format!("{}/{}-{inst_ver}", c.category, c.package);
    !input
        .excluded
        .iter()
        .any(|ex| matches_config_entry(ex, &probe, &c.category, &c.package))
}

pub(crate) fn direct_solve_slot_conflicts(input: DirectSolveInput<'_>) -> DirectSolveOutput {
    // Insertion order for `or_tuple` first-pulled (`:1998-2006`):
    // merge instances in walk (entries) order, installed ones after
    // (real's tracker order for installed nodes is walk-dependent;
    // every S2 oracle tuple is all-merge, so this is untested beyond
    // that -- documented here, not silently assumed).
    let mut order: HashMap<(usize, usize), usize> = HashMap::new();
    for (ci, c) in input.conflicts.iter().enumerate() {
        let mut rank = 0;
        for e in input.entries.iter() {
            for (ii, inst) in c.instances.iter().enumerate() {
                if !inst.installed
                    && e.category == c.category
                    && e.package == c.package
                    && merge_bound_version(&e.outcome) == Some(&inst.version)
                {
                    order.entry((ci, ii)).or_insert_with(|| {
                        rank += 1;
                        rank
                    });
                }
            }
        }
        for (ii, inst) in c.instances.iter().enumerate() {
            if inst.installed {
                order.entry((ci, ii)).or_insert_with(|| {
                    rank += 1;
                    rank
                });
            }
        }
    }
    // All conflict members, for the non-conflict test (`:1897-1901`):
    // a parent whose (cp, version, installedness) names a conflict
    // instance draws a package-to-package edge instead of the
    // `non_conflict_node` substitution.
    let member_of = |cp: &(String, String), ver: &str, installed: bool| -> Option<(usize, usize)> {
        for (ci, c) in input.conflicts.iter().enumerate() {
            if (c.category.clone(), c.package.clone()) != *cp {
                continue;
            }
            for (ii, inst) in c.instances.iter().enumerate() {
                if inst.version == ver && inst.installed == installed {
                    return Some((ci, ii));
                }
            }
        }
        None
    };
    let parent_identity = |parent_cpv: &str| -> Option<(String, String, String)> {
        // Full `cat/pkg-ver:slot/sub::repo` form (see the gate below):
        // strip the slot/repo suffix `split_cpv` cannot read.
        split_cpv(parent_cpv.split(':').next().unwrap_or(""))
    };
    let mut removed: Vec<(String, String, String, String)> = Vec::new();
    let mut skipped: Vec<SkippedUpdate> = Vec::new();
    let mut surviving: Vec<SlotConflict> = Vec::new();
    for (ci, c) in input.conflicts.iter().enumerate() {
        // Slot-operator rebuild conflicts never enter the direct solve
        // (`:1784-1788`): their key is `(root, slot_atom)`; portuale's
        // replace set is cp-keyed, which is the same gate at our
        // granularity (#24/#78 territory, not #90).
        //
        // Neither do conflicts that feed the need_rebuild trailer: an
        // installed parent whose built slot-operator atom (`:=` with a
        // sub-slot, real `Atom.slot_operator_built`) cannot rebuild
        // (`--exclude` / `--useoldpkg-atoms`) is real's rebuild-path
        // residue, not remove-path material -- removing the reinstall
        // would silence the trailer the F-A2 gate exists to print.
        // (The masked-or-unavailable-ebuild reason stays the filed
        // follow-up: this gate only knows the two pinned reasons.)
        let stuck_rebuild_parent = c.instances.iter().flat_map(|i| &i.parents).any(|p| {
            let Some(atom) = portage_dep::parse_atom(&p.atom) else {
                return false;
            };
            if !is_built_slot_op(&atom) {
                return false;
            }
            // `parent_cpv` carries real's full
            // `cat/pkg-ver:slot/sub::repo` form, which `split_cpv`
            // cannot read (it validates the trailing version) --
            // strip the slot/repo suffix first.
            let bare = p.parent_cpv.split(':').next().unwrap_or("");
            let Some((pc, pp, pv)) = split_cpv(bare) else {
                return false;
            };
            if !installed_candidates(input.root, &pc, &pp)
                .iter()
                .any(|(v, _, _)| v == &pv)
            {
                return false;
            }
            let cpv = format!("{pc}/{pp}-{pv}");
            input
                .excluded
                .iter()
                .any(|ex| matches_config_entry(ex, &cpv, &pc, &pp))
                || useoldpkg_atom_matches(&pc, &pp, &pv)
        });
        if stuck_rebuild_parent
            || input
                .replace_cps
                .contains(&(c.category.clone(), c.package.clone()))
        {
            surviving.push(c.clone());
            continue;
        }
        let arg_mode = direct_solve_arg_mode(&input, c);
        // Edges: (parent node, matched instance indices).
        let mut edges: Vec<(DirectSolveNode, Vec<usize>)> = Vec::new();
        let mut protect_all = false;
        for (ii, inst) in c.instances.iter().enumerate() {
            let _ = ii;
            for p in &inst.parents {
                let parent_node = match parent_identity(&p.parent_cpv) {
                    Some((pc, pp, pv)) => member_of(&(pc.clone(), pp.clone()), &pv, p.installed)
                        .map_or(DirectSolveNode::NonConflict, |(mci, mii)| {
                            DirectSolveNode::Instance(mci, mii)
                        }),
                    // A top-level `(Argument)` pull (empty cpv): no
                    // package parent, so the shared sink. Its atom
                    // still matches normally below.
                    None => DirectSolveNode::NonConflict,
                };
                let mut matched: Vec<usize> = Vec::new();
                for (jj, _) in c.instances.iter().enumerate() {
                    if direct_solve_atom_matches(&input, c, jj, &p.atom, arg_mode) {
                        matched.push(jj);
                    }
                }
                if matched.is_empty() {
                    // Autounmask-broken USE or multislot (`:1926-1931`,
                    // bug 220341): protect the whole conflict from
                    // removal instead of solving it wrong.
                    protect_all = true;
                } else if matched.len() > 1 {
                    matched.sort_by_key(|jj| order.get(&(ci, *jj)).copied().unwrap_or(usize::MAX));
                    edges.push((parent_node, matched));
                } else {
                    edges.push((parent_node, matched));
                }
            }
        }
        // Children adjacency for the forced exploration.
        let mut children: HashMap<DirectSolveNode, Vec<DirectSolveNode>> = HashMap::new();
        // or_tuple bodies, by edge index.
        let mut tuples: Vec<Vec<(usize, usize)>> = Vec::new();
        for (parent, matched) in &edges {
            if matched.len() > 1 {
                let id = tuples.len();
                tuples.push(matched.iter().map(|jj| (ci, *jj)).collect());
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(DirectSolveNode::OrTuple(id));
            } else {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(DirectSolveNode::Instance(ci, matched[0]));
            }
        }
        let mut forced: HashSet<DirectSolveNode> = HashSet::new();
        if !protect_all {
            forced.insert(DirectSolveNode::NonConflict);
            let mut unexplored = vec![DirectSolveNode::NonConflict];
            let mut explored: HashSet<DirectSolveNode> = HashSet::new();
            let mut pending_tuples: Vec<usize> = Vec::new();
            while let Some(node) = unexplored.pop() {
                if let Some(kids) = children.get(&node) {
                    for child in kids {
                        if !explored.insert(child.clone()) {
                            continue;
                        }
                        forced.insert(child.clone());
                        match child {
                            DirectSolveNode::Instance(_, _) => unexplored.push(child.clone()),
                            DirectSolveNode::OrTuple(id) => pending_tuples.push(*id),
                            DirectSolveNode::NonConflict => {}
                        }
                    }
                }
            }
            // `or_tuple`s resolve only once every plain package had its
            // chance (`:1994-2006`): an already-forced member satisfies
            // the dependency, otherwise the first-pulled member wins
            // ("the package that was pulled first, as this should be
            // the most desirable choice").
            while let Some(id) = pending_tuples.pop() {
                let members = &tuples[id];
                if members
                    .iter()
                    .any(|m| forced.contains(&DirectSolveNode::Instance(m.0, m.1)))
                {
                    continue;
                }
                let mut first = members.clone();
                first.sort_by_key(|m| order.get(m).copied().unwrap_or(usize::MAX));
                if let Some(winner) = first.first() {
                    forced.insert(DirectSolveNode::Instance(winner.0, winner.1));
                    unexplored.push(DirectSolveNode::Instance(winner.0, winner.1));
                }
            }
            // Dependencies of forced packages join the set (`:2012-2020`;
            // portuale models no indirect candidates -- see the fn docs).
            while let Some(node) = unexplored.pop() {
                if let Some(kids) = children.get(&node) {
                    for child in kids {
                        if explored.insert(child.clone()) {
                            forced.insert(child.clone());
                            unexplored.push(child.clone());
                        }
                    }
                }
            }
        } else {
            // Protected: every instance is forced, nothing is removed.
            for (jj, _) in c.instances.iter().enumerate() {
                forced.insert(DirectSolveNode::Instance(ci, jj));
            }
        }
        let mut dropped_versions: Vec<String> = Vec::new();
        for (jj, inst) in c.instances.iter().enumerate() {
            if inst.installed {
                continue;
            }
            if forced.contains(&DirectSolveNode::Instance(ci, jj)) {
                continue;
            }
            // The take-over version: the first surviving same-cp
            // instance (installed, or a forced merge) in conflict
            // order -- never the removed instance itself. Empty when
            // every instance drops (degenerate: no edge reached
            // anything); then the entry is deleted outright with no
            // notice owed (there is no keeper to report against).
            let kept = c.instances.iter().enumerate().find(|(kk, i)| {
                *kk != jj && (i.installed || forced.contains(&DirectSolveNode::Instance(ci, *kk)))
            });
            dropped_versions.push(inst.version.clone());
            if let Some((_, k)) = kept {
                removed.push((
                    c.category.clone(),
                    c.package.clone(),
                    inst.version.clone(),
                    k.version.clone(),
                ));
            } else {
                removed.push((
                    c.category.clone(),
                    c.package.clone(),
                    inst.version.clone(),
                    String::new(),
                ));
            }
            // Missed-update recording (`:2063-2106`): for the removed
            // instance, every parent of every *other* instance whose
            // atom does not accept it. No `is_arg_parent` gate here,
            // exactly like real.
            for (kk, other) in c.instances.iter().enumerate() {
                if kk == jj {
                    continue;
                }
                for p in &other.parents {
                    if direct_solve_atom_matches(&input, c, jj, &p.atom, false) {
                        continue;
                    }
                    // Backlog #230: the slot instances' own
                    // `use_display` is the slot-collision notice's
                    // IUSE-only rendering; the skipped block renders
                    // real's full `pkg_use_display` instead (recomputed
                    // here -- the notice keeps its own).
                    let skipped_use = skipped_update_use_display_for(
                        input.repos,
                        input.config,
                        &c.category,
                        &c.package,
                        &inst.version,
                    );
                    // #270: a parent no merge entry replaces, whose installed
                    // instance carries the atom, is real's installed
                    // (nomerge) node -- `installed in '<root>'`, vdb
                    // slot/repo, like the #205 rows of
                    // `backtrack_missed_updates` -- not a merge-scheduled
                    // one. A merge-scheduled parent keeps the tree form.
                    let parent_bare = p.parent_cpv.split(':').next().unwrap_or("");
                    let (parent_cpv_disp, parent_installed) = if p.installed {
                        (p.parent_cpv.clone(), true)
                    } else if let Some((pc, pp, pv)) = split_cpv(parent_bare)
                        && !input.entries.iter().any(|e| {
                            e.category == pc
                                && e.package == pp
                                && merge_bound_version(&e.outcome) == Some(&pv)
                        })
                        && let Some(r) = installed_refs(input.root, &pc, &pp)
                            .into_iter()
                            .find(|r| r.version == pv)
                    {
                        (
                            format!("{pc}/{pp}-{pv}:{}/{}::{}", r.slot, r.sub_slot, r.repo),
                            true,
                        )
                    } else {
                        (p.parent_cpv.clone(), false)
                    };
                    let consumer_use = skipped_consumer_use_display(
                        input.repos,
                        input.root,
                        input.config,
                        &parent_cpv_disp,
                        parent_installed,
                    );
                    // Backlog #242 Slice D: per-node roots (the missed
                    // instance rides the rejecting parent's edge; a merge
                    // parent rides its entry; installed parents ride the
                    // walk root — always the target walk here).
                    let (missed_running, consumer_running) = if parent_installed {
                        (false, false)
                    } else {
                        let parent = p.parent_cpv.split(':').next().unwrap_or("");
                        match split_cpv(parent) {
                            Some((cc, cp2, cv)) => skipped_update_roots(
                                input.entries,
                                (&c.category, &c.package),
                                (&cc, &cp2, &cv),
                                &p.atom,
                            ),
                            None => (false, false),
                        }
                    };
                    skipped.push(SkippedUpdate {
                        category: c.category.clone(),
                        package: c.package.clone(),
                        slot: c.slot.clone(),
                        skipped_version: inst.version.clone(),
                        skipped_sub_slot: inst.sub_slot.clone(),
                        skipped_repo: inst.repo_name.clone(),
                        skipped_use,
                        atom: p.atom.clone(),
                        consumer_cpv: parent_cpv_disp,
                        consumer_installed: parent_installed,
                        consumer_use,
                        missed_targets_running_root: missed_running,
                        consumer_targets_running_root: consumer_running,
                    });
                }
            }
        }
        let kept: Vec<SlotConflictInstance> = c
            .instances
            .iter()
            .filter(|i| i.installed || !dropped_versions.iter().any(|v| v == &i.version))
            .cloned()
            .collect();
        if kept.len() >= 2 {
            let mut rest = c.clone();
            rest.instances = kept;
            rest.resolved_version = rest.instances[0].version.clone();
            surviving.push(rest);
        }
    }
    // #270: blocks in the order their missed package was first recorded
    // (real renders its `_conflict_missed_update` dict in insertion
    // order, `depgraph.py:1529-1565`); only the parents inside one block
    // are sorted -- real iterates a set there, so any fixed order is as
    // good, and a stable one keeps the output deterministic.
    let mut first_seen: Vec<(String, String, String)> = Vec::new();
    for s in &skipped {
        let k = (
            s.category.clone(),
            s.package.clone(),
            s.skipped_version.clone(),
        );
        if !first_seen.contains(&k) {
            first_seen.push(k);
        }
    }
    let block_rank = |s: &SkippedUpdate| -> usize {
        first_seen
            .iter()
            .position(|k| k.0 == s.category && k.1 == s.package && k.2 == s.skipped_version)
            .unwrap_or(usize::MAX)
    };
    skipped.sort_by(|a, b| {
        (block_rank(a), &a.atom, &a.consumer_cpv).cmp(&(block_rank(b), &b.atom, &b.consumer_cpv))
    });
    skipped.dedup();
    DirectSolveOutput {
        removed,
        skipped,
        surviving,
    }
}
