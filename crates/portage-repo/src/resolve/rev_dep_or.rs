//! Which `||` arms of an installed consumer become parent atoms.

use crate::*;

/// What the scan knows about `||` evaluation in one pass.
pub(crate) struct OrScan<'a> {
    /// Portage entered complete mode in this pass.
    pub(crate) portage_complete: bool,
    /// The installed packages the walk recursed into, with its picks.
    pub(crate) installed_walked: &'a InstalledWalked,
    pub(crate) update: bool,
    /// What `_want_installed_pkg` reads: `selective` (already without `--emptytree`),
    /// whether backtracking is allowed, `--exclude` and the argument atoms.
    pub(crate) selective: bool,
    pub(crate) backtrack_allowed: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) arg_atoms: &'a [String],
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) post_merge: PostMergeSet,
}

/// An installed consumer's dependency strings, one per dependency key.
pub(crate) struct ConsumerDeps {
    pub(crate) use_flags: HashSet<String>,
    pub(crate) strings: Vec<(String, String)>,
}

/// Where an installed consumer's dependency strings come from.
#[derive(Clone, Copy)]
pub(crate) struct DepSource<'a> {
    pub(crate) root: &'a Path,
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) dynamic_deps: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
}

/// The per-pass memo of `Effective`-layer dependency strings.
pub(crate) type MetaMemo = HashMap<(String, String, String, String), String>;

/// An installed consumer's *effective* dependency strings: live metadata when it
/// is still in the tree, the raw vdb record otherwise. Empty strings are left out.
pub(crate) fn consumer_deps(
    src: DepSource<'_>,
    meta_memo: &mut MetaMemo,
    category: &str,
    package: &str,
    version: &str,
) -> ConsumerDeps {
    let DepSource {
        root,
        gate,
        dep_keys,
        dynamic_deps,
        ignore_built_slot_operator_deps,
    } = src;
    let use_flags = read_vdb_flag_set(root, category, package, version, "USE");
    let live = live_metadata_for_installed(gate.repos, root, category, package, version);
    let layer = if dynamic_deps {
        InstalledMetaLayer::Effective
    } else {
        InstalledMetaLayer::Raw
    };
    let mut strings = Vec::new();
    for key in dep_keys {
        let depstr = installed_dep_string(InstalledDepStringArgs {
            root,
            dynamic_deps,
            ignore_built_slot_operator_deps,
            category,
            package,
            version,
            live: live.as_deref(),
            key,
            layer,
            memo: meta_memo,
        });
        if !depstr.trim().is_empty() {
            strings.push(((*key).to_string(), depstr));
        }
    }
    ConsumerDeps { use_flags, strings }
}

/// The values [`consumer_or_arms`] reads from its caller.
pub(crate) struct ConsumerOrArmsArgs<'a> {
    pub(crate) scan: &'a OrScan<'a>,
    pub(crate) consumer: &'a InstalledPackage,
    pub(crate) split: &'a OrSplit,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
}

/// The arms of one consumer's `||` groups that `dep_zapdeps` selects in this pass.
/// `None` when the consumer's `||` is not evaluated: it was neither walked nor
/// reached by a complete graph.
// Portage: depgraph.py, _add_pkg_deps; dep_check.py, dep_zapdeps
pub(crate) fn consumer_or_arms(args: ConsumerOrArmsArgs<'_>) -> Option<HashSet<String>> {
    let ConsumerOrArmsArgs {
        scan,
        consumer,
        split,
        use_flags,
        entries,
        root,
        config,
        gate,
    } = args;
    let key = (
        consumer.category.clone(),
        consumer.package.clone(),
        consumer.version.clone(),
    );
    if let Some(picks) = scan.installed_walked.get(&key) {
        return Some(picks.clone());
    }
    if !scan.portage_complete {
        return None;
    }
    Some(installed_consumer_or_selection(
        InstalledConsumerOrSelectionArgs {
            split,
            use_flags,
            consumer: &(consumer.category.clone(), consumer.package.clone()),
            config,
            root,
            entries,
            update: scan.update,
            root_deps_running_root: scan.root_deps_running_root,
            gate,
            view: OrView::PostMerge(&scan.post_merge),
        },
    ))
}

/// The values [`consumer_or_demotions`] reads from its caller.
pub(crate) struct ConsumerOrDemotionsArgs<'a> {
    pub(crate) scan: &'a OrScan<'a>,
    pub(crate) consumer: &'a InstalledPackage,
    pub(crate) depstr: &'a str,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) leaked: Option<&'a HashSet<String>>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
}

/// The `||` arm texts of `depstr` that are no parent atoms of the consumer: the
/// arms `dep_zapdeps` did not select, every arm when the `||` is not evaluated,
/// and a selected arm of a complete-graph consumer that no installed package matches
/// (Portage files it as initially unsatisfied, never as a parent atom). An arm that
/// a sibling virtual's choice leaked stays.
// Portage: depgraph.py, _complete_graph (initially_unsatisfied); dep_check.py, _expand_new_virtuals
pub(crate) fn consumer_or_demotions(args: ConsumerOrDemotionsArgs<'_>) -> HashSet<String> {
    let ConsumerOrDemotionsArgs {
        scan,
        consumer,
        depstr,
        use_flags,
        leaked,
        entries,
        root,
        config,
        gate,
    } = args;
    if !depstr.contains("||") {
        return HashSet::new();
    }
    let Some(split) = split_or_groups(depstr, use_flags) else {
        return HashSet::new();
    };
    if split.is_flat() {
        return HashSet::new();
    }
    let selected = consumer_or_arms(ConsumerOrArmsArgs {
        scan,
        consumer,
        split: &split,
        use_flags,
        entries,
        root,
        config,
        gate,
    });
    let empty = HashSet::new();
    let mut demoted = split.arms_outside(selected.as_ref().unwrap_or(&empty));
    if let Some(leaked) = leaked {
        demoted.retain(|a| !leaked.contains(a));
    }
    let walked = scan.installed_walked.contains_key(&(
        consumer.category.clone(),
        consumer.package.clone(),
        consumer.version.clone(),
    ));
    if !walked && let Some(selected) = &selected {
        let arms = split.arms_outside(&HashSet::new());
        for atom in selected.iter().chain(leaked.into_iter().flatten()) {
            let blocker = parse_dep_atom(atom, "rev_dep_or")
                .is_none_or(|a| a.blocker != portage_dep::Blocker::None);
            if arms.contains(atom) && !blocker && !atom_matches_installed(root, atom, config) {
                demoted.insert(atom.clone());
            }
        }
    }
    demoted
}

/// The values [`sibling_virtual_leaks`] reads from its caller.
pub(crate) struct SiblingVirtualLeaksArgs<'a> {
    pub(crate) scan: &'a OrScan<'a>,
    pub(crate) deps: DepSource<'a>,
    pub(crate) meta_memo: &'a mut MetaMemo,
    pub(crate) in_scope: &'a dyn Fn(&InstalledPackage) -> bool,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
}

/// Quirk Q19: Portage compares a virtual's child atoms with the atoms chosen anywhere
/// under the same parent by identity, and interning makes equal atoms one object. So
/// a virtual keeps, as parent atoms, every atom of its own dependency string that
/// equals an atom chosen by its parent or by a sibling virtual. Returns, per
/// installed virtual, those atom texts.
// Portage: depgraph.py, _select_atoms_highest_available (chosen_atom_ids)
pub(crate) fn sibling_virtual_leaks(
    args: SiblingVirtualLeaksArgs<'_>,
) -> HashMap<(String, String, String), HashSet<String>> {
    let SiblingVirtualLeaksArgs {
        scan,
        deps,
        meta_memo,
        in_scope,
        entries,
        root,
        config,
        gate,
    } = args;
    let mut leaks: HashMap<(String, String, String), HashSet<String>> = HashMap::new();
    for parent in all_installed_packages(root).iter() {
        let parent_key = (
            parent.category.clone(),
            parent.package.clone(),
            parent.version.clone(),
        );
        // A parent is evaluated when the walk recursed into it, or a complete graph reaches it.
        if !scan.installed_walked.contains_key(&parent_key)
            && !(scan.portage_complete && in_scope(parent))
        {
            continue;
        }
        let parent_deps = consumer_deps(
            deps,
            meta_memo,
            &parent.category,
            &parent.package,
            &parent.version,
        );
        leak_one_parent(
            &LeakCtx {
                scan,
                deps,
                entries,
                root,
                config,
                gate,
            },
            meta_memo,
            &parent_deps,
            &|split: &OrSplit| {
                consumer_or_arms(ConsumerOrArmsArgs {
                    scan,
                    consumer: parent,
                    split,
                    use_flags: &parent_deps.use_flags,
                    entries,
                    root,
                    config,
                    gate,
                })
                .unwrap_or_default()
            },
            &mut leaks,
        );
    }
    // A merge-bound parent is a graph node the walk expanded: its `||` is chosen in
    // the normal view, its dependency strings are the tree's.
    for entry in entries {
        let Some(version) = merge_bound_version(&entry.outcome) else {
            continue;
        };
        let Some(meta) =
            live_metadata_for_installed(gate.repos, root, &entry.category, &entry.package, version)
        else {
            continue;
        };
        let use_flags: HashSet<String> = entry
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let parent_deps = ConsumerDeps {
            use_flags,
            strings: deps
                .dep_keys
                .iter()
                .filter_map(|k| {
                    meta.get(*k)
                        .filter(|v| !v.trim().is_empty())
                        .map(|v| ((*k).to_string(), v.clone()))
                })
                .collect(),
        };
        let cp = (entry.category.clone(), entry.package.clone());
        leak_one_parent(
            &LeakCtx {
                scan,
                deps,
                entries,
                root,
                config,
                gate,
            },
            meta_memo,
            &parent_deps,
            &|split: &OrSplit| {
                installed_consumer_or_selection(InstalledConsumerOrSelectionArgs {
                    split,
                    use_flags: &parent_deps.use_flags,
                    consumer: &cp,
                    config,
                    root,
                    entries,
                    update: scan.update,
                    root_deps_running_root: scan.root_deps_running_root,
                    gate,
                    view: OrView::Normal,
                })
            },
            &mut leaks,
        );
    }
    leaks
}

/// What [`leak_one_parent`] reads besides the parent.
struct LeakCtx<'a> {
    scan: &'a OrScan<'a>,
    deps: DepSource<'a>,
    entries: &'a [GraphEntry],
    root: &'a Path,
    config: &'a portage_profile::Config,
    gate: CategoryGate<'a>,
}

/// One parent's dep_check, one dependency key at a time: `parent_picks` gives the atoms
/// its own `||` groups choose.
fn leak_one_parent(
    cx: &LeakCtx<'_>,
    meta_memo: &mut MetaMemo,
    parent_deps: &ConsumerDeps,
    parent_picks: &dyn Fn(&OrSplit) -> HashSet<String>,
    leaks: &mut HashMap<(String, String, String), HashSet<String>>,
) {
    let LeakCtx {
        scan,
        deps,
        entries,
        root,
        config,
        gate,
    } = *cx;
    for (_key, depstr) in &parent_deps.strings {
        if !depstr.contains("virtual/") {
            continue;
        }
        let Some(split) = split_or_groups(depstr, &parent_deps.use_flags) else {
            continue;
        };
        let mut chosen: HashSet<String> = split.plain_atoms().clone();
        chosen.extend(parent_picks(&split));
        let virtuals: Vec<String> = chosen
            .iter()
            .filter(|a| {
                parse_dep_atom(a, "rev_dep_or").is_some_and(|p| {
                    p.category == "virtual" && p.blocker == portage_dep::Blocker::None
                })
            })
            .cloned()
            .collect();
        if virtuals.is_empty() {
            continue;
        }
        // (virtual instance, its atoms) for this dep key.
        let mut members: Vec<((String, String, String), HashSet<String>)> = Vec::new();
        for atom in &virtuals {
            let Some(version) = best_installed_matching(root, atom, config) else {
                continue;
            };
            let Some(parsed) = parse_dep_atom(atom, "rev_dep_or") else {
                continue;
            };
            let virt = InstalledPackage {
                category: parsed.category.clone(),
                package: parsed.package.clone(),
                version: version.clone(),
                slot: String::new(),
            };
            let virt_deps = consumer_deps(
                deps,
                meta_memo,
                &virt.category,
                &virt.package,
                &virt.version,
            );
            // GLEP 37: a virtual contributes its RDEPEND whatever key the parent
            // names it in (DC:217-220).
            let Some((_, virt_str)) = virt_deps.strings.iter().find(|(k, _)| k == "RDEPEND") else {
                continue;
            };
            let Some(virt_split) = split_or_groups(virt_str, &virt_deps.use_flags) else {
                continue;
            };
            let Some(all_atoms) = flat_dep_atoms(virt_str, &virt_deps.use_flags) else {
                continue;
            };
            // A virtual nothing evaluates chooses no atom.
            if let Some(picks) = consumer_or_arms(ConsumerOrArmsArgs {
                scan,
                consumer: &virt,
                split: &virt_split,
                use_flags: &virt_deps.use_flags,
                entries,
                root,
                config,
                gate,
            }) {
                chosen.extend(virt_split.plain_atoms().iter().cloned());
                chosen.extend(picks);
                members.push(((virt.category, virt.package, virt.version), all_atoms));
            }
        }
        for (virt_key, atoms) in members {
            let leaked: HashSet<String> =
                atoms.into_iter().filter(|a| chosen.contains(a)).collect();
            if !leaked.is_empty() {
                leaks.entry(virt_key).or_default().extend(leaked);
            }
        }
    }
}

impl OrScan<'_> {
    /// Portage's `_want_installed_pkg` for the installed instance `version` of `cat/pkg`.
    // Portage: depgraph.py, _want_installed_pkg
    pub(crate) fn want_installed_pkg(
        &self,
        root: &Path,
        category: &str,
        package: &str,
        version: &str,
    ) -> bool {
        let cpv = format!("{category}/{package}-{version}");
        if self
            .excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &cpv))
        {
            return true;
        }
        if self.selective {
            return true;
        }
        let Some(held) = installed_refs(root, category, package)
            .into_iter()
            .find(|r| r.version == version)
        else {
            return true;
        };
        let candidate = format!(
            // match-str-ok: installed ref row, not a Candidate
            "{cpv}:{}/{}::{}",
            held.slot, held.sub_slot, held.repo
        );
        !self.arg_atoms.iter().any(|a| {
            portage_dep::match_from_list(a, &[candidate.as_str()]).is_some_and(|m| !m.is_empty())
        })
    }
}

/// Portage's choice bins for one new-style virtual `||` group, narrowed to the
/// expansion question: an arm is available when every leg is tree-visible or
/// installed-matched; the downgrade guard demotes an arm whose best candidate
/// the graph already holds newer (bug #531656). Of the survivors, an installed
/// arm wins outright (it stays a parent atom/pin -- the Q19 shape); otherwise
/// the first survivor in ebuild order is the fall-through arm to seed.
/// dep_zapdeps' fuller bin ladder (USE bins, upgrade promotion, multi-version
/// choices) is deliberately not reproduced: an arm needing USE changes or a
/// multi-version virtual stays unseeded (a later slice's fixture decides).
/// With `keep_demoted` the guard never fires and installed arms stay in the
/// running: the first available arm returns even when demoted, the shape of
/// Portage's *inner* `dep_zapdeps` return over an expanded virtual's own `||`
/// (the outer choice demotes the conjunction as a unit instead).
/// A demotion whose higher-in-slot node the pass's open slot conflict may yet
/// skip commits no seed: the installed arm holds instead, and the feed
/// re-commits once the verdict lands with the demoter surviving (#650).
/// Portage: dep_check.py, dep_zapdeps.
fn select_expansion_arms(args: SelectExpansionArmsArgs<'_>) -> ExpansionArms {
    let SelectExpansionArmsArgs {
        split,
        consumer,
        config,
        root,
        entries,
        update,
        gate,
        keep_demoted,
        conflicts,
    } = args;
    let no_constraints: HashMap<(String, String), Vec<String>> = HashMap::new();
    let self_node: RootCp = (false, consumer.0.clone(), consumer.1.clone());
    let mut survivors: Vec<Vec<String>> = Vec::new();
    let mut installed_survivor = false;
    let mut demoted: Vec<(Vec<String>, Vec<DowngradeDemoter>)> = Vec::new();
    for (tokens, _) in split.groups() {
        let Some(arms) = group_arms(tokens) else {
            continue;
        };
        for arm in arms {
            if arm.iter().any(|a| {
                parse_dep_atom(a, "virtual_expansion_arm")
                    .is_none_or(|at| at.blocker != portage_dep::Blocker::None)
            }) {
                continue;
            }
            if !arm.iter().all(|a| {
                atom_matches_installed(root, a, config)
                    || atom_currently_satisfiable(gate, a, config, &[])
            }) {
                continue;
            }
            // The inner choice over an expanded virtual keeps the
            // demoted-but-available arm: the outer choice demotes the whole
            // conjunction instead, so the guard never filters here.
            // Portage: dep_check.py, dep_zapdeps (inner return).
            if !keep_demoted {
                let demoters = downgrade_demoters(AlternativeDowngradeDemotedArgs {
                    root,
                    config,
                    entries,
                    self_cp: &self_node,
                    constraints: &no_constraints,
                    atoms: &arm,
                    queued: &[],
                    update,
                    gate,
                    view: OrView::Normal,
                });
                if !demoters.is_empty() {
                    // Portage files this demotion transiently
                    // (dep_check.py:531-544, filed :634-635) but re-resolves
                    // post-skip: the installed arm bins `preferred_in_graph`
                    // (:636-655) and the fall-through `preferred_non_installed`
                    // (:599-616). A demoter the pass's open slot conflict may
                    // yet skip carries no settled verdict, so the demotion is
                    // withheld here; the installed arm holds instead.
                    if let Some(hit) = demoters.iter().find(|d| {
                        conflicts.iter().any(|sc| {
                            sc.category == d.category
                                && sc.package == d.package
                                && sc.slot == d.slot
                        })
                    }) {
                        if seed_trace_enabled() {
                            eprintln!(
                                "SEED decline virtual={}/{} arm={:?} demoter={:?} contested",
                                consumer.0, consumer.1, arm, hit
                            );
                        }
                    } else {
                        demoted.push((arm.clone(), demoters));
                        continue;
                    }
                }
            }
            if !keep_demoted && arm.iter().all(|a| atom_matches_installed(root, a, config)) {
                installed_survivor = true;
            } else {
                survivors.push(arm);
            }
        }
    }
    if installed_survivor || survivors.is_empty() {
        return ExpansionArms {
            chosen: Vec::new(),
            demoted,
        };
    }
    // First survivor in ebuild order; each leg is re-checked by the caller.
    ExpansionArms {
        chosen: survivors.remove(0),
        demoted,
    }
}

/// One arm per top-level alternative of a `["||", "(", ..., ")"]` token group:
/// a bare atom or the flattened atoms of a `( ... )` conjunction. `None` for a
/// nested disjunction (stays a documented cut).
fn group_arms(group: &[String]) -> Option<Vec<Vec<String>>> {
    let (_, rest) = group.split_first()?;
    let (open, rest) = rest.split_first()?;
    if *open != "(" || rest.last().map(String::as_str) != Some(")") {
        return None;
    }
    let inner = &rest[..rest.len() - 1];
    let mut arms = Vec::new();
    let mut j = 0;
    while j < inner.len() {
        if inner[j] == "(" {
            let mut depth = 0;
            let mut k = j;
            loop {
                if inner[k] == "(" {
                    depth += 1;
                } else if inner[k] == ")" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if inner[k] == "||" {
                    return None;
                }
                k += 1;
                if k >= inner.len() {
                    return None;
                }
            }
            let mut leg = Vec::new();
            for t in &inner[j + 1..k] {
                if t == "(" || t == ")" {
                    continue;
                }
                if t == "||" {
                    return None;
                }
                leg.push(t.clone());
            }
            if leg.is_empty() {
                return None;
            }
            arms.push(leg);
            j = k + 1;
        } else if inner[j] == "||" || inner[j] == ")" {
            return None;
        } else {
            arms.push(vec![inner[j].clone()]);
            j += 1;
        }
    }
    if arms.is_empty() {
        return None;
    }
    Some(arms)
}

/// The values [`select_expansion_arms`] reads from its caller.
struct SelectExpansionArmsArgs<'a> {
    split: &'a OrSplit,
    consumer: &'a (String, String),
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    update: bool,
    gate: CategoryGate<'a>,
    /// The pass's surviving slot conflicts: a demotion whose higher-in-slot
    /// node sits in one is withheld (the demoter may yet be skipped, #650).
    /// The inner-choice caller passes an empty slice alongside
    /// `keep_demoted` (the guard is dead there either way).
    conflicts: &'a [SlotConflict],
    /// Inner choice over an expanded virtual: keep the demoted-but-available
    /// arm and installed arms instead of filtering them (the p02 seed feed
    /// passes `false`).
    keep_demoted: bool,
}

/// What [`select_expansion_arms`] chose: the fall-through arm to seed (empty
/// when an installed arm holds the binding), plus every guard-demoted arm
/// with the higher-in-slot nodes that demoted it (the seed's demotion
/// record, for the seed-decision trace).
struct ExpansionArms {
    chosen: Vec<String>,
    demoted: Vec<(Vec<String>, Vec<DowngradeDemoter>)>,
}

/// Whether the seed-decision trace is active (`PORTUALE_BT_TRACE=1`).
pub(crate) fn seed_trace_enabled() -> bool {
    std::env::var("PORTUALE_BT_TRACE").is_ok_and(|value| value == "1")
}

/// Whether a new-style virtual's RDEPEND qualifies for expansion: a `||`
/// shape whose plain atoms are all blockers or `:=` slot-operator atoms. A
/// bare or pinned plain atom beside the group suppresses Portage's expansion
/// (S0 `probe572_mixed.py`); the valid-syntax mixed variants show
/// shape-dependent behavior (`probe572_mixed_exact.py`), so the gate skips
/// all mixed virtuals rather than guessing which plain form suppresses. The
/// conjunction extras of the real perl virtuals (`dev-lang/perl:=`, `!<`,
/// `!>`) ride the same expansion instead: Portage matches `:=` like a plain
/// slot and skips blockers in its choice bins, and the M3 playground falls
/// through with exactly this shape.
/// Portage: dep_check.py, dep_zapdeps (blocker skip, `_match_slot`).
pub(crate) fn virtual_rdepend_is_expandable(split: &OrSplit) -> bool {
    if split.is_flat() {
        return false;
    }
    split.plain_atoms().iter().all(|a| {
        parse_dep_atom(a, "virtual_rdepend").is_some_and(|at| {
            at.blocker != portage_dep::Blocker::None
                || at.slot_operator == Some(portage_dep::SlotOperator::Equals)
        })
    })
}

/// What [`virtual_expansion_seeds`] derived this pass: committing seeds plus
/// the recorded-hold delta (added holds whose posture filed this pass,
/// dropped holds whose fresh inputs flipped the verdict).
pub(crate) struct ExpansionFeed {
    pub(crate) seeds: Vec<ExpansionSeed>,
    pub(crate) holds_added: Vec<PostureHold>,
    pub(crate) holds_dropped: Vec<PostureHold>,
}

/// New-style virtual `||` arms that need a fresh merge in this pass's graph:
/// [`ExpansionSeed`]s owned by the virtual, each carrying the veto-level
/// guard-demoters it was chosen under (the post-skip re-check input).
/// Portage inlines the virtual's RDEPEND into the consumer (`dep_check.py`,
/// `_expand_new_virtuals`), so the fall-through arm resolves as the virtual's
/// own dep. Only arms no installed package satisfies become seeds: an
/// installed-satisfied arm stays a parent atom/pin (the Q19 shape), and
/// queueing it as a hard dep would invert the backtracking preference.
/// Only the installed version's RDEPEND seeds, however many tree versions
/// exist: a seed is a merge commitment with no demotion vehicle, so a
/// non-installed version's live arm would merge where Portage's `=<cpv>` pin
/// demotes its whole conjunction. Portage ground (`dep_check.py:197,220,264`):
/// every matching tree virtual contributes its own RDEPEND (`:220`) with an
/// unconditional `=<cpv>` pin (`:197`, `:264`), so a bare arm merged as the
/// virtual's own dep comes only from dep_check on the kept instance's own
/// string. Already-seeded and already-merge-bound arms are skipped, so the
/// feed terminates.
/// Backlog #656 leg (b): the verdict carries the re-evaluating parents'
/// posture. A request-pinned demoter forces every judgment post-demoter, so
/// no keeper binds; with only replacing parents the installed arm holds (the
/// hold is recorded in `BacktrackParams::posture_holds` and re-fires while the
/// posture reads the same), while a fresh-leg parent pulls the fall-through.
/// With a walk-selected demoter only a requested keeper's pre-demoter
/// judgment binds.
// Portage: dep_check.py, _expand_new_virtuals; depgraph.py, _add_dep (:3364)
pub(crate) fn virtual_expansion_seeds(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    grown: &BacktrackParams,
) -> ExpansionFeed {
    // No seeds when the walk follows no dependencies (`--nodeps`), and none
    // under `--onlydeps` either: Portage expands virtuals while walking merge
    // candidates, and the playground oracle merges nothing on this shape under
    // `--onlydeps` (a build-dep-pulled virtual that needs its fall-through
    // arm merged stays a documented cut).
    if ctx.nodeps || ctx.onlydeps {
        return ExpansionFeed {
            seeds: Vec::new(),
            holds_added: Vec::new(),
            holds_dropped: Vec::new(),
        };
    }
    let gate = ctx.gate();
    let mut out = ExpansionFeed {
        seeds: Vec::new(),
        holds_added: Vec::new(),
        holds_dropped: Vec::new(),
    };
    for e in &pass.entries {
        if e.category != "virtual" {
            continue;
        }
        let PretendOutcome::AlreadyInstalled { version } = &e.outcome else {
            continue;
        };
        if e.required_by.is_empty() {
            continue;
        }
        // The installed version's tree ebuild drives the feed; sibling tree
        // versions never seed (see the fn docstring). Single-version graphs
        // take the same element as before, byte-identical.
        let ebuilds: Vec<_> = gate
            .repo_candidates(&e.category, &e.package)
            .map(|cs| {
                cs.iter()
                    .filter(|c| c.source == CandidateSource::Ebuild)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let Some(cand) = ebuilds.iter().find(|c| c.version == *version) else {
            continue;
        };
        let Some(live) =
            live_metadata_for_installed(gate.repos, ctx.root, &e.category, &e.package, version)
        else {
            continue;
        };
        let rdepend = live.get("RDEPEND").map(String::as_str).unwrap_or("");
        // Portage evaluates the virtual's USE conditionals with the virtual's
        // own effective USE, like the walk would for the tree candidate.
        let cand_str = cand.match_str(&e.category, &e.package);
        let use_flags = effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
            config: ctx.config,
            iuse: live.get("IUSE").map(String::as_str).unwrap_or_default(),
            keywords: &cand.keywords,
            candidate_str: &cand_str,
            build_id: cand.build_id.as_deref(),
            category: &e.category,
            package: &e.package,
        });
        // Pure-`||` virtuals only: a plain atom beside the group suppresses
        // Portage's expansion (S0 `probe572_mixed.py`).
        let Some(split) = split_or_groups(rdepend, &use_flags) else {
            continue;
        };
        if !virtual_rdepend_is_expandable(&split) {
            continue;
        }
        let arms = select_expansion_arms(SelectExpansionArmsArgs {
            split: &split,
            consumer: &(e.category.clone(), e.package.clone()),
            config: ctx.config,
            root: ctx.root,
            entries: &pass.entries,
            update: ctx.update,
            gate,
            keep_demoted: false,
            conflicts: &pass.slot_conflicts,
        });
        for atom in arms.chosen {
            let Some(a) = parse_dep_atom(&atom, "virtual_expansion") else {
                continue;
            };
            if a.blocker != portage_dep::Blocker::None {
                continue;
            }
            // Installed-satisfied arms stay parent atoms/pins.
            if atom_matches_installed(ctx.root, &atom, ctx.config) {
                continue;
            }
            if !atom_currently_satisfiable(gate, &atom, ctx.config, &[]) {
                continue;
            }
            // The walk already merged it (deep/emptytree reached the arm itself).
            if pass.entries.iter().any(|o| {
                o.category == a.category
                    && o.package == a.package
                    && merge_bound_version(&o.outcome).is_some()
            }) {
                continue;
            }
            // `via` names the veto-level demoters only: demoted arms the
            // installed binding would have held through (had one survived,
            // `installed_survivor` would have vetoed every seed). A demoted
            // non-installed arm merely yields its survivor slot, so its
            // demoter is not this seed's own.
            let via: Vec<&DowngradeDemoter> = arms
                .demoted
                .iter()
                .filter(|(arm, _)| {
                    arm.iter()
                        .all(|a| atom_matches_installed(ctx.root, a, ctx.config))
                })
                .flat_map(|(_, ds)| ds.iter())
                .collect();
            let seed = ExpansionSeed {
                owner: (e.category.clone(), e.package.clone(), version.clone()),
                atom,
                // The post-skip re-check input: the seed survives only while
                // one of these higher-in-slot nodes survives.
                demoters: via
                    .iter()
                    .map(|d| (d.category.clone(), d.package.clone(), d.version.clone()))
                    .collect(),
            };
            // Latched by owner+atom (first commit wins): a re-derived seed
            // with different demoters is the same commitment.
            if grown
                .virtual_expansion_seeds
                .iter()
                .any(|s| s.owner == seed.owner && s.atom == seed.atom)
            {
                continue;
            }
            // A recorded hold is a re-checkable verdict, not a latch: the
            // posture below re-derives every pass, and the re-fire line says
            // whether the record still agrees. A hold never latches a seed,
            // so a record and a latch for one owner+atom are mutually
            // exclusive by construction.
            let recorded = grown
                .posture_holds
                .iter()
                .any(|h| h.owner == seed.owner && h.atom == seed.atom);
            // A walk-selected demoter may postdate an independent puller's
            // pre-demoter judgment, which Portage keeps: the virtual is
            // already bound without the demoter, so its fall-through holds.
            // Only a predating (requested) keeper binds -- a walk-selected
            // keeper's own judgment is post-demoter by construction, so it
            // never keeps and the fresh leg falls through (backlog #656 leg
            // (b2): the Encode shape). A request-pinned demoter is forced
            // from walk start instead, so every judgment is post-demoter and
            // the seed fires (M6b/M5).
            // Portage: depgraph.py, _create_graph (top-level atoms resolve
            // first); dep_check.py, dep_zapdeps (posture-split first evals).
            if !seed.demoters.is_empty()
                && seed
                    .demoters
                    .iter()
                    .all(|d| !demoter_request_pinned(ctx, &d.0, &d.1, &d.2))
                && let Some((kcat, kpkg)) = independent_keeper(&pass.entries, e, &seed.demoters)
            {
                if !keeper_requested(ctx, &kcat, &kpkg) {
                    if seed_trace_enabled() {
                        eprintln!(
                            "SEED keeper-unrequested virtual={}/{} seed={} keeper={kcat}/{kpkg} demoters={:?} post-demoter",
                            e.category, e.package, seed.atom, seed.demoters
                        );
                    }
                } else {
                    if seed_trace_enabled() {
                        eprintln!(
                            "SEED hold virtual={}/{} seed={} keeper={kcat}/{kpkg} demoters={:?} walk-selected",
                            e.category, e.package, seed.atom, seed.demoters
                        );
                    }
                    continue;
                }
            }
            // A request-pinned demoter is forced from walk start, so no keeper
            // binds; the replacing parent's posture then decides: with only
            // replacing re-evaluating parents the installed arm holds
            // (backlog #656 leg (b1): the TH shape), while a fresh-leg parent
            // pulls the fall-through (the Encode shape).
            // Portage: dep_check.py:480/:531 (`if not replacing` bypass),
            // :490/:541 (downgrade flags); depgraph.py, _will_replace_child.
            if !seed.demoters.is_empty()
                && seed
                    .demoters
                    .iter()
                    .any(|d| demoter_request_pinned(ctx, &d.0, &d.1, &d.2))
                && replacing_posture_holds(&pass.entries, e, &seed.demoters)
            {
                if recorded {
                    if seed_trace_enabled() {
                        eprintln!(
                            "SEED hold virtual={}/{} seed={} re-fire=agree recorded",
                            e.category, e.package, seed.atom
                        );
                    }
                } else {
                    if seed_trace_enabled() {
                        eprintln!(
                            "SEED hold virtual={}/{} seed={} demoters={:?} replacing recorded",
                            e.category, e.package, seed.atom, seed.demoters
                        );
                    }
                    out.holds_added.push(PostureHold {
                        owner: seed.owner.clone(),
                        atom: seed.atom.clone(),
                    });
                }
                continue;
            }
            // Fresh inputs overrule the stale record: the hold drops and the
            // seed commits (Portage re-evaluates every consumer's `||`).
            if recorded {
                if seed_trace_enabled() {
                    eprintln!(
                        "SEED hold virtual={}/{} seed={} re-fire=flip committing",
                        e.category, e.package, seed.atom
                    );
                }
                out.holds_dropped.push(PostureHold {
                    owner: seed.owner.clone(),
                    atom: seed.atom.clone(),
                });
            }
            if seed_trace_enabled() {
                eprintln!(
                    "SEED commit virtual={}/{} seed={} via={:?}",
                    e.category, e.package, seed.atom, via
                );
            }
            out.seeds.push(seed);
        }
    }
    out
}

/// The values [`expand_new_style_virtual_arm`] reads from its caller.
pub(crate) struct ExpandNewStyleVirtualArmArgs<'a> {
    pub(crate) atom: &'a str,
    pub(crate) consumer: &'a (String, String),
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) gate: CategoryGate<'a>,
    /// The walk updates: a newer visible tree version upgrades the virtual
    /// instead of falling through (M4/M5b oracle), so multi-version
    /// expansion stays off and the bare arm resolves to the upgrade.
    /// Portage expands either way: its update-awareness lives in the upgrade
    /// selection (`depgraph.py:7332,7356 _want_update_pkg`, `--update` in
    /// myopts) and the choice bins (`dep_check.py:574-598` `want_update` on
    /// each choice, `:783-798` upgrade promotion, `:812-818` two-pass
    /// selection), and the oracle records the upgrade outcome
    /// (`probe647_oracle.py:339` M4-bare-Ud, `:364` M5b-bare-Ud, header
    /// `:26-32`). portuale's tie-break is same-bin only, so it suppresses
    /// the expansion and reaches the same observable outcome through its
    /// own upgrade path instead (the two `-uD` pins prove it).
    pub(crate) update: bool,
}

/// One consumer `||` arm naming an installed new-style virtual becomes one
/// conjunction per matching tree version, each `[that version's RDEPEND...,
/// =virtual/<that version>]`, spliced as sibling arms of the consumer's
/// group: Portage's `_expand_new_virtuals` disjunction-context shape
/// (`newsplit.extend(a)`). The RDEPEND rides verbatim (its `||` intact), so
/// the existing ranker demotes versions with dead members exactly like
/// Portage's `all_available` pass, and the version pin demotes the rest
/// through the existing downgrade guard. A nested `||` stays a documented
/// cut (#644): it never resolves, so its arm is unselectable, never chosen.
/// Single-version virtuals keep the resolved shape (the nested choice plus
/// the exact-version pin); the conjunction extras ride either shape.
/// `None` when the arm names no expandable virtual, so the caller keeps the
/// arm byte-identical.
// Portage: dep_check.py, _expand_new_virtuals.
pub(crate) fn expand_new_style_virtual_arm(
    args: ExpandNewStyleVirtualArmArgs<'_>,
) -> Option<Vec<Vec<String>>> {
    let ExpandNewStyleVirtualArmArgs {
        atom,
        consumer,
        config,
        root,
        gate,
        update,
    } = args;
    let parsed = parse_dep_atom(atom, "virtual_arm_expansion")?;
    if parsed.blocker != portage_dep::Blocker::None || parsed.category != "virtual" {
        return None;
    }
    let version = best_installed_matching(root, atom, config)?;
    let ebuilds: Vec<_> = gate
        .repo_candidates(&parsed.category, &parsed.package)
        .map(|cs| {
            cs.iter()
                .filter(|c| c.source == CandidateSource::Ebuild)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    // The installed version must still have a tree ebuild to evaluate.
    // Corner (a) of #648: installed version with no tree ebuild but other
    // tree versions present stays off (bare arm), same as the pre-#647 cut.
    if !ebuilds.iter().any(|c| c.version == version) {
        return None;
    }
    if update
        && ebuilds.iter().any(|c| {
            vercmp_ordering(&c.version, &version) == Ordering::Greater
                && is_visible(c, &parsed.category, &parsed.package, config)
        })
    {
        return None;
    }
    if ebuilds.len() == 1 {
        return expand_single_version_virtual_arm(ExpandSingleVersionVirtualArmArgs {
            parsed: &parsed,
            version: &version,
            cand: &ebuilds[0],
            consumer,
            config,
            root,
            gate,
        })
        .map(|conjunction| vec![conjunction]);
    }
    // Portage's descending-version order (`matches.reverse()`).
    let mut ebuilds_sorted = ebuilds;
    ebuilds_sorted.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));
    let mut out = Vec::new();
    for cand in &ebuilds_sorted {
        let Some(live) = live_metadata_for_installed(
            gate.repos,
            root,
            &parsed.category,
            &parsed.package,
            &cand.version,
        ) else {
            continue;
        };
        let rdepend = live.get("RDEPEND").map(String::as_str).unwrap_or("");
        // Portage evaluates the virtual's USE conditionals with the virtual's
        // own effective USE, like the walk would for the tree candidate.
        let cand_str = cand.match_str(&parsed.category, &parsed.package);
        let use_flags = effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
            config,
            iuse: live.get("IUSE").map(String::as_str).unwrap_or_default(),
            keywords: &cand.keywords,
            candidate_str: &cand_str,
            build_id: cand.build_id.as_deref(),
            category: &parsed.category,
            package: &parsed.package,
        });
        let Some(split) = split_or_groups(rdepend, &use_flags) else {
            continue;
        };
        // Corner (c) of #648: flat-RDEPEND versions (e.g. `1.680.0`, no
        // `||`) contribute no conjunction on the multi path either — the
        // flat veto is shared with singles — while Portage evaluates their
        // flat members plus the `=<cpv>` pin as a conjunction.
        if !virtual_rdepend_is_expandable(&split) {
            continue;
        }
        // Conditional virtual RDEPENDs stay cut: the verbatim tokens below
        // would evaluate against the consumer's USE at pop time.
        let members: Vec<String> = rdepend.split_whitespace().map(str::to_string).collect();
        if members.iter().any(|t| t.ends_with('?')) {
            continue;
        }
        let mut conjunction = members;
        conjunction.push(format!(
            "={}/{}-{}",
            parsed.category, parsed.package, cand.version
        ));
        out.push(conjunction);
    }
    if out.is_empty() { None } else { Some(out) }
}

/// The selectable pool for pinning judgments: visible tree match strings
/// plus installed `cat/pkg-ver:slot/sub` partials (no `::repo`), ignoring
/// backtrack masks: pinning is a request/pull-form property, not a pass
/// verdict.
pub(crate) fn selectable_pool(
    gate: CategoryGate<'_>,
    config: &portage_profile::Config,
    root: &Path,
    category: &str,
    package: &str,
) -> Vec<String> {
    let mut pool: Vec<String> = gate
        .repo_candidates(category, package)
        .map(|cs| {
            cs.iter()
                .filter(|c| c.source == CandidateSource::Ebuild)
                .filter(|c| is_visible(c, category, package, config))
                .map(|c| c.match_str(category, package))
                .collect()
        })
        .unwrap_or_default();
    for (ver, slot, sub) in installed_candidates(root, category, package) {
        // match-str-ok: partial form (no ::repo) from the vdb record's fields.
        pool.push(format!("{category}/{package}-{ver}:{slot}/{sub}"));
    }
    pool
}

/// Whether `atom` matches exactly `version` of the package over the
/// selectable pool and nothing else: a version-forcing requirement.
fn atom_matches_only_version(
    atom: &str,
    pool: &[String],
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let refs: Vec<&str> = pool.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom, &refs) else {
        return false;
    };
    matched.len() == 1
        && candidate_version(matched[0], category, package).as_deref() == Some(version)
}

/// Whether a top-level request atom pins the seed's demoter version exactly:
/// the demoter is forced from walk start, so every `||` judgment is
/// post-demoter and the fall-through fires (M6b/M5: `=prov-5.44.0` pins the
/// transient). A bare-selected demoter instead lands mid-walk, possibly after
/// an independent puller's pre-demoter judgment, which Portage keeps (the
/// live TH hold). Judged over the selectable pool (visible tree plus
/// installed), ignoring masks: pinning is a request-form property, not a
/// pass verdict. Undecidable request text counts as pinning (fail closed,
/// same rationale as [`merge_version_forced`]).
// Portage: depgraph.py, _create_graph (top-level atoms resolve first).
fn demoter_request_pinned(
    ctx: &ResolveCtx<'_>,
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let pool = selectable_pool(ctx.gate(), ctx.config, ctx.root, category, package);
    ctx.atoms.iter().any(|a| {
        atom_matches_only_version(a, &pool, category, package, version) || !forcing_decidable(a)
    })
}

/// What [`merge_version_forced`] reads from its caller.
pub(crate) struct MergeVersionForcedArgs<'a> {
    pub(crate) arg_atoms: &'a [String],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
}

/// Whether the forcing question is decidable for this requirement text: it
/// parses as a match atom. USE-conditional (`flag? ( ... )`) and other
/// multi-token spellings never parse as one atom, so their verdict is
/// unknown — and an unknown verdict must enforce (fail closed = pre-#652
/// unconditional enforcement), never skip.
/// Portage: depgraph.py, _slot_operator_check_reverse_dependencies (parent
/// atoms constrain without re-proving their spelling).
pub(crate) fn forcing_decidable(atom: &str) -> bool {
    portage_dep::parse_match_atom(atom).is_some()
}

/// Whether the merge version is forced by a versioned requirement: a
/// top-level request atom matching only it, or a merge-bound entry's dep
/// atom matching only it. A forced demoter makes the conflict hard (Q19:
/// xpull-2's `=5.44*` forces the transient the virtuals' arms block, so the
/// skip stands); a walk-selected one leaves the kept binding soft (the bed
/// TH hold, where no versioned puller wants the transient).
// Portage: depgraph.py, _create_graph (top-level atoms resolve first).
pub(crate) fn merge_version_forced(args: MergeVersionForcedArgs<'_>) -> bool {
    let MergeVersionForcedArgs {
        arg_atoms,
        entries,
        gate,
        config,
        root,
        category,
        package,
        version,
    } = args;
    let pool = selectable_pool(gate, config, root, category, package);
    if arg_atoms
        .iter()
        .any(|a| atom_matches_only_version(a, &pool, category, package, version))
    {
        return true;
    }
    // Fail closed: a requirement text the matcher cannot decide
    // (USE-conditional or other multi-token spelling) could pin the version;
    // the pre-#652 behavior enforced unconditionally, so uncertainty enforces
    // too. Decidable non-singletons keep the new verdicts (bare atoms,
    // multi-version pins, wrong-version pins).
    // Portage: depgraph.py, _slot_operator_check_reverse_dependencies (parent
    // atoms constrain without re-proving their spelling).
    if arg_atoms.iter().any(|a| !forcing_decidable(a)) {
        return true;
    }
    entries.iter().any(|e| {
        e.category != "virtual"
            && merge_bound_version(&e.outcome).is_some()
            && e.deps.iter().any(|d| {
                atom_matches_only_version(&d.atom, &pool, category, package, version)
                    || !forcing_decidable(&d.atom)
            })
    })
}

/// The merge-bound puller outside the seed's demoter cps that keeps the
/// virtual: its binding survives the demoter, so Portage's pre-demoter
/// self-parent judgment sticks and the fall-through never fires (the live TH
/// hold: the early consumer binds TH before the transient lands). `None`
/// when every puller is the demoter itself (the transient-dependent Encode
/// posture, which falls through).
fn independent_keeper(
    entries: &[GraphEntry],
    virtual_entry: &GraphEntry,
    demoters: &BTreeSet<(String, String, String)>,
) -> Option<(String, String)> {
    let demoter_cps: HashSet<(&str, &str)> = demoters
        .iter()
        .map(|(c, p, _)| (c.as_str(), p.as_str()))
        .collect();
    virtual_entry.required_by.iter().find_map(|(cat, pkg)| {
        if demoter_cps.contains(&(cat.as_str(), pkg.as_str())) {
            return None;
        }
        entries
            .iter()
            .any(|o| {
                o.category == *cat && o.package == *pkg && merge_bound_version(&o.outcome).is_some()
            })
            .then(|| (cat.clone(), pkg.clone()))
    })
}

/// Whether the keeper's judgment predates the demoter: only a requested
/// puller's pre-demoter binding sticks (H-form: requested-first `p652early`
/// keeps TH). A walk-selected keeper is post-demoter by construction and
/// never keeps, so the fresh leg falls through instead (backlog #656 leg
/// (b2): the real-cell Archive-Zip posture). Undecidable request text counts
/// as requesting (fail closed = the pre-#656 behavior binds).
// Portage: depgraph.py, _create_graph (top-level atoms resolve first).
fn keeper_requested(ctx: &ResolveCtx<'_>, category: &str, package: &str) -> bool {
    ctx.atoms.iter().any(|a| {
        parse_dep_atom(a, "keeper_requested")
            .is_none_or(|at| at.category == category && at.package == package)
    })
}

/// Whether every merge-bound re-evaluating parent of the virtual replaces the
/// demoted installed arm: all in-graph requirers share a demoter cp (the
/// replacing transient's posture keeps the installed arm), with at least one
/// present. A fresh-leg parent outside the demoter cps pulls the
/// fall-through instead, so the seed commits (the Encode shape, M6b/M5).
/// Installed-only requirers never re-evaluate: Portage judges each
/// consumer's `||` when the consumer itself is processed.
// Portage: dep_check.py:480/:531 (`if not replacing` bypass), :490/:541
// (downgrade flags); depgraph.py, _will_replace_child (same cp+slot: the
// guard demotes within one slot, so the cp test decides it here).
fn replacing_posture_holds(
    entries: &[GraphEntry],
    virtual_entry: &GraphEntry,
    demoters: &BTreeSet<(String, String, String)>,
) -> bool {
    let demoter_cps: HashSet<(&str, &str)> = demoters
        .iter()
        .map(|(c, p, _)| (c.as_str(), p.as_str()))
        .collect();
    let mut replacing = false;
    for (cat, pkg) in &virtual_entry.required_by {
        let merge_bound = entries.iter().any(|o| {
            o.category == *cat && o.package == *pkg && merge_bound_version(&o.outcome).is_some()
        });
        if !merge_bound {
            continue;
        }
        if !demoter_cps.contains(&(cat.as_str(), pkg.as_str())) {
            return false;
        }
        replacing = true;
    }
    replacing
}

/// The values the single-version arm expansion reads from its caller.
struct ExpandSingleVersionVirtualArmArgs<'a> {
    parsed: &'a portage_dep::Atom,
    version: &'a str,
    cand: &'a Candidate,
    consumer: &'a (String, String),
    config: &'a portage_profile::Config,
    root: &'a Path,
    gate: CategoryGate<'a>,
}

/// The pre-#647 shape, kept for single-tree-version virtuals: the nested
/// choice is Portage's inner `dep_zapdeps` return, which keeps the
/// demoted-but-available arm; the outer choice then demotes the whole
/// conjunction through the existing `||` ranking. The conjunction extras
/// ride the conjunction; pure-`||` singles emit exactly the old tokens.
// Portage: dep_check.py, dep_zapdeps (inner return).
fn expand_single_version_virtual_arm(
    args: ExpandSingleVersionVirtualArmArgs<'_>,
) -> Option<Vec<String>> {
    let ExpandSingleVersionVirtualArmArgs {
        parsed,
        version,
        cand,
        consumer,
        config,
        root,
        gate,
    } = args;
    let live =
        live_metadata_for_installed(gate.repos, root, &parsed.category, &parsed.package, version)?;
    let rdepend = live.get("RDEPEND").map(String::as_str).unwrap_or("");
    // The virtual's USE conditionals read its own effective USE, like the seed feed.
    let cand_str = cand.match_str(&parsed.category, &parsed.package);
    let use_flags = effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
        config,
        iuse: live.get("IUSE").map(String::as_str).unwrap_or_default(),
        keywords: &cand.keywords,
        candidate_str: &cand_str,
        build_id: cand.build_id.as_deref(),
        category: &parsed.category,
        package: &parsed.package,
    });
    let split = split_or_groups(rdepend, &use_flags)?;
    if !virtual_rdepend_is_expandable(&split) {
        return None;
    }
    // The nested choice is computed here at enqueue/token-rewrite time, not at
    // pop time (REPORT §4.4): safe because `keep_demoted` skips the downgrade
    // guard, so the graph inputs are dead by construction, and arm
    // availability is tree/vdb-static. Tripwire: if `keep_demoted` ever
    // reads the graph, this call must move to pop time or take the drained
    // graph instead.
    // Portage: dep_check.py, dep_zapdeps (inner return).
    let mut nested = select_expansion_arms(SelectExpansionArmsArgs {
        split: &split,
        consumer,
        config,
        root,
        entries: &[],
        update: false,
        gate,
        keep_demoted: true,
        conflicts: &[],
    });
    if nested.chosen.is_empty() {
        return None;
    }
    // Deterministic order: the plain set has no ebuild order.
    let mut extras: Vec<String> = split.plain_atoms().iter().cloned().collect();
    extras.sort();
    nested.chosen.extend(extras);
    nested
        .chosen
        .push(format!("={}/{}-{version}", parsed.category, parsed.package));
    Some(nested.chosen)
}

/// The values [`expand_consumer_virtual_arms`] reads from its caller.
pub(crate) struct ExpandConsumerVirtualArmsArgs<'a> {
    pub(crate) tokens: &'a [String],
    pub(crate) consumer: &'a (String, String),
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) update: bool,
}

/// Rewrite a merge-bound consumer's dep-string tokens before `||` deferral:
/// every bare arm of a top-level `||` group that names an installed
/// new-style virtual expands to its per-version conjunctions (see
/// [`expand_new_style_virtual_arm`]), spliced as sibling arms of the
/// consumer's group. Arms inside a nested `||` stay verbatim (that shape is
/// a documented cut); everything else is copied unchanged, so graphs without
/// a nested-virtual expansion come out byte-identical.
// Portage: dep_check.py, _expand_new_virtuals.
pub(crate) fn expand_consumer_virtual_arms(args: ExpandConsumerVirtualArmsArgs<'_>) -> Vec<String> {
    let ExpandConsumerVirtualArmsArgs {
        tokens,
        consumer,
        config,
        root,
        gate,
        update,
    } = args;
    let ex = Expander {
        consumer,
        config,
        root,
        gate,
        update,
    };
    let mut out = Vec::with_capacity(tokens.len());
    expand_seq_into(tokens, false, &ex, &mut out);
    out
}

/// The per-arm expansion inputs [`expand_seq_into`] threads through the token walk.
struct Expander<'a> {
    consumer: &'a (String, String),
    config: &'a portage_profile::Config,
    root: &'a Path,
    gate: CategoryGate<'a>,
    update: bool,
}

/// One token-sequence pass: `arm_position` marks the direct arms of an `||`
/// group body, the only positions Portage expands. `||`, plain groups and
/// `flag?` conditionals recurse (the deferral split evaluates conditionals
/// afterwards); a nested `||` copies verbatim with its cut shape.
fn expand_seq_into(seq: &[String], arm_position: bool, ex: &Expander<'_>, out: &mut Vec<String>) {
    let mut i = 0;
    while i < seq.len() {
        let tok = &seq[i];
        if tok == "||"
            && seq.get(i + 1).map(String::as_str) == Some("(")
            && let Some(close) = find_close(seq, i + 1)
        {
            if arm_position {
                out.extend(seq[i..=close].iter().cloned());
            } else {
                out.push("||".to_string());
                out.push("(".to_string());
                expand_seq_into(&seq[i + 2..close], true, ex, out);
                out.push(")".to_string());
            }
            i = close + 1;
        } else if tok == "("
            && let Some(close) = find_close(seq, i)
        {
            out.push("(".to_string());
            expand_seq_into(&seq[i + 1..close], false, ex, out);
            out.push(")".to_string());
            i = close + 1;
        } else if tok.ends_with('?')
            && seq.get(i + 1).map(String::as_str) == Some("(")
            && let Some(close) = find_close(seq, i + 1)
        {
            out.push(tok.clone());
            out.push("(".to_string());
            expand_seq_into(&seq[i + 2..close], false, ex, out);
            out.push(")".to_string());
            i = close + 1;
        } else if arm_position && let Some(conjunctions) = ex.expand_arm(tok) {
            for conjunction in conjunctions {
                out.push("(".to_string());
                out.extend(conjunction);
                out.push(")".to_string());
            }
            i += 1;
        } else {
            out.push(tok.clone());
            i += 1;
        }
    }
}

/// The balanced `)` closing the `(` at `open`: `None` when unbalanced. The
/// caller copies the input verbatim on `None`, so the downstream flattener
/// errors exactly as it would without the rewrite.
fn find_close(seq: &[String], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (j, tok) in seq.iter().enumerate().skip(open) {
        if tok == "(" {
            depth += 1;
        } else if tok == ")" {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

impl Expander<'_> {
    /// The conjunctions for one bare `||` arm, or `None` to keep the arm.
    fn expand_arm(&self, tok: &str) -> Option<Vec<Vec<String>>> {
        expand_new_style_virtual_arm(ExpandNewStyleVirtualArmArgs {
            atom: tok,
            consumer: self.consumer,
            config: self.config,
            root: self.root,
            gate: self.gate,
            update: self.update,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn split_of(depstr: &str) -> Option<OrSplit> {
        split_or_groups(depstr, &HashSet::from(["amd64".to_string()]))
    }

    /// Backlog #572 review focus: a mixed virtual is not expanded. The S0
    /// shape (bare plain atom beside the group, `probe572_mixed.py`) is
    /// suppressed by Portage; the gate pins that shape plus its valid-syntax
    /// cousins, since `probe572_mixed_exact.py` shows a same-cp plain pin
    /// expands-with-failure and a different-cp plain pin suppresses-with-failure.
    #[test]
    fn mixed_virtual_rdepend_is_not_expandable() {
        for depstr in [
            // S0's exact shape: bare plain atom beside the group.
            "dev-perl/Other-1.0 || ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 )",
            // Valid-syntax cousins: same-cp pin and different-cp pin beside the group.
            "=dev-lang/perl-5.42* || ( =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 )",
            "=dev-perl/Other-1.0 || ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 )",
            // No group at all.
            "=dev-lang/perl-5.42*",
        ] {
            let split =
                split_of(depstr).unwrap_or_else(|| panic!("fixture shape must parse: {depstr}"));
            assert!(
                !virtual_rdepend_is_expandable(&split),
                "mixed or group-less RDEPEND must not expand: {depstr}"
            );
        }
    }

    #[test]
    fn pure_or_virtual_rdepend_is_expandable() {
        for depstr in [
            "|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 )",
            "|| ( =dev-libs/p572prov-5.42* ~dev-libs/p572enc-3.210 )",
        ] {
            let split =
                split_of(depstr).unwrap_or_else(|| panic!("fixture shape must parse: {depstr}"));
            assert!(
                virtual_rdepend_is_expandable(&split),
                "pure || RDEPEND must expand: {depstr}"
            );
        }
    }

    /// Backlog #647 second lift: the real perl virtuals' conjunction extras
    /// ride the expansion. Portage matches `:=` like a plain slot and skips
    /// blockers in its choice bins, and the M3 playground falls through with
    /// exactly this shape — while the mixed-disjunction vetoes above stay.
    #[test]
    fn conjunction_extras_virtual_rdepend_is_expandable() {
        for depstr in [
            // M3 shape: verbatim real 1.680.100_rc RDEPEND.
            "|| ( =dev-lang/perl-5.42* ~perl-core/Scalar-List-Utils-1.680.100_rc ) dev-lang/perl:= !<perl-core/Scalar-List-Utils-1.680.100_rc !>perl-core/Scalar-List-Utils-1.680.100_rc-r999",
            // Leg-B shape: verbatim real 3.210.0-r2 RDEPEND.
            "|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 ) dev-lang/perl:= !<perl-core/Encode-3.210.0 !>perl-core/Encode-3.210.0-r999",
        ] {
            let split =
                split_of(depstr).unwrap_or_else(|| panic!("fixture shape must parse: {depstr}"));
            assert!(
                virtual_rdepend_is_expandable(&split),
                "conjunction-extras RDEPEND must expand: {depstr}"
            );
        }
    }

    /// Backlog #652 R5: a USE-conditional spelling of a version-forcing
    /// requirement is undecidable for the singleton matcher, so it must
    /// count as forcing (fail closed = pre-#652 unconditional enforcement),
    /// never as walk-selected. RED before the R5 fix (fail-open returned
    /// false → the gate would skip enforcement); GREEN after.
    #[test]
    fn use_conditional_spelling_counts_as_forced() {
        let tmp = portage_util::TempDir::new("portage-repo-652r5");
        let gate = crate::test_gate(&[]);
        let config = portage_profile::Config::default();
        fn args<'a>(
            atoms: &'a [String],
            gate: CategoryGate<'a>,
            config: &'a portage_profile::Config,
            root: &'a Path,
        ) -> MergeVersionForcedArgs<'a> {
            MergeVersionForcedArgs {
                arg_atoms: atoms,
                entries: &[],
                gate,
                config,
                root,
                category: "dev-libs",
                package: "p651prov",
                version: "5.44.0",
            }
        }
        let plain = ["=dev-libs/p651prov-5.44.0".to_string()];
        let cond = ["use? ( =dev-libs/p651prov-5.44.0 )".to_string()];
        // Undecidable: USE-conditional spelling of a version pin. The pool is
        // empty here, so even the plain pin decides false (nothing selectable
        // matches) — isolating exactly the R5 hole: undecidable must force.
        assert!(
            !forcing_decidable("use? ( =dev-libs/p651prov-5.44.0 )"),
            "USE-conditional text never parses as one match atom"
        );
        assert!(forcing_decidable("=dev-libs/p651prov-5.44.0"));
        assert!(forcing_decidable("dev-libs/p651prov"));
        assert!(
            !merge_version_forced(args(&plain, gate, &config, tmp.path())),
            "empty pool: plain pin decides false (control)"
        );
        assert!(
            merge_version_forced(args(&cond, gate, &config, tmp.path())),
            "undecidable requirement spelling must enforce (fail closed)"
        );
    }
}
