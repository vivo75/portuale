//! Solves slot conflicts directly, without backtracking.

use crate::*;
use std::ops::ControlFlow;

/// Unconditional direct solve of solvable slot conflicts (no backtracking
/// gate: the capture says `backtrack: 0/0`, zero retries, yet reconciled).
// Portage: depgraph.py, _solve_non_slot_operator_slot_conflicts
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
    /// than deleted: Portage merges the kept instance (its row stays),
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

/// Whether `atom_text` accepts conflict `c`'s instance `inst_idx`.
/// `for_skipped` selects Portage's plain-match form used for the
/// missed-update recording (no `is_arg_parent` gate there).
// Portage: depgraph.py, _solve_non_slot_operator_slot_conflicts (parent, atom matching)
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
    // A bare `cat/pkg:=` matches its own conflict's every instance: Portage
    // matches the raw atom structurally while the list matcher does not
    // implement the built operator. Slot and cp always agree on a
    // per-conflict edge; version/USE never discriminate a same-slot `:=` pull.
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
    // A *versioned* bare `:=` atom still discriminates by version, so drop
    // the operator and match the rest.
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

/// Resolved USE (enabled) plus declared IUSE for a conflict instance: merge
/// entries read their display flags, shadowed merges the tree candidate,
/// installed instances the vdb record.
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

/// Whether the installed instance is excluded from every parent's match
/// list: the cp was explicitly requested, without `--exclude` and outside
/// selective mode.
// Portage: depgraph.py, _want_installed_pkg
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
        .any(|ex| matches_cli_option_atom(ex, &probe))
}

pub(crate) fn direct_solve_slot_conflicts(input: DirectSolveInput<'_>) -> DirectSolveOutput {
    // Insertion order for `or_tuple` first-pulled: merge instances in walk
    // order, installed ones after.
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
    // All conflict members, for the non-conflict test: a parent naming a
    // conflict instance draws a package-to-package edge instead of the
    // sink substitution.
    let mut removed: Vec<(String, String, String, String)> = Vec::new();
    let mut skipped: Vec<SkippedUpdate> = Vec::new();
    let mut surviving: Vec<SlotConflict> = Vec::new();
    'conflict: for (ci, c) in input.conflicts.iter().enumerate() {
        if solve_one_conflict(SolveOneConflictArgs {
            c,
            input: &input,
            surviving: &mut surviving,
            order: &order,
            ci,
            removed: &mut removed,
            skipped: &mut skipped,
        })
        .is_break()
        {
            continue 'conflict;
        }
    }
    // Blocks in first-recorded order; parents inside one block are sorted
    // for a deterministic render.
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

/// The values [`solve_one_conflict`] reads from its caller.
pub(crate) struct SolveOneConflictArgs<'a> {
    pub(crate) c: &'a SlotConflict,
    pub(crate) input: &'a DirectSolveInput<'a>,
    pub(crate) surviving: &'a mut Vec<SlotConflict>,
    pub(crate) order: &'a HashMap<(usize, usize), usize>,
    pub(crate) ci: usize,
    pub(crate) removed: &'a mut Vec<(String, String, String, String)>,
    pub(crate) skipped: &'a mut Vec<SkippedUpdate>,
}

/// Solve one slot conflict: protect, remove or skip its instances; Break means the conflict survives as it is.
pub(crate) fn solve_one_conflict(args: SolveOneConflictArgs<'_>) -> ControlFlow<()> {
    let SolveOneConflictArgs {
        c,
        input,
        surviving,
        order,
        ci,
        removed,
        skipped,
    } = args;
    // Slot-operator rebuild conflicts and need-rebuild-trailer conflicts
    // never enter the direct solve; they survive as-is.
    let stuck_rebuild_parent = c.instances.iter().flat_map(|i| &i.parents).any(|p| {
        let Some(atom) = portage_dep::parse_atom(&p.atom) else {
            return false;
        };
        if !is_built_slot_op(&atom) {
            return false;
        }
        // Strip the slot/repo suffix first.
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
            .any(|ex| matches_cli_option_atom(ex, &cpv))
            || useoldpkg_atom_matches(&pc, &pp, &pv)
    });
    if stuck_rebuild_parent
        || input
            .replace_cps
            .contains(&(c.category.clone(), c.package.clone()))
    {
        surviving.push(c.clone());
        return ControlFlow::Break(());
    }
    let arg_mode = direct_solve_arg_mode(input, c);
    // Edges: (parent node, matched instance indices).
    let mut edges: Vec<(DirectSolveNode, Vec<usize>)> = Vec::new();
    let mut protect_all = false;
    for (ii, inst) in c.instances.iter().enumerate() {
        let _ = ii;
        for p in &inst.parents {
            let parent_node = match parent_identity(&p.parent_cpv) {
                Some((pc, pp, pv)) => member_of(&(pc.clone(), pp.clone()), &pv, p.installed, input)
                    .map_or(DirectSolveNode::NonConflict, |(mci, mii)| {
                        DirectSolveNode::Instance(mci, mii)
                    }),
                // A top-level `(Argument)` pull: no package parent, so the
                // shared sink; its atom still matches normally below.
                None => DirectSolveNode::NonConflict,
            };
            let mut matched: Vec<usize> = Vec::new();
            for (jj, _) in c.instances.iter().enumerate() {
                if direct_solve_atom_matches(input, c, jj, &p.atom, arg_mode) {
                    matched.push(jj);
                }
            }
            if matched.is_empty() {
                // No instance matches: protect the whole conflict from
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
    let forced = protect_forced_instances(ProtectForcedInstancesArgs {
        protect_all,
        children: &children,
        tuples: &tuples,
        order,
        c,
        ci,
    });
    let mut dropped_versions: Vec<String> = Vec::new();
    pair_instance_with_others(PairInstanceWithOthersArgs {
        c,
        forced: &forced,
        ci,
        dropped_versions: &mut dropped_versions,
        removed,
        input,
        skipped,
    });
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
    ControlFlow::Continue(())
}

/// The values [`protect_forced_instances`] reads from its caller.
pub(crate) struct ProtectForcedInstancesArgs<'a> {
    pub(crate) protect_all: bool,
    pub(crate) children: &'a HashMap<DirectSolveNode, Vec<DirectSolveNode>>,
    pub(crate) tuples: &'a [Vec<(usize, usize)>],
    pub(crate) order: &'a HashMap<(usize, usize), usize>,
    pub(crate) c: &'a SlotConflict,
    pub(crate) ci: usize,
}

/// Explore from the non-conflict sink and protect every instance a forced edge reaches.
pub(crate) fn protect_forced_instances(
    args: ProtectForcedInstancesArgs<'_>,
) -> HashSet<DirectSolveNode> {
    let ProtectForcedInstancesArgs {
        protect_all,
        children,
        tuples,
        order,
        c,
        ci,
    } = args;
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
        // `or_tuple`s resolve only once every plain package had its chance:
        // an already-forced member satisfies the dependency, otherwise the
        // first-pulled member wins.
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
        // Dependencies of forced packages join the set.
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
    forced
}

/// The values [`pair_instance_with_others`] reads from its caller.
pub(crate) struct PairInstanceWithOthersArgs<'a> {
    pub(crate) c: &'a SlotConflict,
    pub(crate) forced: &'a HashSet<DirectSolveNode>,
    pub(crate) ci: usize,
    pub(crate) dropped_versions: &'a mut Vec<String>,
    pub(crate) removed: &'a mut Vec<(String, String, String, String)>,
    pub(crate) input: &'a DirectSolveInput<'a>,
    pub(crate) skipped: &'a mut Vec<SkippedUpdate>,
}

/// For each conflict instance, record the other instances' parents it blocks and the updates skipped.
pub(crate) fn pair_instance_with_others(args: PairInstanceWithOthersArgs<'_>) {
    let PairInstanceWithOthersArgs {
        c,
        forced,
        ci,
        dropped_versions,
        removed,
        input,
        skipped,
    } = args;
    for (jj, inst) in c.instances.iter().enumerate() {
        if inst.installed {
            continue;
        }
        if forced.contains(&DirectSolveNode::Instance(ci, jj)) {
            continue;
        }
        // The take-over version: the first surviving same-cp instance in
        // conflict order, never the removed instance itself.
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
        // Missed-update recording: for the removed instance, every parent
        // of every *other* instance whose atom does not accept it.
        for (kk, other) in c.instances.iter().enumerate() {
            if kk == jj {
                continue;
            }
            for p in &other.parents {
                if direct_solve_atom_matches(input, c, jj, &p.atom, false) {
                    continue;
                }
                // The skipped block renders the full display; the notice
                // keeps its own IUSE-only rendering.
                let skipped_use = skipped_update_use_display_for(
                    input.repos,
                    input.config,
                    &c.category,
                    &c.package,
                    &inst.version,
                );
                // A parent no merge entry replaces, whose installed instance
                // carries the atom, renders as an installed node (vdb
                // slot/repo). A merge-scheduled parent keeps the tree form.
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
                // The missed instance rides the rejecting parent's edge; a
                // merge parent rides its entry.
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
}

/// Finds the conflict instance of the given package, version and kind.
fn member_of(
    cp: &(String, String),
    ver: &str,
    installed: bool,
    input: &DirectSolveInput<'_>,
) -> Option<(usize, usize)> {
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
}

/// Splits a parent's `cpv:slot::repo` into category, package and version.
fn parent_identity(parent_cpv: &str) -> Option<(String, String, String)> {
    // Strip the slot/repo suffix `split_cpv` cannot read.
    split_cpv(parent_cpv.split(':').next().unwrap_or(""))
}
