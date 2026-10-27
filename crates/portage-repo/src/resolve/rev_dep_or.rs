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
