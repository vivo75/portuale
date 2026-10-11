//! Choosing the next nodes to merge.
//! Portage: _emerge/depgraph.py, _serialize_tasks

use crate::merge_order::*;

pub(crate) fn select_nodes(
    g: &mut Digraph,
    entries: &[GraphEntry],
    root: &Path,
    mut tree: Option<&mut TreeStuck>,
) -> Vec<usize> {
    let mut retlist: Vec<usize> = Vec::new();
    let mut asap: Vec<usize> = seed_toolchain_asap(entries);
    let mut prefer_asap = true;
    let mut drop_satisfied = false;
    // The leaf frontier, kept in sync at the one removal site below.
    // `None` falls back to the plain scans.
    let mut frontier: Option<SerializeFrontier> =
        frontier_enabled().then(|| SerializeFrontier::build(g));
    let mut mo_iter: usize = 0;

    // Uninstall scheduling, virtualized as leaf-selection rules (the frontier never
    // sees an edge change): an owner of a live unadmitted removal waits, and the
    // first stuck round admits removals. No removal present: both rules are vacuous.
    // Portage: depgraph.py, _validate_blockers through _serialize_tasks
    let removals: Vec<usize> = (0..entries.len())
        .filter(|&i| matches!(entries[i].outcome, PretendOutcome::Uninstall { .. }))
        .collect();
    // Static (removal, owner) pairs; liveness is checked per round.
    let removal_owners: Vec<(usize, usize)> =
        removals
            .iter()
            .filter_map(|&r| {
                let rcpv = match &entries[r].outcome {
                    PretendOutcome::Uninstall { version } => {
                        format!("{}/{}-{version}", entries[r].category, entries[r].package)
                    }
                    _ => return None,
                };
                entries
                    .iter()
                    .position(|e| {
                        matches!(
                    e.outcome,
                    PretendOutcome::New { .. }
                        | PretendOutcome::Upgrade { .. }
                        | PretendOutcome::Downgrade { .. }
                        | PretendOutcome::Reinstall { .. }
                ) && entries[r].required_by.iter().any(|(cc, pp)| {
                    *cc == e.category && *pp == e.package
                }) && e.blockers.iter().any(|b| {
                    matches!(
                        b.satisfied_by,
                        Some(BlockerSatisfiedBy::Uninstall { ref cpv, .. }) if cpv == &rcpv
                    )
                })
                    })
                    .map(|o| (r, o))
            })
            .collect();
    let mut scheduled: HashSet<usize> = HashSet::new();

    // One `MO_NODES` snapshot of the post-prune graph for the trace aligner.
    if mo_sel_enabled() {
        let nodes: Vec<String> = g
            .order
            .iter()
            .filter(|&&i| g.alive[i])
            .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
            .collect();
        eprintln!("MO_NODES count={} {}", nodes.len(), nodes.join(" "));
    }

    while g.order.iter().any(|&i| g.alive[i]) {
        mo_iter += 1;
        let mut selected: Option<Vec<usize>> = None;
        let mut used_ig: Option<Ignore> = None;
        let asap_active = prefer_asap && !asap.is_empty();
        let range: &PriorityRange = if asap_active { &SATISFIED } else { &NORMAL };
        // Owners waiting on their unadmitted removal; admission lifts them out.
        let waiting: HashSet<usize> = removal_owners
            .iter()
            .filter(|(r, _)| !scheduled.contains(r) && g.alive[*r])
            .map(|(_, o)| *o)
            .filter(|o| g.alive[*o])
            .collect();
        // An admitted removal waits on its owner in tree mode (the reversed edge).
        // Tree-mode only; termination is safe (the fallbacks ignore this set).
        let admitted_waiting: HashSet<usize> = if tree.is_some() {
            removal_owners
                .iter()
                .filter(|(r, o)| {
                    scheduled.contains(r) && !scheduled.contains(o) && g.alive[*r] && g.alive[*o]
                })
                .map(|(r, _)| *r)
                .collect()
        } else {
            HashSet::new()
        };
        // Waiting owners, unadmitted removals and admitted-waiting removals stay
        // out of the cycle search and every leaf set.
        let ineligible: HashSet<usize> = waiting
            .iter()
            .copied()
            .chain(
                removals
                    .iter()
                    .copied()
                    .filter(|r| !scheduled.contains(r) && g.alive[*r]),
            )
            .chain(admitted_waiting.iter().copied())
            .collect();

        if asap_active {
            asap.retain(|&i| g.alive[i]);
            'asap: for i in 1..=range.medium_soft {
                let ig = range.ig(i);
                for (pos, &node) in asap.iter().enumerate() {
                    if !waiting.contains(&node)
                        && !admitted_waiting.contains(&node)
                        && is_leaf_via(frontier.as_ref(), g, node, ig)
                    {
                        selected = Some(vec![node]);
                        used_ig = ig;
                        asap.remove(pos);
                        break 'asap;
                    }
                }
            }
        }

        if selected.is_none() && !(prefer_asap && !asap.is_empty()) {
            for i in 0..=range.medium_soft {
                let ig = range.ig(i);
                let nodes = leaves_via(frontier.as_mut(), g, ig);
                // Waiting owners and waiting removals never compete.
                let rest: Vec<usize> = nodes
                    .iter()
                    .copied()
                    .filter(|n| {
                        !waiting.contains(n)
                            && !admitted_waiting.contains(n)
                            && (scheduled.contains(n) || !removals.contains(n))
                    })
                    .collect();
                if rest.is_empty() {
                    continue;
                }
                // At rung `NONE` pop every leaf at once; tree mode pops one so the
                // stuck branch below can fire. A mixed merge/uninstall round takes
                // the uninstalls first.
                let mut rest = rest;
                let good_uninstalls: Vec<usize> = if rest.len() > 1 {
                    rest.iter()
                        .copied()
                        .filter(|&node| {
                            matches!(entries[node].outcome, PretendOutcome::Uninstall { .. })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let has_good_uninstalls = !good_uninstalls.is_empty();
                if has_good_uninstalls {
                    rest = good_uninstalls;
                }
                if has_good_uninstalls
                    || rest.len() == 1
                    || (ig.is_none() && asap.is_empty() && tree.is_none())
                {
                    selected = Some(rest);
                } else {
                    // One node only: prefer a child of an asap node, else a node with a parent.
                    let mut picked = None;
                    if !asap.is_empty() {
                        picked = rest.iter().copied().find(|&node| {
                            g.parents[node].iter().any(|&p| {
                                g.alive[p]
                                    && asap.contains(&p)
                                    && g.children[p].iter().any(|(c, prios)| {
                                        *c == node
                                            && range
                                                .ig_medium_soft()
                                                .is_none_or(|f| prios.iter().any(|q| !f.ignores(q)))
                                    })
                            })
                        });
                    }
                    if picked.is_none() {
                        picked = rest.iter().copied().find(|&node| g.has_parents(node));
                    }
                    selected = picked.map(|p| vec![p]);
                }
                if selected.is_some() {
                    used_ig = ig;
                    break;
                }
            }
        }

        if selected.is_none() {
            let mut ranges: Vec<&PriorityRange> = Vec::new();
            if !std::ptr::eq(range, &NORMAL) {
                ranges.push(&NORMAL);
            }
            ranges.push(range);
            if drop_satisfied && !std::ptr::eq(range, &SATISFIED) {
                ranges.push(&SATISFIED);
            }
            for lr in ranges {
                if let Some((sub, ig)) = find_smallest_cycle(FindSmallestCycleArgs {
                    g,
                    frontier: frontier.as_mut(),
                    entries,
                    range: lr,
                    asap: &asap,
                    prefer_asap,
                    ineligible: &ineligible,
                }) {
                    used_ig = ig;
                    // `emerge --pretend --debug`: the cycle's induced subgraph + leaf.
                    if crate::resolver_debug() && sub.len() > 1 {
                        debug_dump_cycle(g, entries, &sub, root);
                    }
                    // One node only, so merge order accounts for as many deps as possible.
                    let sub_leaves: Vec<usize> = g
                        .order
                        .iter()
                        .copied()
                        .filter(|i| {
                            sub.contains(i) && !g.children[*i].iter().any(|(c, _)| sub.contains(c))
                        })
                        .collect();
                    selected = Some(if let Some(&first) = sub_leaves.first() {
                        vec![first]
                    } else if sub.len() > 1 {
                        harvest_cycle(g, &sub)
                    } else {
                        sub.into_iter().collect()
                    });
                    break;
                }
            }
            if selected.is_none() && prefer_asap && !asap.is_empty() {
                // No asap node mergeable: ignore them next iteration.
                prefer_asap = false;
                continue;
            }
        }

        // Promote unsatisfied `medium_post` children freed by this selection to `asap`.
        if let (Some(sel), Some(_)) = (selected.as_ref(), used_ig) {
            let mut promoted: Vec<usize> = Vec::new();
            for &node in sel {
                for (c, prios) in &g.children[node] {
                    if !g.alive[*c] {
                        continue;
                    }
                    let is_medium_post = !prios.iter().any(|p| !s_ignore_runtime_post(p));
                    let is_satisfied_medium_post =
                        !prios.iter().any(|p| !s_ignore_satisfied_runtime_post(p));
                    if is_medium_post && !is_satisfied_medium_post {
                        promoted.push(*c);
                    }
                }
            }
            for c in promoted {
                if sel.contains(&c) || asap.contains(&c) {
                    continue;
                }
                asap.push(c);
            }
        }

        // Stuck branch, tree-mode only: schedule an uninstall for the blocked package.
        // The flat caller never takes it.
        if selected.is_none()
            && let Some(ts) = tree.as_mut()
        {
            tree_schedule_stuck(g, entries, ts);
            if ts.progressed {
                ts.progressed = false;
                prefer_asap = true;
                drop_satisfied = false;
                continue;
            }
        }

        // Admission: no merge node selectable and a live unadmitted removal exists.
        // Admit the first in graph order and re-select; sits before the roots
        // last-resort so a waiting owner is never picked as a root first.
        if selected.is_none()
            && let Some(&r) = removals
                .iter()
                .find(|r| !scheduled.contains(r) && g.alive[**r])
        {
            scheduled.insert(r);
            prefer_asap = true;
            drop_satisfied = false;
            continue;
        }

        // Roots last-resort: isolated leftovers and parentless leaves the scan
        // skipped. Runs before the `drop_satisfied` escalation.
        if selected.is_none() {
            // Same alive-`None`-leaf set, via the level-0 heap instead.
            let mut roots: Vec<usize> = leaves_via(frontier.as_mut(), g, None);
            if !roots.is_empty() {
                // The heap collects the set; the sequence follows `g.order`.
                roots.sort_by_key(|&i| g.order.iter().position(|&j| j == i));
                selected = Some(roots);
            }
        }

        if selected.is_none() && !drop_satisfied {
            drop_satisfied = true;
            continue;
        }
        let selected = match selected {
            Some(s) => s,
            // Unbreakable cycle: prefer a node breakable at a run-time edge, else bias order.
            None => vec![
                g.order
                    .iter()
                    .copied()
                    .find(|&i| g.alive[i] && g.is_leaf(i, SATISFIED.ig_medium()))
                    .or_else(|| g.order.iter().copied().find(|&i| g.alive[i]))
                    .expect("loop condition guarantees one"),
            ],
        };

        if mo_sel_enabled() {
            let retlist_merges = retlist
                .iter()
                .filter(|&&i| {
                    !matches!(
                        entries[i].outcome,
                        PretendOutcome::AlreadyInstalled { .. }
                            | PretendOutcome::NoVisibleCandidate
                    )
                })
                .count();
            let alive = g.order.iter().filter(|&&i| g.alive[i]).count();
            let pick: Vec<String> = selected
                .iter()
                .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
                .collect();
            let asap_cpvs: Vec<String> = asap
                .iter()
                .filter(|&&i| g.alive[i])
                .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
                .collect();
            eprintln!(
                "{}",
                mo_sel_trace_line(MoSelTraceLineArgs {
                    iter: mo_iter,
                    retlist: retlist_merges,
                    alive,
                    asap: &asap_cpvs,
                    prefer_asap,
                    drop_satisfied,
                    ig: used_ig,
                    pick: &pick
                })
            );
        }
        prefer_asap = true;
        drop_satisfied = false;
        for i in selected {
            if !g.alive[i] {
                continue;
            }
            g.alive[i] = false;
            // Keep the frontier in sync at the loop's one removal site.
            if let Some(fr) = frontier.as_mut() {
                fr.remove(i, &g.children[i], &g.parents[i]);
            }
            // Selecting the blocked package while its uninstall is scheduled solves
            // the blocker; selecting the synthetic uninstall itself discards it.
            if let Some(ts) = tree.as_mut() {
                tree_note_selection(g, entries, ts, i);
            }
            retlist.push(i);
        }
        g.order.retain(|&i| g.alive[i]);
    }
    retlist
}
