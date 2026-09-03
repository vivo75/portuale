//! Circular dependencies: hard-cycle detection, restart triggers, virtual
//! cycles and USE-fix suggestions.

use crate::*;

/// Whether a fresh single-node self-loop closed by a kept `||` self-branch
/// may retry with the branch demoted. Multi-node rings, inline self-deps
/// and installed instances keep the report-no-retry path.
// Portage: dep_check.py, preferred_in_graph (pass-1 self choice)
pub(crate) fn cycle_restartable(
    entries: &[GraphEntry],
    kept: &[HashSet<usize>],
    cycle: &[String],
    root: &Path,
) -> bool {
    let [node_cpv] = cycle else {
        return false;
    };
    let Some((cat, pkg, _)) = split_cpv(node_cpv) else {
        return false;
    };
    // Fresh only: the demoted retry must never pull installed subgraphs.
    if !installed_candidates(root, &cat, &pkg).is_empty() {
        return false;
    }
    let Some(owner_idx) = entries.iter().position(|e| {
        e.category == cat && e.package == pkg && merge_bound_cpv(e).as_deref() == Some(node_cpv)
    }) else {
        return false;
    };
    let Some(owner_cpv) = merge_bound_cpv(&entries[owner_idx]) else {
        return false;
    };
    entries[owner_idx].deps.iter().enumerate().any(|(ei, d)| {
        d.alt.is_some()
            && kept[owner_idx].contains(&ei)
            && d.category == cat
            && d.package == pkg
            && !d.priority.optional
            && portage_dep::match_from_list(&d.atom, &[owner_cpv.as_str()])
                .is_some_and(|m| !m.is_empty())
    })
}

/// Whether the owner's kept `deps` edges name and match its own merge-bound
/// instance. Self-loops touching an installed instance stay invisible:
/// reporting one would flip an outcome-correct resolve into a spurious
/// circular abort.
pub(crate) fn owner_self_loop(
    entries: &[GraphEntry],
    kept: &[std::collections::HashSet<usize>],
    oi: usize,
    root: &Path,
) -> bool {
    let Some(owner_cpv) = merge_bound_cpv(&entries[oi]) else {
        return false;
    };
    if !installed_candidates(root, &entries[oi].category, &entries[oi].package).is_empty() {
        return false;
    }
    entries[oi].deps.iter().enumerate().any(|(ei, d)| {
        (d.alt.is_none() || kept[oi].contains(&ei))
            && d.category == entries[oi].category
            && d.package == entries[oi].package
            && !d.priority.optional
            && portage_dep::match_from_list(&d.atom, &[owner_cpv.as_str()])
                .is_some_and(|m| !m.is_empty())
    })
}

pub(crate) fn find_hard_cycles(
    entries: &[GraphEntry],
    edge_kind_map: &EdgeKindMap,
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
    insertion_rank: Option<&[usize]>,
) -> Vec<Vec<String>> {
    // Finds the shortest unbreakable cycle: hard edges only (unsatisfied
    // build-time with no run-time alternative, plus unsatisfied
    // non-optional slot-operator edges). Empty when acyclic.
    // Portage: resolver/circular_dependency.py, _find_cycles + shortest_cycle
    // Merge-bound entries only, lowest index per cp.
    let cp_index = merge_bound_index(entries);
    // The per-(root, cp) resolution view; single-root graphs use the cp fallback.
    let root_index = merge_bound_root_index(entries);
    // Slot-op arm: an unsatisfied non-optional slot-operator `deps` edge on
    // the selected `||` branch counts too (Portage's digraph holds the
    // resolved edge only), under the installed gate.
    let kept = kept_alt_branches(entries, root, circular);
    let slot_op_hard = |owner_idx: usize, dep_idx: usize| -> bool {
        entries[owner_idx].deps.iter().enumerate().any(|(ei, d)| {
            (d.alt.is_none() || kept[owner_idx].contains(&ei))
                && d.category == entries[dep_idx].category
                && d.package == entries[dep_idx].package
                && (d.priority.buildtime_slot_op || d.priority.runtime_slot_op)
                && !d.priority.optional
                && !dep_edge_satisfied_by_installed(root, d, Some(&entries[dep_idx]))
        })
    };
    // adjacency: owner index -> sorted, deduped dep indices (hard edges).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    for ((dep_cp, owner_cp), (has_hard, has_soft)) in edge_kind_map {
        let (Some(oi), Some(di)) = (
            root_cp_lookup(&root_index, &cp_index, owner_cp),
            root_cp_lookup(&root_index, &cp_index, dep_cp),
        ) else {
            continue;
        };
        // The walk's `(true, false)` arm, or the slot-op deps arm (an `RDEPEND`
        // `:=` edge is never `buildtime_hard`, so the map alone can never see it).
        let hard = (*has_hard && !*has_soft) || slot_op_hard(oi, di);
        if oi == di {
            // A same-cp edge is a cycle only when it targets the owner's own
            // instance.
            if hard && owner_self_loop(entries, &kept, oi, root) {
                adj[oi].push(oi);
            }
            continue;
        }
        if hard {
            adj[oi].push(di);
        }
    }
    // A slot-op deps edge the walk never recorded has no map entry at all --
    // it still counts on the selected branch.
    for &oi in cp_index.values() {
        for (ei, dep) in entries[oi].deps.iter().enumerate() {
            if dep.alt.is_some() && !kept[oi].contains(&ei) {
                continue;
            }
            if !(dep.priority.buildtime_slot_op || dep.priority.runtime_slot_op)
                || dep.priority.optional
            {
                continue;
            }
            // The child's resolving-root instance; the cp fallback is the
            // single-root behaviour.
            let Some(di) = dep_child_lookup(
                &root_index,
                &cp_index,
                entries[oi].targets_running_root,
                dep,
            ) else {
                continue;
            };
            // Same true-self-loop gate as the map arm above.
            if di == oi && !owner_self_loop(entries, &kept, oi, root) {
                continue;
            }
            if !dep_edge_satisfied_by_installed(root, dep, Some(&entries[di]))
                && !adj[oi].contains(&di)
            {
                adj[oi].push(di);
            }
        }
    }
    // Order analogue of the digraph's node insertion order: the scheduler's
    // pre-bias rank when threaded, else the resolver's `discovery` stamp.
    let order_key = |i: usize| -> (usize, usize) {
        match insertion_rank {
            Some(ranks) => (ranks[i], i),
            None => (entries[i].discovery, i),
        }
    };
    for v in &mut adj {
        // Insertion order, not array position.
        v.sort_by_key(|&i| order_key(i));
        v.dedup();
    }
    // Shortest directed cycle, chosen the way Portage's
    // `circular_dependency_handler` chooses what to print: per node, close a
    // cycle through each child with the shortest child-`>`node path and keep
    // the first strictly-shortest one; print from its `[0]`.
    //
    // Only the display's start node changes: the winning length is the same
    // global minimum, and empty stays empty.
    let shortest_to = |from: usize, to: usize| -> Option<Vec<usize>> {
        if from == to {
            return Some(vec![from]);
        }
        let mut prev: HashMap<usize, usize> = HashMap::new();
        let mut seen: HashSet<usize> = HashSet::from([from]);
        let mut frontier = vec![from];
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for &u in &frontier {
                for &w in &adj[u] {
                    if w == to {
                        // Reconstruct from -> ... -> u, then wrap to `to`.
                        let mut path = vec![u];
                        let mut cur = u;
                        while cur != from {
                            cur = prev[&cur];
                            path.push(cur);
                        }
                        path.reverse();
                        path.push(to);
                        return Some(path);
                    }
                    if seen.insert(w) {
                        prev.insert(w, u);
                        next.push(w);
                    }
                }
            }
            frontier = next;
        }
        None
    };
    let mut best: Option<Vec<usize>> = None;
    // Insertion rank order, not array position.
    let mut by_insertion: Vec<usize> = (0..entries.len()).collect();
    by_insertion.sort_by_key(|&i| order_key(i));
    for &start in &by_insertion {
        if adj[start].is_empty() {
            continue;
        }
        let mut node_best: Option<Vec<usize>> = None;
        for &child in &adj[start] {
            if let Some(path) = shortest_to(child, start)
                && node_best.as_ref().is_none_or(|b| path.len() < b.len())
            {
                node_best = Some(path);
            }
        }
        if let Some(candidate) = node_best
            && best.as_ref().is_none_or(|b| candidate.len() < b.len())
        {
            best = Some(candidate);
        }
    }
    match best {
        None => Vec::new(),
        Some(cycle) => {
            vec![
                cycle
                    .into_iter()
                    .map(|i| merge_bound_cpv(&entries[i]).expect("merge-bound"))
                    .collect(),
            ]
        }
    }
}

/// One label per edge of a `find_hard_cycles` cycle: `labels[i]` is the edge
/// `cycle[i] -> cycle[(i + 1) % len]`, rendered from the hardest `deps`
/// priority the owner carries toward the dep's cp.
// Portage: resolver/circular_dependency.py, _prepare_circular_dep_message
pub fn cycle_edge_labels(entries: &[GraphEntry], cycle: &[String]) -> Vec<String> {
    if cycle.is_empty() {
        return Vec::new();
    }
    cycle
        .iter()
        .enumerate()
        .map(|(i, owner_cpv)| {
            let dep_cpv = &cycle[(i + 1) % cycle.len()];
            let hardest = entries
                .iter()
                .find(|e| merge_bound_cpv(e).as_deref() == Some(owner_cpv.as_str()))
                .into_iter()
                .flat_map(|e| e.deps.iter())
                .filter_map(|d| {
                    split_cpv(dep_cpv).and_then(|(cat, pkg, _)| {
                        (d.category == cat && d.package == pkg).then_some(d.priority)
                    })
                })
                .max_by_key(crate::resolver_trace::dep_priority_rank);
            hardest
                .map(|p| crate::resolver_trace::dep_priority_str(&p).to_string())
                .unwrap_or_else(|| "buildtime".to_string())
        })
        .collect()
}

/// Splits `category/package-version` into its three parts (the last `-`
/// boundary whose right half verifies as a version).
pub fn split_cpv(cpv: &str) -> Option<(String, String, String)> {
    let (cat, rest) = cpv.split_once('/')?;
    let parts: Vec<&str> = rest.split('-').collect();
    for i in 1..parts.len() {
        let version = parts[i..].join("-");
        if portage_versions::ververify(&version) {
            return Some((cat.to_string(), parts[..i].join("-"), version));
        }
    }
    None
}

/// Every USE flag name referenced anywhere in a `REQUIRED_USE` string.
// Portage: portage.dep, get_required_use_flags
pub(crate) fn required_use_flag_names(required_use: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for tok in required_use.split_whitespace() {
        let t = tok.strip_suffix('?').unwrap_or(tok);
        let t = t.strip_prefix('!').unwrap_or(t);
        if matches!(t, "(" | ")" | "||" | "^^" | "??" | "") {
            continue;
        }
        if t.starts_with(|c: char| c.is_ascii_alphanumeric())
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+_@-".contains(c))
        {
            out.insert(t.to_string());
        }
    }
    out
}

/// One entry of the circular-dependency suggestions: a specific `Change
/// USE:` fix that would break the cycle at one edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircularSuggestion {
    /// `category/package-version` of the package whose USE would change.
    pub parent_cpv: String,
    /// `(flag, enable)` diffs from the package's current USE, sorted.
    pub changes: Vec<(String, bool)>,
    /// A grandparent's atom conditionally constrains one of these flags, so
    /// the fix may cascade upward.
    pub followup: bool,
}

/// For each edge of the shortest cycle, the minimal `package.use` changes on
/// the *depending* package that would drop the offending atom from its
/// build-time deps without violating its own `REQUIRED_USE`. Deduped and
/// sorted for a deterministic render.
// Portage: resolver/circular_dependency.py, _find_suggestions
pub fn circular_dep_solutions(
    cycle: &[String],
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    autounmask_use_changes: &[AutounmaskChange],
    entries: &[GraphEntry],
) -> Vec<CircularSuggestion> {
    const MAX_AFFECTING_USE: usize = 10;
    let mut out: Vec<CircularSuggestion> = Vec::new();
    let n = cycle.len();
    if n == 0 {
        return out;
    }

    for pos in 0..n {
        let parent_cpv = &cycle[(pos + n - 1) % n];
        let child_cpv = &cycle[pos];
        let (Some((pcat, ppkg, pver)), Some((ccat, cpkg, _))) =
            (split_cpv(parent_cpv), split_cpv(child_cpv))
        else {
            continue;
        };
        let Ok(pcands) = list_candidates(repos, &pcat, &ppkg) else {
            continue;
        };
        let Some(pc) = pcands.iter().find(|c| c.version == pver) else {
            continue;
        };
        let Ok(md) = repo_aux_metadata(&pc.repo_location, &pcat, &format!("{ppkg}-{pver}")) else {
            continue;
        };
        let get = |k: &str| md.get(k).map(String::as_str).unwrap_or("");
        let dep = format!("{} {}", get("DEPEND"), get("BDEPEND"));
        let dep_tokens: Vec<String> = dep.split_whitespace().map(str::to_string).collect();
        let required_use = get("REQUIRED_USE");
        let iuse_names: HashSet<String> = pc
            .iuse
            .split_whitespace()
            .map(|t| t.trim_start_matches(['+', '-']).to_string())
            .collect();
        let parent_str = format!(
            "{pcat}/{ppkg}-{pver}:{}/{}::{}",
            pc.slot, pc.sub_slot, pc.repo_name
        );
        // The parent USE plus the autounmask-USE overlay; the changed flags
        // themselves stay untouchable below.
        let mut parent_use: HashSet<String> = effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: &pc.iuse,
            keywords: &pc.keywords,
            candidate_str: &parent_str,
            category: &pcat,
            package: &ppkg,
        })
        .as_ref()
        .clone();
        for ch in autounmask_use_changes {
            if matches_config_entry(&ch.atom, &parent_str, &pcat, &ppkg) {
                for tok in ch.token.split_whitespace() {
                    if let Some(flag) = tok.strip_prefix('-') {
                        parent_use.remove(flag);
                    } else {
                        parent_use.insert(tok.trim_start_matches('+').to_string());
                    }
                }
            }
        }

        // `parent_atom`: the (unevaluated) token in `dep` that pulls the child.
        let ccp = format!("{ccat}/{cpkg}");
        let Some(parent_atom) = dep_tokens.iter().find(|t| {
            portage_dep::parse_atom(t)
                .is_some_and(|a| format!("{}/{}", a.category, a.package) == ccp)
        }) else {
            continue;
        };
        let parent_atom = parent_atom.clone();

        let Some(mut affecting) = portage_dep::extract_affecting_use(&dep, &parent_atom) else {
            continue;
        };

        // untouchable = use.mask ∪ use.force ∪ autounmask-changed
        let mut untouchable = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: &pc.iuse,
            keywords: &pc.keywords,
            candidate_str: &parent_str,
            category: &pcat,
            package: &ppkg,
            config,
        });
        for ch in autounmask_use_changes {
            if matches_config_entry(&ch.atom, &parent_str, &pcat, &ppkg) {
                for tok in ch.token.split_whitespace() {
                    untouchable.insert(tok.trim_start_matches(['+', '-']).to_string());
                }
            }
        }
        affecting.retain(|f| !untouchable.contains(f));

        // REQUIRED_USE entanglement: bounded widening to the referenced flags.
        let ruf = required_use_flag_names(required_use);
        if affecting.iter().any(|f| ruf.contains(f)) {
            let mut total: HashSet<String> = affecting.clone();
            total.extend(ruf.iter().cloned());
            total.retain(|f| !untouchable.contains(f));
            if total.len() <= MAX_AFFECTING_USE {
                affecting = total;
            }
        }

        if affecting.is_empty() {
            continue;
        }
        let mut affecting: Vec<String> = affecting.into_iter().collect();
        affecting.sort();
        if affecting.len() > MAX_AFFECTING_USE {
            // Drop irrelevant not-currently-enabled flags.
            affecting.retain(|f| parent_use.contains(f));
            if affecting.len() > MAX_AFFECTING_USE {
                continue;
            }
        }

        // Every enable/disable assignment of the affecting flags whose
        // reduced `dep` no longer contains `parent_atom` and still
        // satisfies `REQUIRED_USE`, recorded as a minimal diff.
        let n_aff = affecting.len();
        let mut solutions: HashSet<Vec<(String, bool)>> = HashSet::new();
        for mask in 0u32..(1u32 << n_aff) {
            let mut cur = parent_use.clone();
            for (i, f) in affecting.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    cur.insert(f.clone());
                } else {
                    cur.remove(f);
                }
            }
            let Ok(reduced) = portage_use_reduce::use_reduce_flat(
                &dep_tokens,
                &cur,
                portage_use_reduce::MatchMode::Normal,
            ) else {
                continue;
            };
            if reduced.contains(&parent_atom) {
                continue;
            }
            if !required_use.trim().is_empty()
                && !matches!(
                    portage_required_use::check_required_use(required_use, &cur, &iuse_names),
                    Ok(true)
                )
            {
                continue;
            }
            let mut sol: Vec<(String, bool)> = Vec::new();
            for (i, f) in affecting.iter().enumerate() {
                let enabled = mask & (1 << i) != 0;
                if enabled && !parent_use.contains(f) {
                    sol.push((f.clone(), true));
                } else if !enabled && parent_use.contains(f) {
                    sol.push((f.clone(), false));
                }
            }
            sol.sort();
            solutions.insert(sol);
        }

        let all: Vec<Vec<(String, bool)>> = solutions.into_iter().collect();
        for sol in &all {
            // Drop a solution that's a strict superset of another.
            let sol_set: HashSet<&(String, bool)> = sol.iter().collect();
            if all
                .iter()
                .any(|o| o != sol && o.iter().all(|x| sol_set.contains(x)))
            {
                continue;
            }
            let (ignore, followup) = grandparent_use_conflict(&pcat, &ppkg, sol, repos, entries);
            if ignore {
                continue;
            }
            out.push(CircularSuggestion {
                parent_cpv: format!("{pcat}/{ppkg}-{pver}"),
                changes: sol.clone(),
                followup,
            });
        }
    }

    out.sort_by(|a, b| {
        (a.parent_cpv.as_str(), &a.changes).cmp(&(b.parent_cpv.as_str(), &b.changes))
    });
    out.dedup();
    out
}

/// Whether a grandparent's own atom on `parent`'s `cat/pkg` constrains a flag
/// in `solution`: a hard `[flag]`/`[-flag]` clash disqualifies it, a
/// conditional one only flags `followup_change`.
pub(crate) fn grandparent_use_conflict(
    pcat: &str,
    ppkg: &str,
    solution: &[(String, bool)],
    repos: &[RepoConfig],
    entries: &[GraphEntry],
) -> (bool, bool) {
    let pcp = format!("{pcat}/{ppkg}");
    let mut followup = false;
    let Some(pe) = entries
        .iter()
        .find(|e| e.category == pcat && e.package == ppkg)
    else {
        return (false, false);
    };
    for (ocat, opkg) in &pe.required_by {
        let Some(over) = entries
            .iter()
            .find(|e| &e.category == ocat && &e.package == opkg)
            .and_then(merge_bound_cpv)
            .and_then(|cpv| split_cpv(&cpv).map(|(_, _, v)| v))
        else {
            continue;
        };
        let Ok(ocands) = list_candidates(repos, ocat, opkg) else {
            continue;
        };
        let Some(oc) = ocands.iter().find(|c| c.version == over) else {
            continue;
        };
        let Ok(omd) = repo_aux_metadata(&oc.repo_location, ocat, &format!("{opkg}-{over}")) else {
            continue;
        };
        for key in ["DEPEND", "BDEPEND", "RDEPEND", "PDEPEND"] {
            let Some(dep) = omd.get(key) else { continue };
            for tok in dep.split_whitespace() {
                let Some(a) = portage_dep::parse_atom(tok) else {
                    continue;
                };
                if format!("{}/{}", a.category, a.package) != pcp {
                    continue;
                }
                let Some(use_deps) = &a.use_deps else {
                    continue;
                };
                for (flag, _) in solution {
                    for ud in use_deps {
                        if &ud.flag != flag {
                            continue;
                        }
                        match ud.op {
                            portage_dep::UseDepOp::Enabled | portage_dep::UseDepOp::Disabled => {
                                return (true, followup);
                            }
                            _ => followup = true,
                        }
                    }
                }
            }
        }
    }
    (false, followup)
}
