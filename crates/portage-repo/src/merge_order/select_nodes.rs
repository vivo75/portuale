//! Choosing the next nodes to merge (Portage `_serialize_tasks`).

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
    // The incremental leaf frontier (see `SerializeFrontier`): built
    // once from the biased order, kept in sync at the one removal site
    // below. `None` when `PORTAGE_SERIALIZE_FRONTIER_DISABLE` falls back
    // to the plain scans.
    let mut frontier: Option<SerializeFrontier> =
        frontier_enabled().then(|| SerializeFrontier::build(g));
    let mut mo_iter: usize = 0;

    // #142 S2 F4b: real's uninstall-scheduling dance
    // (`depgraph.py`, `_validate_blockers` through `_serialize_tasks`):
    // every uninstall task is ordered after its blocker owner (the
    // validation edge — the owner is not a leaf while its removal is
    // unscheduled), an empty selection round admits removals (edges
    // reversed: the removal then waits on its owner), and a round
    // mixing merges and admitted uninstalls takes the uninstalls
    // first (`good_uninstalls`). Portuale files removals only for
    // solved blockers (real's schedulable uninstalls) with the
    // execution edge already in the graph, so the dance is virtualized
    // as two leaf-selection rules instead of graph surgery (the
    // frontier never sees an edge change): an owner of a live
    // unadmitted removal waits (filtered out of every leaf set), and
    // the first stuck round admits removals. No removal present: both
    // rules are vacuous and selection is byte-identical to before.
    //
    // Waiting fires only for the merging arm — the anchor entry
    // carrying the `Uninstall` row naming this removal. A nomerge-arm
    // anchor (B0b/#77 A1: the blocked merge instance) never waits,
    // exactly like real, whose validation edge runs owner-side.
    let removals: Vec<usize> = (0..entries.len())
        .filter(|&i| matches!(entries[i].outcome, PretendOutcome::Uninstall { .. }))
        .collect();
    // Static (removal, merging-arm owner) pairs; liveness is checked
    // per round below.
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

    // B1: one `MO_NODES` snapshot of the post-prune graph, so the aligner
    // can name a membership difference (gtk:4 is alive=398 vs 395 at
    // iteration 1) instead of only reporting the count.
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
        // F4b: owners waiting on their unadmitted removal (the
        // validation edge, virtualized). Recomputed per round: admission
        // below lifts entries out of this set.
        let waiting: HashSet<usize> = removal_owners
            .iter()
            .filter(|(r, _)| !scheduled.contains(r) && g.alive[*r])
            .map(|(_, o)| *o)
            .filter(|o| g.alive[*o])
            .collect();
        // #131 S1: an admitted removal waits on its owner in tree mode.
        // Real reverses the validation edge on admission (the removal then
        // waits on its owner, `depgraph.py:9998-10020`); portuale's
        // virtualization only had the owner-waits half above, which
        // sufficed while tree-mode selection fed verdicts alone. The
        // display order consumes the selection sequence, and without this
        // half an admitted removal (u1's `samepkg` uninstall) schedules
        // ahead of its owner where real schedules the owner first. An
        // owner outside the graph (pruned) or already scheduled lifts the
        // wait -- there is nothing to wait on. Tree-mode only: the flat
        // caller keeps its standing batch behavior, pinned everywhere;
        // porting the reversal flat is reorder work outside this phase.
        // Termination is safe: the roots last-resort and the final
        // fallback below ignore this set.
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
        // Cycle search must not see waiting owners either (real's
        // validation edge keeps them out of its leaf set), nor a
        // removal that was never admitted (defensive: one is only a
        // leaf once its owner is gone, which admission precedes).
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
                // F4b: waiting owners never compete (real's validation
                // edge keeps them out of the leaf set), and an
                // unadmitted removal is only a leaf once its owner is
                // gone — which admission below always precedes, so
                // filter defensively all the same. #131 S1: an admitted
                // removal waiting on its owner (see `admitted_waiting`
                // above) never competes either.
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
                // Real: "Greedily pop all of these nodes since no
                // relationship has been ignored." Real additionally
                // gates this on `not tree_mode`, because the batch
                // "destroys --tree output" (`depgraph.py:9764-9777`).
                // The flat caller (`tree == None`) keeps portuale's
                // standing batch behavior; the #81 tree simulation passes
                // `Some` and takes real's gate, popping one non-root
                // node per round so the stuck branch below can fire --
                // and #131's `tree_display_order` consumes that
                // one-at-a-time sequence as the `--tree` display list,
                // exactly like real's `_ordered_tree_display` consumes
                // the reversed tree-mode retlist.
                //
                // #142 S2 F4: "If there is a mixture of merges and
                // uninstalls, do the uninstalls first"
                // (`depgraph.py`, same selection-round block). Every
                // removal reaching this filter is admitted (F4b
                // admission runs before any owner it waits on can
                // schedule), i.e. one of real's `scheduled_uninstalls`
                // — bed blk0 cells schedule `[X-1, uninstall, blocks,
                // ...]`; the blockerorderpkg T7 pin already expects
                // post-owner placement.
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
                    // "For optimal merge order: only pop one node;
                    // removing a root node (node without a parent) will
                    // not produce a leaf node, so avoid it." Real first
                    // prefers a node whose parent is itself an asap node.
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
                                                .is_none_or(|f| prios.iter().any(|q| !f(q)))
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
                    // `emerge --pretend --debug`: real
                    // `depgraph.py:9917-9930`'s `\nruntime cycle digraph
                    // (<n> nodes):\n\n` + `debug_print()` of the induced
                    // subgraph, then `runtime cycle leaf: <pkg>` -- to
                    // stderr, once per relaxed cycle.
                    if crate::resolver_debug() && sub.len() > 1 {
                        debug_dump_cycle(g, entries, &sub, root);
                    }
                    // "NOTE: This case should only be triggered when
                    // prefer_asap is True... select only one node here,
                    // so that merge order accounts for as many
                    // dependencies as possible."
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
                // "We failed to find any asap nodes to merge, so ignore
                // them for the next iteration."
                prefer_asap = false;
                continue;
            }
        }

        // "Try to merge neglected medium_post deps as soon as possible if
        // they're not satisfied by installed packages." -- real's
        // `PDEPEND`-asap promotion, bug #180045.
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

        // Backlog #81: the stuck branch (`depgraph.py:9998`), tree-mode
        // only. When no leaf is selectable and replacement rows are still
        // pending, real schedules an uninstall for the blocked package
        // (`scheduled_uninstalls`, `:10190`) and reverses its edges so the
        // blocked package merges on top of it; the solved blocker is
        // appended when the blocked package is later selected
        // (`:10351-10358`). It sits here -- after the cycle harvest and
        // the PDEPEND promotion, before the roots-last-resort -- exactly
        // like real's. The flat caller never takes it (`tree == None`).
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

        // #142 S2 F4b admission: real's uninstall-scheduling round
        // (`depgraph.py:9998-10020`, "An Uninstall task needs to be
        // executed in order to avoid conflict if possible"). No merge
        // node is selectable anywhere on the ladder or the cycle paths
        // above, and a live unadmitted removal exists: admit the first
        // in graph order (real admits one uninstall per stuck round —
        // its min-parent_deps choice approximated here by graph order;
        // single-removal runs are exact), reset the round state like
        // real, and re-select. The admitted removal's owner stops
        // waiting (the validation edge reversed); the execution edge
        // already in the graph still orders the removal after it.
        // Sits after the cycle harvest and before the roots
        // last-resort, exactly like real's: a waiting owner must not
        // be picked as a last-resort root while its removal is still
        // unadmitted.
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

        // Real `_serialize_tasks` (`depgraph.py:10230-10231`): "Only
        // select root nodes as a last resort. This case should only
        // trigger when the graph is nearly empty and the only remaining
        // nodes are isolated (no parents or children)." -- but it also
        // catches parentless plain leaves the one-node-at-a-time scan
        // above skipped (it only ever picks a node *with* parents, and
        // once `asap` is non-empty its rung-`NONE` batch is disabled).
        // Real runs this *before* escalating to `drop_satisfied`, so a
        // stub whose only dep is a satisfied-runtime edge is not freed
        // early via `s_ignore_satisfied_runtime`.
        if selected.is_none() {
            // Same set the order scan collected (alive `None`-leaves),
            // via the level-0 heap instead.
            let mut roots: Vec<usize> = leaves_via(frontier.as_mut(), g, None);
            if !roots.is_empty() {
                // Real leaves `ignore_priority` at `None` here, so the
                // `medium_post` PDEPEND-asap promotion above (gated on
                // `ignore_priority is not None`) does not apply -- and it
                // has already run for this iteration regardless.
                // #131 S1: real selects `get_nodes()` here -- its
                // `leaf_nodes()` order, i.e. graph order, not heap order
                // (`depgraph.py:10230-10231`; p2b's round-2 batch is
                // `[newpkg, p2bowner]` in graph order). The heap only
                // collects the set; the sequence follows `g.order`.
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
            // Real raises `_unknown_internal_error` here (an unresolved
            // blocker, or a cycle none of the `ignore_priority` rungs
            // could break). Portuale reports a genuinely unbreakable
            // cycle separately via `find_hard_cycles` and still has to
            // produce a list either way, so it keeps going instead:
            // prefer a node whose every remaining dependency is one the
            // *widest* filter (`DepPrioritySatisfiedRange.ignore_medium`,
            // i.e. everything except an unsatisfied build-time dep) would
            // drop -- the same "break the cycle at a run-time edge"
            // preference the pre-digraph implementation used -- and only
            // fall back to plain bias order when even that finds nothing.
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
            // Keep the frontier in sync at the loop's one removal site
            // (real `_FrontierDigraph.remove`); counts/edges are read off
            // the adjacency as it stands.
            if let Some(fr) = frontier.as_mut() {
                fr.remove(i, &g.children[i], &g.parents[i]);
            }
            // Backlog #81: selecting the blocked package while its
            // uninstall is scheduled solves the blocker (real
            // `:10351-10358` appends it to the retlist here); selecting
            // the synthetic uninstall itself discards it the same way
            // (`myblocker_uninstalls.remove`). Either way the row's
            // verdict is recorded once.
            if let Some(ts) = tree.as_mut() {
                tree_note_selection(g, entries, ts, i);
            }
            retlist.push(i);
        }
        g.order.retain(|&i| g.alive[i]);
    }
    retlist
}
