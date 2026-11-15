//! Charter 2a bootstrap: a real build-time cycle whose one USE-conditional
//! arm drops under a single flag, serialized as boot (reduced USE,
//! temporary) -> middle -> definitive (declared USE). The selection rule is
//! `portage_use_reduce::bootstrap_reduction`; this module feeds it the
//! resolver's cycles, synthesizes the boot row and rewrites the sorted vec.

use crate::*;
use portage_use_reduce::{
    BootstrapUse, MatchMode, boot_closure_reaches_member, bootstrap_reduction_strict_first,
    conditional_guards, use_reduce_flat,
};
use std::collections::BTreeMap;

/// `DepEdge::key` ordinals (`RDEPEND`, `IDEPEND`, `PDEPEND`, `DEPEND`,
/// `BDEPEND`) to `DEP_KEYS`-layout bits (`DEPEND`, `RDEPEND`, `PDEPEND`,
/// `BDEPEND`, `IDEPEND`), the layout both cycle reporters read.
fn key_bit(key: u8) -> u8 {
    match key {
        3 => 1 << 0,
        0 => 1 << 1,
        2 => 1 << 2,
        4 => 1 << 3,
        1 => 1 << 4,
        _ => 0,
    }
}

/// The `*DEPEND` variables a build-time edge can come from.
const BUILD_KEYS: [&str; 3] = ["DEPEND", "BDEPEND", "IDEPEND"];

/// The `(category, package)` of one raw dependency token, or `None` for
/// operators, group markers and blockers (a blocker never pulls a merge).
fn token_cp(token: &str) -> Option<(String, String)> {
    let atom = crate::parse_dep_atom(token, "bootstrap_guards")?;
    if atom.blocker != portage_dep::Blocker::None {
        return None;
    }
    Some((atom.category, atom.package))
}

/// Whether removing `flag` drops every build-time arm of `metadata` naming
/// `dep_cp`: every raw atom with that `cp` sits under the flag
/// (universally quantified, so an unguarded `||` alternative keeps the edge
/// and the STOP-2 re-check below refuses instead of dropping a live need).
/// A `[flag?]` USE-conditional atom counts as unguarded: the single-flag
/// boot shape cannot express it, so such an edge is never reduced. One
/// shared predicate for the guard map and the `deps` filter, so the two
/// cannot disagree.
fn edge_drops_without_flag(
    metadata: &HashMap<String, String>,
    dep_cp: &(String, String),
    flag: &str,
) -> bool {
    let mut seen = false;
    for key in BUILD_KEYS {
        let Some(text) = metadata.get(key) else {
            continue;
        };
        for (atom, guards) in conditional_guards(text) {
            if token_cp(&atom).as_ref() == Some(dep_cp) {
                seen = true;
                if !guards.contains(flag) {
                    return false;
                }
            }
        }
    }
    seen
}

/// The USE flags guarding each directed edge into a build-time dep: the
/// flags whose removal drops every build-time arm naming the dep's `cp`
/// (see [`edge_drops_without_flag`]). Keyed exactly like the leaf's
/// `edges`/`kinds` maps (merge-bound `cat/pkg-ver` on both ends) via the
/// same cp lookup.
fn guard_map(
    entries: &[GraphEntry],
    cp_index: &HashMap<(&str, &str), usize>,
) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut out: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for owner in entries {
        let Some(cpv) = merge_bound_cpv(owner) else {
            continue;
        };
        let Some(input) = owner.dep_input.as_deref() else {
            continue;
        };
        let mut candidates: BTreeSet<String> = BTreeSet::new();
        for key in BUILD_KEYS {
            if let Some(text) = input.metadata.get(key) {
                for (_, guards) in conditional_guards(text) {
                    candidates.extend(guards);
                }
            }
        }
        let mut targets: BTreeSet<(String, String)> = BTreeSet::new();
        for dep in &owner.deps {
            if dep.priority.buildtime {
                targets.insert((dep.category.clone(), dep.package.clone()));
            }
        }
        for target in targets {
            let Some(&di) = cp_index.get(&(target.0.as_str(), target.1.as_str())) else {
                continue;
            };
            let Some(dep_cpv) = merge_bound_cpv(&entries[di]) else {
                continue;
            };
            let dropping: BTreeSet<String> = candidates
                .iter()
                .filter(|f| edge_drops_without_flag(&input.metadata, &target, f))
                .cloned()
                .collect();
            if !dropping.is_empty() {
                out.insert((cpv.clone(), dep_cpv), dropping);
            }
        }
    }
    out
}

/// Whether removing `flag` switches on a build-time arm: a `cp` the
/// build-time dependency strings of `metadata` name under the declared USE
/// minus `flag` but not under the declared USE (a `!flag?` arm).
pub(crate) fn flag_adds_build_dep(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    flag: &str,
) -> bool {
    let cps = |use_flags: &HashSet<String>| -> BTreeSet<(String, String)> {
        BUILD_KEYS
            .iter()
            .filter_map(|key| metadata.get(*key))
            .filter_map(|text| {
                let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
                use_reduce_flat(&tokens, use_flags, MatchMode::Normal).ok()
            })
            .flatten()
            .filter_map(|token| token_cp(&token))
            .collect()
    };
    let mut reduced = use_flags.clone();
    reduced.remove(flag);
    !cps(&reduced).is_subset(&cps(use_flags))
}

/// Adjacency over merge-bound entries plus the kinds behind each edge, in
/// the leaf's node namespace (merge-bound `cat/pkg-ver`). A dep with no
/// merge-bound instance ends the DFS there: an already-installed package
/// is not rebuilt by the boot, so it cannot carry a build-time need for a
/// member.
struct GraphView<'a> {
    cpv_index: BTreeMap<String, usize>,
    cp_index: HashMap<(&'a str, &'a str), usize>,
    edges: BTreeMap<String, BTreeSet<String>>,
    kinds: BTreeMap<(String, String), u8>,
}

fn graph_view(entries: &[GraphEntry]) -> GraphView<'_> {
    let cp_index = merge_bound_index(entries);
    let mut cpv_index: BTreeMap<String, usize> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        if let Some(cpv) = merge_bound_cpv(e) {
            cpv_index.entry(cpv).or_insert(i);
        }
    }
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut kinds: BTreeMap<(String, String), u8> = BTreeMap::new();
    for owner in entries {
        let Some(owner_cpv) = merge_bound_cpv(owner) else {
            continue;
        };
        for dep in &owner.deps {
            let Some(&di) = cp_index.get(&(dep.category.as_str(), dep.package.as_str())) else {
                continue;
            };
            let Some(dep_cpv) = merge_bound_cpv(&entries[di]) else {
                continue;
            };
            edges
                .entry(owner_cpv.clone())
                .or_default()
                .insert(dep_cpv.clone());
            *kinds.entry((owner_cpv.clone(), dep_cpv)).or_default() |= key_bit(dep.key);
        }
    }
    GraphView {
        cpv_index,
        cp_index,
        edges,
        kinds,
    }
}

/// The `installed`/`forced` inputs the expand displays mirror from the
/// definitive entry's own computation: the installed version the outcome
/// diffs against, and the force/mask flags of the same candidate. Reads
/// the same sources as the walk's display pass, with the metadata in hand
/// instead of the candidate.
fn boot_display_inputs(
    definitive: &GraphEntry,
    root: &Path,
    config: &portage_profile::Config,
) -> (Option<InstalledUseState>, HashSet<String>) {
    let Some((_, installed_version)) = display_versions(
        &definitive.outcome,
        root,
        &definitive.category,
        &definitive.package,
    ) else {
        return (None, HashSet::new());
    };
    let installed = installed_version.map(|v| {
        let old_iuse =
            read_vdb_flag_set(root, &definitive.category, &definitive.package, &v, "IUSE");
        let mut old_use =
            read_vdb_flag_set(root, &definitive.category, &definitive.package, &v, "USE");
        old_use.retain(|f| old_iuse.contains(f));
        InstalledUseState { old_use, old_iuse }
    });
    (installed, definitive_forced_flags(definitive, config))
}

/// The force/mask flags of the definitive entry's candidate.
fn definitive_forced_flags(
    definitive: &GraphEntry,
    config: &portage_profile::Config,
) -> HashSet<String> {
    let input = definitive.dep_input.as_deref();
    let iuse = input
        .and_then(|i| i.metadata.get("IUSE").map(String::as_str))
        .unwrap_or("");
    let keywords: Vec<String> = input
        .and_then(|i| i.metadata.get("KEYWORDS"))
        .map(|k| k.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let slot = definitive.slot.as_deref().unwrap_or("0");
    let sub_slot = definitive.sub_slot.as_deref().unwrap_or("0");
    let repo = definitive.repo_name.as_deref().unwrap_or("");
    let version = match &definitive.outcome {
        PretendOutcome::New { version } => Some(version.clone()),
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
            Some(to.clone())
        }
        PretendOutcome::Reinstall { version, .. } => Some(version.to_string()),
        _ => None,
    };
    let candidate_str = version.map_or_else(String::new, |v| {
        format!(
            // match-str-ok: the merge-list entry's own Option fields, not a candidate.
            "{}/{category}-{v}:{slot}/{sub_slot}::{repo}",
            definitive.category,
            category = definitive.package,
        )
    });
    forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
        iuse,
        keywords: &keywords,
        candidate_str: &candidate_str,
        build_id: definitive.build_id.as_deref(),
        category: &definitive.category,
        package: &definitive.package,
        config,
    })
}

/// Whether `REQUIRED_USE` still holds with `flag` removed from `use_flags`
/// (an unreadable `REQUIRED_USE` counts as holding).
fn required_use_holds_without(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    flag: &str,
) -> bool {
    let Some(required_use) = metadata.get("REQUIRED_USE") else {
        return true;
    };
    let iuse: HashSet<String> = metadata
        .get("IUSE")
        .map(|i| {
            i.split_whitespace()
                .map(|t| t.trim_start_matches(['+', '-']).to_string())
                .collect()
        })
        .unwrap_or_default();
    let mut reduced = use_flags.clone();
    reduced.remove(flag);
    portage_required_use::check_required_use(required_use, &reduced, &iuse).unwrap_or(true)
}

/// Whether the boot build may force `flag` off: the flag is on, the profile
/// does not force it, removing it switches on no build-time arm, and (when
/// `keep_required_use`) `REQUIRED_USE` still holds. Shared by the resolver
/// and the fast plan so both pick the same bootstrap.
pub fn bootstrap_flag_eligible(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    forced: &HashSet<String>,
    flag: &str,
    keep_required_use: bool,
) -> bool {
    use_flags.contains(flag)
        && !forced.contains(flag)
        && !flag_adds_build_dep(metadata, use_flags, flag)
        && (!keep_required_use || required_use_holds_without(metadata, use_flags, flag))
}

/// The boot row for the definitive entry with `flag` forced off: the
/// entry cloned variant-for-variant (the marker, not the outcome,
/// distinguishes the rows), USE displays recomputed with the flag off,
/// `deps` the reduced closure (the build-time `DepEdge`s to cycle members
/// whose arm the flag guards are gone), `dep_input` re-pointed at the
/// reduced evaluation. `blockers` ride along as filed under the declared
/// USE (no fixture conditions a blocker on the reduced flag; re-evaluating
/// them under boot USE is open work, not a known divergence).
#[allow(clippy::too_many_arguments)]
fn boot_row(
    definitive: &GraphEntry,
    member_cps: &BTreeSet<(String, String)>,
    flag: &str,
    declared_use: Vec<String>,
    root: &Path,
    config: &portage_profile::Config,
) -> GraphEntry {
    let mut boot = definitive.clone();
    let mut display = definitive.use_flags_display.clone();
    if let Some(slot) = display.iter_mut().find(|(f, _)| f == flag) {
        slot.1 = false;
    } else {
        display.push((flag.to_string(), false));
        display.sort_by_key(|p| alnum_sort_key(&p.0));
    }
    boot.use_flags_display = display;
    let (installed, forced) = boot_display_inputs(definitive, root, config);
    let reinst: HashSet<String> = match &definitive.outcome {
        PretendOutcome::Reinstall { changed_flags, .. } => changed_flags.iter().cloned().collect(),
        _ => HashSet::new(),
    };
    boot.use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &boot.use_flags_display,
        config,
        installed: installed.as_ref(),
        forced: &forced,
        all_flags: true,
        reinst_flags: &reinst,
    });
    boot.use_expand_display_p = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &boot.use_flags_display,
        config,
        installed: installed.as_ref(),
        forced: &forced,
        all_flags: false,
        reinst_flags: &reinst,
    });
    if let Some(input) = definitive.dep_input.as_deref() {
        boot.deps = definitive
            .deps
            .iter()
            .filter(|d| {
                !(d.priority.buildtime
                    && member_cps.contains(&(d.category.clone(), d.package.clone()))
                    && edge_drops_without_flag(
                        &input.metadata,
                        &(d.category.clone(), d.package.clone()),
                        flag,
                    ))
            })
            .cloned()
            .collect();
        let mut reduced_use: HashSet<String> = input.use_flags.clone();
        reduced_use.remove(flag);
        boot.dep_input = Some(std::sync::Arc::new(DepInput {
            metadata: input.metadata.clone(),
            use_flags: reduced_use,
        }));
    }
    boot.bootstrap = Some(BootstrapUse {
        reduced_flag: flag.to_string(),
        declared_use,
    });
    boot
}

/// Try the charter 2a bootstrap over this pass's hard cycles: synthesize
/// the boot row, rewrite the sorted entries, and soften the dropped hard
/// map edges so the cycle report reads the reduced edge set. Returns
/// `true` when the caller must re-run the cycle report; the pass keeps
/// today's shape otherwise (STOP rules 1–3: no single-flag reduction, a
/// boot that still needs a member, or more than 3 affected rows).
pub(crate) fn maybe_bootstrap(
    ctx: &ResolveCtx<'_>,
    config: &portage_profile::Config,
    pass: &mut PassResult,
    circular_deps: &[Vec<String>],
) -> bool {
    // A bootstrap *is* merges: under `--buildpkgonly` the reduction is
    // `None` and the cycle keeps today's failure. Under `--onlydeps` the
    // same holds for a suppressed top-level target: it must not merge at
    // all, so it must not boot either (a boot row for it would print
    // nowhere and satisfy nothing).
    if ctx.buildpkgonly || circular_deps.is_empty() {
        return false;
    }
    let GraphView {
        cpv_index,
        cp_index,
        edges,
        kinds,
    } = graph_view(&pass.entries);
    let guards = guard_map(&pass.entries, &cp_index);
    // The boot build only removes a flag, never a forced one; it prefers a
    // flag whose removal keeps REQUIRED_USE, and refuses one whose removal
    // switches on a build-time arm it could not carry in order.
    let eligible = |node: &str, flag: &str, keep_required_use: bool| {
        let Some(entry) = cpv_index.get(node).map(|&i| &pass.entries[i]) else {
            return false;
        };
        entry.dep_input.as_deref().is_some_and(|input| {
            bootstrap_flag_eligible(
                &input.metadata,
                &input.use_flags,
                &definitive_forced_flags(entry, config),
                flag,
                keep_required_use,
            )
        })
    };
    let Some((boot_cpv, flag)) =
        bootstrap_reduction_strict_first(circular_deps, &edges, &kinds, &guards, &eligible)
    else {
        return false;
    };
    let members: BTreeSet<String> = circular_deps.iter().flatten().cloned().collect();
    if boot_closure_reaches_member(&edges, &kinds, &guards, &boot_cpv, &flag, &members) {
        return false;
    }
    if members.len() + 1 > 3 {
        return false;
    }
    let Some(&boot_idx) = cpv_index.get(&boot_cpv) else {
        return false;
    };
    let definitive = pass.entries[boot_idx].clone();
    // `--onlydeps` suppresses the top-level targets from the merge: a
    // boot row for one would print nowhere and satisfy nothing, while the
    // JSON plan would still carry it as a merge. Refuse, so the cycle
    // keeps today's failure. (A cycle member that is a plain dependency
    // still bootstraps: it merges normally.)
    if ctx.onlydeps
        && ctx
            .atoms
            .iter()
            // drop-ok: @sets are not dependency tokens; only plain atoms name the boot slot.
            .filter_map(|a| portage_dep::parse_atom(a))
            .any(|a| a.category == definitive.category && a.package == definitive.package)
    {
        return false;
    }
    let declared: HashSet<String> = definitive
        .dep_input
        .as_deref()
        .map(|input| input.use_flags.clone())
        .unwrap_or_default();
    let mut declared_use: Vec<String> = declared.iter().cloned().collect();
    declared_use.sort();
    let member_cps: BTreeSet<(String, String)> = members
        .iter()
        .filter_map(|m| {
            cpv_index.get(m).map(|&i| {
                (
                    pass.entries[i].category.clone(),
                    pass.entries[i].package.clone(),
                )
            })
        })
        .collect();
    let mut boot = boot_row(
        &definitive,
        &member_cps,
        &flag,
        declared_use,
        ctx.root,
        config,
    );
    // STOP rule 2, on the synthesized row itself: any remaining build-time
    // `DepEdge` from the boot slot to a cycle member refuses the bootstrap.
    if boot.deps.iter().any(|d| {
        d.priority.buildtime && member_cps.contains(&(d.category.clone(), d.package.clone()))
    }) {
        return false;
    }
    // Soften exactly the dropped walk edges: the hard-only map entries
    // from the boot slot's own root view to a cycle member. Hard-only
    // means no softer arm shares the key (a runtime arm would have set
    // it), and STOP-2 above already refused when any build-time `DepEdge`
    // to a member survives — so every such key is the dropped arm.
    // Other roots' same-cp keys are untouched.
    for (key, (hard, soft)) in pass.edge_kind_map.iter_mut() {
        if key.1.0 != definitive.targets_running_root
            || (key.1.1.clone(), key.1.2.clone())
                != (definitive.category.clone(), definitive.package.clone())
            || !member_cps.contains(&(key.0.1.clone(), key.0.2.clone()))
        {
            continue;
        }
        if *hard && !*soft {
            *hard = false;
        }
    }
    // Rewrite rule on the sorted vec: the boot row takes the earliest of
    // the boot slot's own position and its transitive dependents (it must
    // precede everything needing it), and the definitive entry moves to
    // immediately after the last entry that transitively build-depends on
    // the boot slot (reduced build graph).
    // Every entry that transitively build-depends on the boot slot: the
    // boot row precedes the earliest of them, the definitive entry follows
    // the latest.
    let merge_index = merge_bound_index(&pass.entries);
    let mut dependents: BTreeSet<usize> = BTreeSet::new();
    for (i, e) in pass.entries.iter().enumerate() {
        if i != boot_idx
            && merge_bound_cpv(e).is_some()
            && graph_view_reaches(&pass.entries, &merge_index, i, boot_idx)
        {
            dependents.insert(i);
        }
    }
    let mut rest: Vec<GraphEntry> = Vec::with_capacity(pass.entries.len() + 1);
    for (i, e) in pass.entries.drain(..).enumerate() {
        if i != boot_idx {
            rest.push(e);
        }
    }
    // Indices above are pre-removal; the boot slot sat at `boot_idx`, so
    // later positions shift down by one.
    let shift = |i: usize| if i > boot_idx { i - 1 } else { i };
    let dep_pos: BTreeSet<usize> = dependents.iter().map(|&i| shift(i)).collect();
    let boot_pos = dep_pos
        .iter()
        .copied()
        .min()
        .unwrap_or(shift(boot_idx))
        .min(shift(boot_idx));
    boot.discovery = boot_pos;
    rest.insert(boot_pos, boot);
    let last_dep = dep_pos
        .iter()
        .copied()
        .max()
        .map(|p| if p >= boot_pos { p + 1 } else { p });
    let definitive_pos = last_dep.map_or(rest.len(), |p| p + 1).min(rest.len());
    let mut definitive = definitive;
    definitive.discovery = definitive_pos;
    rest.insert(definitive_pos, definitive);
    pass.entries = rest;
    true
}

/// Whether entry `i` transitively build-depends on entry `j` over the
/// current `deps` (which, for the boot slot, is already the reduced
/// closure): the placement rule's own reachability, not the leaf's.
fn graph_view_reaches(
    entries: &[GraphEntry],
    index: &HashMap<(&str, &str), usize>,
    i: usize,
    j: usize,
) -> bool {
    let target = match merge_bound_cpv(&entries[j]) {
        Some(cpv) => cpv,
        None => return false,
    };
    let mut seen: BTreeSet<usize> = BTreeSet::from([i]);
    let mut stack = vec![i];
    while let Some(u) = stack.pop() {
        for d in &entries[u].deps {
            if !d.priority.buildtime {
                continue;
            }
            let Some(&di) = index.get(&(d.category.as_str(), d.package.as_str())) else {
                continue;
            };
            if di == j || merge_bound_cpv(&entries[di]).as_ref() == Some(&target) {
                return true;
            }
            if seen.insert(di) {
                stack.push(di);
            }
        }
    }
    false
}
