//! Updates `-u` leaves alone for an installed consumer's `[use]` dependency.

use crate::*;

/// The `[use]` atoms of `depstr` that are not inside a `||` group: a `||` arm
/// the consumer can do without holds nothing back.
fn atoms_outside_any_of(depstr: &str, use_flags: &HashSet<String>) -> Vec<String> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let Ok(reduced) = portage_use_reduce::use_reduce_structured(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut any_depth = 0usize;
    let mut depth = 0usize;
    let mut it = reduced.iter().peekable();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "||" if any_depth == 0 && it.peek().map(|t| t.as_str()) == Some("(") => {
                any_depth = depth + 1;
            }
            "(" => depth += 1,
            ")" => {
                if any_depth == depth {
                    any_depth = 0;
                }
                depth = depth.saturating_sub(1);
            }
            t if any_depth == 0 && !t.ends_with('?') && t != "||" => out.push(t.to_string()),
            _ => {}
        }
    }
    out
}

/// The visible update an installed consumer's `[use]` dependency keeps out of
/// a `-u` plan, with the USE change that would let it in. Only a change the
/// user can make is reported: a flag the profile masks or forces, or one the
/// candidate lacks in `IUSE`, yields nothing.
// Portage: depgraph.py, _show_unsatisfied_dep ("Change USE"); comparison only.
pub(crate) fn held_back_use_updates(
    ctx: &ResolveCtx<'_>,
    config: &portage_profile::Config,
    entries: &[GraphEntry],
) -> Vec<HeldBackUse> {
    if !ctx.update {
        return Vec::new();
    }
    let kept: HashMap<(String, String), String> = entries
        .iter()
        .filter_map(|e| match &e.outcome {
            PretendOutcome::AlreadyInstalled { version } => {
                Some(((e.category.clone(), e.package.clone()), version.clone()))
            }
            _ => None,
        })
        .collect();
    if kept.is_empty() {
        return Vec::new();
    }
    let cps = |keep: fn(&PretendOutcome) -> bool| -> HashSet<(String, String)> {
        entries
            .iter()
            .filter(|e| keep(&e.outcome))
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect()
    };
    let being_replaced: HashSet<(String, String)> = entries
        .iter()
        .filter(|e| merge_bound_cpv(e).is_some())
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    let being_removed = cps(|o| matches!(o, PretendOutcome::Uninstall { .. }));
    let graph_cps = cps(|_| true);
    let no_top_level = HashSet::new();
    let scope = ConsumerScope {
        being_replaced: &being_replaced,
        being_removed: &being_removed,
        reachable: &ctx.world_reachable,
        graph_cps: &graph_cps,
        already_installed_top_level: &no_top_level,
        world_reachable: &ctx.world_reachable,
        excluded: ctx.excluded,
    };
    let dep_keys: &[&str] = if ctx.with_bdeps {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND"]
    };
    let mut memo: MetaMemo = HashMap::new();
    let mut pins: Vec<RevDepPin> = Vec::new();
    for consumer in all_installed_packages(ctx.root).iter() {
        if !consumer_in_scope(&scope, consumer) {
            continue;
        }
        let deps = consumer_deps(
            DepSource {
                root: ctx.root,
                gate: ctx.gate(),
                dep_keys,
                dynamic_deps: ctx.dynamic_deps,
                ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
            },
            &mut memo,
            &consumer.category,
            &consumer.package,
            &consumer.version,
        );
        for (_, depstr) in &deps.strings {
            for tok in atoms_outside_any_of(depstr, &deps.use_flags) {
                let Some(atom) = parse_dep_atom(&tok, "held_back_use") else {
                    continue;
                };
                let cp = (atom.category.clone(), atom.package.clone());
                if atom.blocker != portage_dep::Blocker::None
                    || atom.use_deps.as_ref().is_none_or(|d| d.is_empty())
                    || !kept.contains_key(&cp)
                {
                    continue;
                }
                pins.push(RevDepPin {
                    atom: reverse_dep_constraint_atom(&tok, &atom),
                    raw_atom: tok,
                    cp,
                    built_render: Vec::new(),
                    consumer: (
                        consumer.category.clone(),
                        consumer.package.clone(),
                        consumer.version.clone(),
                    ),
                    holder_open: false,
                });
            }
        }
    }
    let mut out: Vec<HeldBackUse> = Vec::new();
    for row in constraint_withheld_updates(ctx.gate(), ctx.root, config, entries, &pins) {
        let wanted = format!("{}/{}-{}", row.category, row.package, row.skipped_version);
        let Some(rows) = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
            root: ctx.root,
            entries,
            display_atom: &row.atom,
            evaluated_atom: &row.atom,
            owner: None,
            config,
            gate: ctx.gate(),
        }) else {
            continue;
        };
        let Some(changes) = rows
            .iter()
            .filter(|(cpv, _)| cpv.split("::").next() == Some(wanted.as_str()))
            .flat_map(|(_, reasons)| reasons)
            .find_map(|r| r.strip_prefix("Change USE: "))
        else {
            continue;
        };
        let consumer = row
            .consumer_cpv
            .split(':')
            .next()
            .unwrap_or(&row.consumer_cpv)
            .to_string();
        out.push(HeldBackUse {
            category: row.category.clone(),
            package: row.package.clone(),
            slot: row.slot.clone(),
            installed_version: kept
                .get(&(row.category.clone(), row.package.clone()))
                .cloned()
                .unwrap_or_default(),
            skipped_version: row.skipped_version.clone(),
            atom: row.atom.clone(),
            consumer,
            changes: changes.split_whitespace().map(String::from).collect(),
        });
    }
    out.dedup();
    out
}
