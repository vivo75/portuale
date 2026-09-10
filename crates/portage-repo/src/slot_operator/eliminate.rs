//! Eliminating unnecessary slot-operator rebuilds.

use crate::*;

/// Arguments of [`slot_operator_eliminate_rebuilds`].
pub(crate) struct SlotOperatorEliminateRebuildsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) replace: &'a BTreeSet<(String, String)>,
    pub(crate) slot_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) reverse_pins: &'a [RevDepPin],
    pub(crate) selective: bool,
    pub(crate) changed_slot: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) empty: bool,
}

/// Arguments of [`slot_operator_rebuild_entries`].
pub(crate) struct SlotOperatorRebuildEntriesArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) update: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) top_level_atoms: &'a HashSet<String>,
    pub(crate) config: &'a portage_profile::Config,
}

/// Legacy path for the `--solver=pubgrub` / `--solver=resolvo` bridges only:
/// those engines re-drive no pass, so they keep the post-pass synthesiser --
/// one `slot_operator_rebuild` reinstall entry per scheduled consumer,
/// iterated until the set stops growing. The default solver path seeds the
/// consumer as a walked node instead.
pub(crate) fn slot_operator_rebuild_entries(
    args: SlotOperatorRebuildEntriesArgs<'_>,
) -> (Vec<GraphEntry>, Vec<(String, String)>) {
    let SlotOperatorRebuildEntriesArgs {
        root,
        repos,
        entries,
        reachable,
        with_bdeps,
        excluded,
        update,
        top_level_cps,
        top_level_atoms,
        config,
    } = args;
    let installed = all_installed_packages(root);
    let mut scheduled: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out: Vec<GraphEntry> = Vec::new();
    let mut abi_rebuilds: Vec<(String, String)>;
    loop {
        let mut combined: Vec<GraphEntry> = entries.to_vec();
        combined.extend(out.iter().cloned());
        let (next, pairs, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
            root,
            repos,
            entries: &combined,
            reachable,
            already: &scheduled,
            undone: &BTreeSet::new(),
            with_bdeps,
            update,
            top_level_cps,
            top_level_atoms,
            excluded,
            config,
        });
        abi_rebuilds = pairs;
        if next == scheduled {
            break;
        }
        scheduled = next;
        out = installed
            .iter()
            .filter(|pkg| scheduled.contains(&(pkg.category.clone(), pkg.package.clone())))
            .map(|pkg| {
                // The rebuild lands at the tree ebuild's SLOT, not the vdb's.
                let (v_slot, v_sub) =
                    read_vdb_slot(root, &pkg.category, &pkg.package, &pkg.version);
                let (slot, sub_slot, tree_repo) =
                    list_candidates(repos, &pkg.category, &pkg.package)
                        .ok()
                        .and_then(|cands| {
                            cands
                                .iter()
                                .find(|c| c.version == pkg.version)
                                .map(|c| (c.slot.clone(), c.sub_slot.clone(), c.repo_name.clone()))
                        })
                        .map(|(slot, sub_slot, repo)| (slot, sub_slot, Some(repo)))
                        .unwrap_or_else(|| (v_slot.clone(), v_sub.clone(), None));
                // The installed repo, `__unknown__` when the vdb records none.
                let installed_repo =
                    installed_pkg_repo(root, &pkg.category, &pkg.package, &pkg.version);
                // The rebuilt cpv is already installed, so the `[oldver]`
                // bracket shows when the rebuild lands at a different
                // slot/sub-slot or repo than the installed instance.
                let repo_drift = tree_repo
                    .as_deref()
                    .is_some_and(|tree| tree != installed_repo.as_str());
                let oldbest = if (slot.as_str(), sub_slot.as_str())
                    != (v_slot.as_str(), v_sub.as_str())
                    || repo_drift
                {
                    vec![InstalledRef {
                        version: pkg.version.clone(),
                        slot: v_slot,
                        sub_slot: v_sub,
                        repo: installed_repo.clone(),
                    }]
                } else {
                    Vec::new()
                };
                // The entry is the tree package being merged, not the installed
                // instance.
                let entry_repo = tree_repo
                    .filter(|tree| !tree.is_empty())
                    .or_else(|| (installed_repo != "__unknown__").then(|| installed_repo.clone()));
                GraphEntry {
                    discovery: 0,
                    category: pkg.category.clone(),
                    package: pkg.package.clone(),
                    outcome: PretendOutcome::Reinstall {
                        version: pkg.version.clone(),
                        changed_flags: Vec::new(),
                        deps_changed: false,
                        slot_changed: false,
                        rebuilt_binary: false,
                        new_repo: false,
                        slot_operator_rebuild: true,
                    },
                    slot: Some(slot),
                    sub_slot: Some(sub_slot),
                    repo_name: entry_repo,
                    oldbest,
                    blockers: Vec::new(),
                    use_flags_display: Vec::new(),
                    use_expand_display: Vec::new(),
                    use_expand_display_p: Vec::new(),
                    keyword_mask: None,
                    new_slot: false,
                    interactive: false,
                    fetch_restrict: false,
                    fetch_restrict_satisfied: false,
                    download_files: Vec::new(),
                    required_by: Vec::new(),
                    source: CandidateSource::Ebuild,
                    provenance: VisibilityProvenance::default(),
                    keyword_suggestion: None,
                    use_suggestion: None,
                    parent_use_suggestion: None,
                    targets_running_root: false,
                    remote_binary: false,
                    build_id: None,
                    deps: Vec::new(),
                }
            })
            .collect();
    }
    (out, abi_rebuilds)
}

/// Every installed package reachable over the installed dependency graph from
/// the required sets (`@world` ∪ `@selected` ∪ `@system`). Gates
/// `slot_operator_rebuild_entries`: Portage only slot-op-rebuilds a consumer
/// that is in the graph.
// Portage: _emerge/depgraph.py, _complete_graph
pub fn required_set_reachable_cps(
    root: &Path,
    world_selected_atoms: &[String],
    system_atoms: &[String],
) -> HashSet<(String, String)> {
    let installed = all_installed_packages(root);
    let by_cp: HashMap<(&str, &str), &InstalledPackage> = installed
        .iter()
        .map(|p| ((p.category.as_str(), p.package.as_str()), p))
        .collect();
    let matches_cp = |atom_str: &str| -> Option<(String, String)> {
        let a = portage_dep::parse_atom(atom_str)?;
        let p = by_cp.get(&(a.category.as_str(), a.package.as_str()))?;
        // A slot/version-constrained seed still only needs cp-level
        // reachability here.
        Some((p.category.clone(), p.package.clone()))
    };

    let mut reachable: HashSet<(String, String)> = HashSet::new();
    let mut queue: Vec<(String, String)> = Vec::new();
    for atom_str in world_selected_atoms.iter().chain(system_atoms) {
        if let Some(cp) = matches_cp(atom_str)
            && reachable.insert(cp.clone())
        {
            queue.push(cp);
        }
    }
    while let Some((cat, pkg)) = queue.pop() {
        let Some(p) = by_cp.get(&(cat.as_str(), pkg.as_str())) else {
            continue;
        };
        let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
        for dep_key in ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
            if depstr.trim().is_empty() {
                continue;
            }
            let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            for atom_str in atoms {
                if let Some(cp) = matches_cp(&atom_str)
                    && reachable.insert(cp.clone())
                {
                    queue.push(cp);
                }
            }
        }
    }
    reachable
}

/// `<version>` with a trailing `-r<digits>` revision suffix removed.
/// Returns the input unchanged when there is no such suffix.
pub(crate) fn strip_revision(version: &str) -> &str {
    match version.rsplit_once("-r") {
        Some((base, rev)) if !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => version,
    }
}

/// Arguments of [`rebuild_if_entries`].
pub(crate) struct RebuildIfEntriesArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) unbuilt: bool,
    pub(crate) new_rev: bool,
    pub(crate) new_ver: bool,
    pub(crate) rebuild_exclude: &'a [String],
    pub(crate) rebuild_ignore: &'a [String],
}

/// Portage's `_rebuild_config` (`--rebuild-if-unbuilt` / `--rebuild-if-new-rev`
/// / `--rebuild-if-new-ver`): an already-installed package whose own build-time
/// dependency is satisfied by a package this run merges is scheduled for a
/// rebuild. `--rebuild-exclude` matches the parent, `--rebuild-ignore` the dep.
// Portage: _emerge/depgraph.py, _rebuild_config
pub(crate) fn rebuild_if_entries(args: RebuildIfEntriesArgs<'_>) -> Vec<GraphEntry> {
    let RebuildIfEntriesArgs {
        root,
        entries,
        unbuilt,
        new_rev,
        new_ver,
        rebuild_exclude,
        rebuild_ignore,
    } = args;
    if !(unbuilt || new_rev || new_ver) {
        return Vec::new();
    }
    // `cat/pkg` -> merged version, for every entry being installed this run.
    let mut merged: HashMap<(String, String), String> = HashMap::new();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    for e in entries {
        let cp = (e.category.clone(), e.package.clone());
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version)
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
            // A removal merges nothing.
            PretendOutcome::AlreadyInstalled { .. }
            | PretendOutcome::NoVisibleCandidate
            | PretendOutcome::Uninstall { .. } => None,
        };
        if let Some(v) = version {
            in_graph.insert(cp.clone());
            merged.insert(cp, v.clone());
        }
    }
    if merged.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<GraphEntry> = Vec::new();
    for pkg in all_installed_packages(root) {
        let cp = (pkg.category.clone(), pkg.package.clone());
        if in_graph.contains(&cp) {
            continue;
        }
        let parent_cpv = pkg.cpv();
        if rebuild_exclude
            .iter()
            .any(|a| matches_config_entry(a, &parent_cpv, &pkg.category, &pkg.package))
        {
            continue;
        }
        let built_use = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
        let mut depstr = String::new();
        for key in ["DEPEND", "BDEPEND"] {
            depstr.push_str(&read_vdb_string(
                root,
                &pkg.category,
                &pkg.package,
                &pkg.version,
                key,
            ));
            depstr.push(' ');
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let Ok(flat) = portage_use_reduce::use_reduce_flat(
            &tokens,
            &built_use,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };

        let triggered = flat
            .iter()
            .filter_map(|t| portage_dep::parse_atom(t))
            .any(|atom| {
                if atom.blocker != portage_dep::Blocker::None {
                    return false;
                }
                let Some(merged_ver) = merged.get(&(atom.category.clone(), atom.package.clone()))
                else {
                    return false;
                };
                let dep_cpv = format!("{}/{}-{merged_ver}", atom.category, atom.package);
                if rebuild_ignore
                    .iter()
                    .any(|a| matches_config_entry(a, &dep_cpv, &atom.category, &atom.package))
                {
                    return false;
                }
                if unbuilt {
                    return true;
                }
                let installed = installed_versions(root, &atom.category, &atom.package);
                if new_rev {
                    return !installed.iter().any(|v| v == merged_ver);
                }
                // new_ver: revision ignored on both sides.
                let want = strip_revision(merged_ver);
                !installed.iter().any(|v| strip_revision(v) == want)
            });
        if !triggered {
            continue;
        }

        let (slot, sub_slot) = read_vdb_slot(root, &pkg.category, &pkg.package, &pkg.version);
        let repo = read_vdb_string(
            root,
            &pkg.category,
            &pkg.package,
            &pkg.version,
            "repository",
        )
        .trim()
        .to_string();
        out.push(GraphEntry {
            discovery: 0,
            category: pkg.category.clone(),
            package: pkg.package.clone(),
            outcome: PretendOutcome::Reinstall {
                version: pkg.version.clone(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            },
            blockers: Vec::new(),
            slot: Some(slot),
            sub_slot: Some(sub_slot),
            repo_name: (!repo.is_empty()).then_some(repo),
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: Vec::new(),
            source: CandidateSource::Ebuild,
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        });
    }
    out.sort_by(|a, b| (a.category.as_str(), a.package.as_str()).cmp(&(&b.category, &b.package)));
    out
}
