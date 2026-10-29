//! The update probe for a merge-bound binary parent (`slot_operator_mask_built`).

use crate::*;

/// Arguments of [`built_parent_update_probe`].
pub(crate) struct BuiltParentUpdateProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) local_binpkg: &'a BinaryIndex,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    /// The pass's `runtime_pkg_mask`: a masked tree instance is no update.
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
    /// The installed dependency graph and the installed consumers the pass's
    /// complete graph can contain (`live_reachable_consumers`).
    pub(crate) installed: &'a InstalledReachability,
    pub(crate) live: &'a HashSet<(String, String)>,
    /// The installed packages the request's own walk reaches: their dependencies
    /// have edges in Portage's digraph, which exempts them from the check.
    pub(crate) walked: &'a HashSet<(String, String)>,
    pub(crate) gate: CategoryGate<'a>,
}

/// [`built_parent_update_probe`]'s result: the installed children joining the
/// replace set and the `(category, package, version)` binary parents to mask.
pub(crate) type BuiltParentUpdateProbe = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// Merge-bound binary parents whose built `:S=` dep is missing a slot ABI
/// update: the installed child in the bound slot has a tree instance with
/// another sub-slot that an ebuild replacement of the parent accepts. Each
/// returned binary `(category, package, version)` is masked so the restart
/// resolves the parent from the tree; the installed child joins the replace set,
/// as Portage's `_slot_operator_update_backtrack` seeds its `_replace_installed_atom`.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls, _slot_operator_update_probe, _slot_operator_update_backtrack
pub(crate) fn built_parent_update_probe(
    args: BuiltParentUpdateProbeArgs<'_>,
) -> BuiltParentUpdateProbe {
    let BuiltParentUpdateProbeArgs {
        root,
        config,
        local_binpkg,
        entries,
        with_bdeps,
        excluded,
        masked,
        installed,
        live,
        walked,
        gate,
    } = args;
    const DEP_KEYS: [&str; 5] = ["RDEPEND", "PDEPEND", "IDEPEND", "DEPEND", "BDEPEND"];
    let dep_keys: &[&str] = if with_bdeps {
        &DEP_KEYS
    } else {
        &DEP_KEYS[..3]
    };
    let mut masks = BTreeSet::new();
    let mut replace: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in entries {
        if entry.source != CandidateSource::Binary {
            continue;
        }
        let Some(bin_version) = merge_bound_version(&entry.outcome) else {
            continue;
        };
        let Some(bin_slot) = entry.slot.as_deref() else {
            continue;
        };
        let parent = (entry.category.clone(), entry.package.clone());
        let parent_cpv = format!("{}/{}-{bin_version}", parent.0, parent.1);
        if excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &parent_cpv))
        {
            continue;
        }
        let Some(bin_meta) = read_binary_metadata_any(ReadBinaryMetadataAnyArgs {
            config,
            root,
            local: local_binpkg,
            category: &parent.0,
            package: &parent.1,
            version: bin_version,
        }) else {
            continue;
        };
        let use_flags: HashSet<String> = entry
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let Ok(tree) = gate.repo_candidates(&parent.0, &parent.1) else {
            continue;
        };
        // The ebuild replacements in the binary's slot, not older than it.
        let replacements: Vec<&Candidate> = tree
            .iter()
            .filter(|c| {
                c.source == CandidateSource::Ebuild
                    && c.slot == bin_slot
                    && vercmp_ordering(&c.version, bin_version) != Ordering::Less
                    && is_visible(c, &parent.0, &parent.1, config)
            })
            .collect();
        if replacements.is_empty() {
            continue;
        }
        let bin_tokens: Vec<String> = dep_keys
            .iter()
            .filter_map(|key| {
                flat_dep_atoms(
                    bin_meta.get(*key).map(String::as_str).unwrap_or_default(),
                    &use_flags,
                )
            })
            .flatten()
            .collect();
        let mut hit: Option<(String, String)> = None;
        for token in bin_tokens {
            let Some(atom) = parse_dep_atom(&token, "built_parent_bin_tokens") else {
                continue;
            };
            if atom.blocker != portage_dep::Blocker::None || !is_built_slot_op(&atom) {
                continue;
            }
            let child = (atom.category.clone(), atom.package.clone());
            let bound_slot = atom.slot.clone().unwrap_or_default();
            if child_has_slot_update(ChildUpdateArgs {
                root,
                config,
                entries,
                parent: &parent,
                replacements: &replacements,
                use_flags: &use_flags,
                dep_keys,
                child: &child,
                bound_slot: &bound_slot,
                excluded,
                masked,
                installed,
                live,
                walked,
                gate,
                from_graph_child: false,
            }) {
                hit = Some(child.clone());
                break;
            }
        }
        if let Some(child) = hit {
            replace.insert(child);
            masks.insert((parent.0, parent.1, bin_version.clone()));
        }
    }
    (replace, masks)
}

/// Arguments of [`child_has_slot_update`].
pub(crate) struct ChildUpdateArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) parent: &'a (String, String),
    pub(crate) replacements: &'a [&'a Candidate],
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) child: &'a (String, String),
    pub(crate) bound_slot: &'a str,
    pub(crate) excluded: &'a [String],
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
    pub(crate) installed: &'a InstalledReachability,
    pub(crate) live: &'a HashSet<(String, String)>,
    pub(crate) walked: &'a HashSet<(String, String)>,
    pub(crate) gate: CategoryGate<'a>,
    /// Probe from the instance the graph merges for the child (Portage's
    /// `dep.child` of an installed parent's edge) instead of leaving a
    /// merge-bound child alone.
    pub(crate) from_graph_child: bool,
}

/// Whether the installed `child` in `bound_slot` has a tree instance with another
/// sub-slot that every atom on `child` in some replacement of `parent` accepts.
/// A child the graph already merges is left alone: the binary's dep then names
/// a different instance than the installed one -- unless `from_graph_child`
/// (an installed parent), where the merged instance is the probe's child.
// Portage: _emerge/depgraph.py, _slot_operator_update_probe (replacement_parent loop, candidate filter)
pub(crate) fn child_has_slot_update(args: ChildUpdateArgs<'_>) -> bool {
    let ChildUpdateArgs {
        root,
        config,
        entries,
        parent,
        replacements,
        use_flags,
        dep_keys,
        child,
        bound_slot,
        excluded,
        masked,
        installed,
        live,
        walked,
        gate,
        from_graph_child,
    } = args;
    let (child_cat, child_pkg) = (child.0.as_str(), child.1.as_str());
    let graph_child = entries.iter().find(|e| {
        e.category == child_cat
            && e.package == child_pkg
            && merge_bound_version(&e.outcome).is_some()
    });
    if graph_child.is_some() && !from_graph_child {
        return false;
    }
    // The installed instance in the bound slot, the one the binary was built against.
    let Some((child_ver, _, _)) = installed_candidates(root, child_cat, child_pkg)
        .into_iter()
        .find(|(v, _, _)| read_vdb_slot(root, child_cat, child_pkg, v).0 == bound_slot)
    else {
        return false;
    };
    // An excluded installed child refuses the probe.
    if excluded
        .iter()
        .any(|ex| matches_cli_option_atom(ex, &format!("{child_cat}/{child_pkg}-{child_ver}")))
    {
        return false;
    }
    let child_sub = read_vdb_slot(root, child_cat, child_pkg, &child_ver).1;
    let child_sub = if child_sub.is_empty() {
        bound_slot.to_string()
    } else {
        child_sub
    };
    // The probe's `dep.child` is the instance the graph merges, when it sits
    // in the bound slot.
    // Portage: _slot_operator_update_probe (`dep.child` is the graph's pick)
    let (child_ver, child_sub) = match graph_child {
        Some(e) if e.slot.as_deref() == Some(bound_slot) => (
            merge_bound_version(&e.outcome)
                .cloned()
                .unwrap_or(child_ver),
            e.sub_slot.clone().unwrap_or(child_sub),
        ),
        _ => (child_ver, child_sub),
    };
    let Ok(child_cands) = gate.repo_candidates(child_cat, child_pkg) else {
        return false;
    };
    for replacement in replacements {
        let pf = format!("{}-{}", parent.1, replacement.version);
        let Ok(meta) = repo_aux_metadata(&replacement.repo_location, &parent.0, &pf) else {
            continue;
        };
        // The replacement's atoms on the child's cp, USE-evaluated with the binary's USE.
        let mut atoms: Vec<String> = Vec::new();
        for key in dep_keys {
            let Some(tokens) = flat_dep_atoms(
                meta.get(*key).map(String::as_str).unwrap_or_default(),
                use_flags,
            ) else {
                continue;
            };
            for token in tokens {
                let Some(atom) = parse_dep_atom(&token, "built_parent_tokens") else {
                    continue;
                };
                if atom.blocker == portage_dep::Blocker::None
                    && atom.category == child_cat
                    && atom.package == child_pkg
                {
                    atoms.push(portage_dep::without_use(&token).to_string());
                }
            }
        }
        if atoms.is_empty() {
            continue;
        }
        for cand in child_cands.iter() {
            // Portage: `_select_package` honours `runtime_pkg_mask`.
            let plain_neg = format!("!={child_cat}/{child_pkg}-{}", cand.version);
            if cand.source != CandidateSource::Ebuild
                || masked
                    .get(child)
                    .is_some_and(|b| b.iter().any(|m| m.neg == plain_neg))
                || cand.slot != bound_slot
                || cand.sub_slot == child_sub
                || vercmp_ordering(&cand.version, &child_ver) == Ordering::Less
                // The installed version's own ebuild is no update: a sub-slot
                // move without a revbump is insignificant until a parent's
                // rebuild needs it (then the slot-conflict probe takes over).
                || cand.version == child_ver
                || !is_visible(cand, child_cat, child_pkg, config)
                || excluded.iter().any(|ex| {
                    matches_cli_option_atom(
                        ex,
                        &format!("{child_cat}/{child_pkg}-{}", cand.version),
                    )
                })
            {
                continue;
            }
            let cpv = cand.match_str(child_cat, child_pkg);
            if atoms.iter().all(|a| {
                portage_dep::match_from_list(a, &[cpv.as_str()]).is_some_and(|m| !m.is_empty())
            }) && other_consumers_accept(&OtherConsumers {
                root,
                installed,
                live,
                walked,
                entries,
                config,
                parent,
                child,
                cpv: &cpv,
                gate,
            }) {
                return true;
            }
        }
    }
    false
}

/// What [`other_consumers_accept`] checks a candidate child against.
struct OtherConsumers<'a> {
    root: &'a Path,
    installed: &'a InstalledReachability,
    live: &'a HashSet<(String, String)>,
    walked: &'a HashSet<(String, String)>,
    entries: &'a [GraphEntry],
    config: &'a portage_profile::Config,
    parent: &'a (String, String),
    child: &'a (String, String),
    cpv: &'a str,
    gate: CategoryGate<'a>,
}

/// Whether every installed consumer of the child that the graph keeps accepts
/// the candidate child. A consumer this run replaces, one the request's own
/// walk reaches (it has an edge to the child), one with a visible upgrade, or
/// the probed parent itself does not count.
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
fn other_consumers_accept(args: &OtherConsumers<'_>) -> bool {
    let consumers: HashSet<(String, String)> = args
        .live
        .iter()
        .filter(|cp| *cp != args.parent && *cp != args.child)
        // Portage: a parent with a digraph edge to the child is exempt.
        .filter(|cp| !args.walked.contains(*cp))
        .filter(|cp| {
            !args.entries.iter().any(|e| {
                e.category == cp.0 && e.package == cp.1 && merge_bound_version(&e.outcome).is_some()
            })
        })
        .cloned()
        .collect();
    for (consumer, atom) in args.installed.atoms_on(&consumers, args.child) {
        if has_visible_upgrade(args.gate, args.root, args.config, &consumer) {
            continue;
        }
        // A built `:slot/sub=` atom is judged without its slot/sub-slot part.
        // drop-ok: unparseable atom kept verbatim, not dropped
        let atom = match portage_dep::parse_atom(&atom) {
            Some(a) if is_built_slot_op(&a) => strip_slot_part(&atom),
            _ => atom,
        };
        if !portage_dep::match_from_list(&atom, &[args.cpv]).is_some_and(|m| !m.is_empty()) {
            return false;
        }
    }
    true
}

/// `atom` with its `:slot/sub=` suffix cut back to a bare `:=`.
fn strip_slot_part(atom: &str) -> String {
    match atom.find(':') {
        Some(i) => format!("{}:=", &atom[..i]),
        None => atom.to_string(),
    }
}

/// Whether the tree offers a visible ebuild above the installed `cp`.
fn has_visible_upgrade(
    gate: CategoryGate<'_>,
    root: &Path,
    config: &portage_profile::Config,
    cp: &(String, String),
) -> bool {
    let Some(inst) = installed_candidates(root, &cp.0, &cp.1).into_iter().next() else {
        return false;
    };
    let Ok(tree) = gate.repo_candidates(&cp.0, &cp.1) else {
        return false;
    };
    tree.iter().any(|c| {
        c.source == CandidateSource::Ebuild
            && vercmp_ordering(&c.version, &inst.0) == Ordering::Greater
            && is_visible(c, &cp.0, &cp.1, config)
    })
}
