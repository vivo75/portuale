//! Eliminating unnecessary slot-operator rebuilds.

use crate::*;

/// Arguments of [`slot_operator_eliminate_rebuilds`].
pub(crate) struct SlotOperatorEliminateRebuildsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) replace: &'a BTreeSet<(String, String)>,
    pub(crate) slot_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) reverse_pins: &'a [RevDepPin],
    pub(crate) selective: bool,
    pub(crate) changed_slot: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) empty: bool,
    pub(crate) gate: CategoryGate<'a>,
}

/// Portage's `_eliminate_rebuilds`: the undo path. For every cp in the replace
/// set whose walked entry is a `slot_operator_rebuild` reinstall at the
/// installed version, apply rules 1-8 in Portage's order and return the cps
/// whose rebuild is unnecessary (rules 6/8 keep a genuine ABI rebuild).
/// Skipped under `--emptytree` and under any live slot conflict.
// Portage: _emerge/depgraph.py, _eliminate_rebuilds
pub(crate) fn slot_operator_eliminate_rebuilds(
    args: SlotOperatorEliminateRebuildsArgs<'_>,
) -> BTreeSet<(String, String)> {
    let SlotOperatorEliminateRebuildsArgs {
        root,
        entries,
        replace,
        slot_want,
        reverse_pins,
        selective,
        changed_slot,
        top_level_cps,
        empty,
        gate,
    } = args;
    let mut demoted: BTreeSet<(String, String)> = BTreeSet::new();
    if empty || replace.is_empty() {
        return demoted;
    }
    let installed_all = all_installed_packages(root);
    let libc_cps = libc_provider_cps(root);
    let dep_keys = ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"];
    for cp in replace {
        let Some(entry) = entries
            .iter()
            .find(|e| e.category == cp.0 && e.package == cp.1)
        else {
            continue;
        };
        // Only a `slot_operator_rebuild` reinstall at the installed version
        // can be undone; an ordinary upgrade or USE reinstall keeps its
        // rebuild.
        let PretendOutcome::Reinstall {
            version,
            changed_flags,
            slot_operator_rebuild: true,
            ..
        } = &entry.outcome
        else {
            continue;
        };
        // Rule 7 first (it can only keep a rebuild): binary halves are not
        // ported yet, so a binary entry keeps its rebuild.
        if entry.source != CandidateSource::Ebuild {
            continue;
        }
        // The installed instance in the entry's own main slot.
        let entry_slot = entry.slot.as_deref().unwrap_or("0");
        let Some(installed) = installed_all
            .iter()
            .find(|p| p.category == cp.0 && p.package == cp.1 && p.slot == entry_slot)
        else {
            continue;
        };
        // Rule 1.
        if installed.version != *version {
            continue;
        }
        // Rule 2.
        if !changed_flags.is_empty() {
            continue;
        }
        // Rule 3: `--changed-slot` keeps a rebuild whose tree ebuild at the
        // same cpv moved (slot, sub_slot) vs the installed instance.
        if changed_slot && slot_changed(gate, root, &cp.0, &cp.1, version) {
            continue;
        }
        let (i_slot, i_sub) = read_vdb_slot(root, &cp.0, &cp.1, version);
        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
        let installed_str = format!("{}/{}-{version}:{i_slot}/{i_sub}", cp.0, cp.1);
        // Rule 4.
        let parents_match = slot_want
            .get(cp)
            .is_none_or(|atoms| atoms.iter().all(|a| atom_matches_str(a, &installed_str)))
            && reverse_pins
                .iter()
                .filter(|pin| &pin.cp == cp)
                .all(|pin| atom_matches_str(&pin.atom, &installed_str));
        if !parents_match {
            continue;
        }
        // Rule 5.
        if !selective && top_level_cps.contains(cp) {
            continue;
        }
        // Rule 6.
        let (Some(e_slot), Some(e_sub)) = (entry.slot.as_deref(), entry.sub_slot.as_deref()) else {
            continue;
        };
        if (i_slot.as_str(), i_sub.as_str()) != (e_slot, e_sub) {
            continue;
        }
        // Rule 8.
        let Some(metadata) = tree_metadata_for(gate, cp, version) else {
            continue;
        };
        let new_use: HashSet<String> = entry
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let mut equal = true;
        for key in dep_keys {
            let installed_atoms =
                flat_dep_atoms(&read_vdb_string(root, &cp.0, &cp.1, version, key), &new_use)
                    .map(|atoms| strip_libc_atoms(atoms, &libc_cps));
            let bound = bind_slot_operator_deps(
                metadata.get(key).map(String::as_str).unwrap_or_default(),
                entries,
                root,
            );
            let new_atoms =
                flat_dep_atoms(&bound, &new_use).map(|atoms| strip_libc_atoms(atoms, &libc_cps));
            let key_equal = match (installed_atoms, new_atoms) {
                (Some(a), Some(b)) => a == b,
                // Either side's parse failure counts as "not equal", keeping
                // the rebuild.
                _ => false,
            };
            if !key_equal {
                equal = false;
                break;
            }
        }
        if !equal {
            continue;
        }
        demoted.insert(cp.clone());
    }
    demoted
}

/// Arguments of [`slot_operator_rebuild_entries`].
pub(crate) struct SlotOperatorRebuildEntriesArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) update: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) top_level_atoms: &'a HashSet<String>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
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
        entries,
        reachable,
        with_bdeps,
        excluded,
        update,
        top_level_cps,
        top_level_atoms,
        config,
        gate,
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
            entries: &combined,
            reachable,
            scope: None,
            already: &scheduled,
            undone: &BTreeSet::new(),
            with_bdeps,
            update,
            top_level_cps,
            top_level_atoms,
            excluded,
            config,
            gate,
        });
        abi_rebuilds = pairs;
        if next == scheduled {
            break;
        }
        scheduled = next;
        out = installed
            .iter()
            .filter(|pkg| scheduled.contains(&(pkg.category.clone(), pkg.package.clone())))
            // An upgraded provider that joined the set is already a merge.
            .filter(|pkg| {
                !entries.iter().any(|e| {
                    e.category == pkg.category
                        && e.package == pkg.package
                        && !matches!(
                            e.outcome,
                            PretendOutcome::AlreadyInstalled { .. }
                                | PretendOutcome::NoVisibleCandidate
                                | PretendOutcome::Uninstall { .. }
                        )
                })
            })
            .map(|pkg| {
                // The rebuild lands at the tree ebuild's SLOT, not the vdb's.
                let (v_slot, v_sub) =
                    read_vdb_slot(root, &pkg.category, &pkg.package, &pkg.version);
                let (slot, sub_slot, tree_repo) = gate
                    .repo_candidates(&pkg.category, &pkg.package)
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
                    build_time: None,
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
    InstalledReachability::new(root).reachable(world_selected_atoms, system_atoms)
}

type Cp = (String, String);

/// The installed dependency graph of one `root`, with each package's outgoing
/// `cat/pkg` edges parsed once. Several [`reachable`](Self::reachable) walks
/// (the seed sets differ, the edges do not) share the `USE` and four `*DEPEND`
/// reads. Lives for one caller; nothing is cached across calls.
pub struct InstalledReachability {
    installed: std::sync::Arc<Vec<InstalledPackage>>,
    by_cp: HashMap<Cp, usize>,
    root: PathBuf,
    edges: RefCell<HashMap<usize, Vec<Cp>>>,
}

impl InstalledReachability {
    /// Index the installed packages of `root` by `cat/pkg`.
    pub fn new(root: &Path) -> Self {
        let installed = all_installed_packages(root);
        let by_cp = installed
            .iter()
            .enumerate()
            .map(|(i, p)| ((p.category.clone(), p.package.clone()), i))
            .collect();
        Self {
            installed,
            by_cp,
            root: root.to_path_buf(),
            edges: RefCell::new(HashMap::new()),
        }
    }

    fn matches_cp(&self, atom_str: &str) -> Option<Cp> {
        let a = parse_dep_atom(atom_str, "eliminate_matches_cp")?;
        let cp = (a.category, a.package);
        // A slot/version-constrained seed still only needs cp-level
        // reachability here.
        self.by_cp.contains_key(&cp).then_some(cp)
    }

    /// Installed `cat/pkg` edges out of package `i`, in dep-string order.
    fn edges_of(&self, i: usize) -> Vec<Cp> {
        if let Some(found) = self.edges.borrow().get(&i) {
            return found.clone();
        }
        let p = &self.installed[i];
        let root = self.root.as_path();
        let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
        let mut out = Vec::new();
        for dep_key in ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
            if depstr.trim().is_empty() {
                continue;
            }
            let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            out.extend(atoms.iter().filter_map(|a| self.matches_cp(a)));
        }
        self.edges.borrow_mut().insert(i, out.clone());
        out
    }

    /// Every `cat/pkg` that one of the installed packages in `from` depends on,
    /// other than itself. Blockers (`!atom`) are not dependencies.
    pub fn targets_of(&self, from: &HashSet<Cp>) -> HashSet<Cp> {
        let mut out = HashSet::new();
        for cp in from {
            let Some(&i) = self.by_cp.get(cp) else {
                continue;
            };
            let p = &self.installed[i];
            let root = self.root.as_path();
            let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
            for dep_key in ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
                let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
                let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                    continue;
                };
                out.extend(
                    atoms
                        .iter()
                        .filter(|a| !a.starts_with('!'))
                        .filter_map(|a| self.matches_cp(a))
                        .filter(|target| target != cp),
                );
            }
        }
        out
    }

    /// The dependency atoms the installed packages in `from` record on `target`
    /// (blockers excluded), as `(consumer, atom)` pairs with USE conditionals
    /// evaluated against the consumer's own USE.
    // Portage: depgraph.py, _slot_operator_check_reverse_dependencies (_parent_atoms)
    pub fn atoms_on(&self, from: &HashSet<Cp>, target: &Cp) -> Vec<(Cp, String)> {
        let mut out = Vec::new();
        for cp in from {
            let Some(&i) = self.by_cp.get(cp) else {
                continue;
            };
            let p = &self.installed[i];
            let root = self.root.as_path();
            let use_flags = read_vdb_flag_set(root, &p.category, &p.package, &p.version, "USE");
            for dep_key in ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
                let depstr = read_vdb_string(root, &p.category, &p.package, &p.version, dep_key);
                let Some(atoms) = flat_dep_atoms(&depstr, &use_flags) else {
                    continue;
                };
                for atom in atoms.iter().filter(|a| !a.starts_with('!')) {
                    if let Some(a) = parse_dep_atom(atom, "eliminate_dep_atoms")
                        && (&a.category, &a.package) == (&target.0, &target.1)
                    {
                        out.push((cp.clone(), atom.clone()));
                    }
                }
            }
        }
        out
    }

    /// The closure of `world_selected_atoms` ∪ `system_atoms` over the edges.
    pub fn reachable(
        &self,
        world_selected_atoms: &[String],
        system_atoms: &[String],
    ) -> HashSet<Cp> {
        self.reachable_without(world_selected_atoms, system_atoms, &HashSet::new())
    }

    /// Like [`Self::reachable`], but a `skip` member (an installed package this
    /// run replaces, so no graph node) is listed without following its edges.
    // Portage: depgraph.py, _complete_graph (only graph nodes pull their deps in)
    pub fn reachable_without(
        &self,
        world_selected_atoms: &[String],
        system_atoms: &[String],
        skip: &HashSet<Cp>,
    ) -> HashSet<Cp> {
        let mut reachable: HashSet<Cp> = HashSet::new();
        let mut queue: Vec<Cp> = Vec::new();
        for atom_str in world_selected_atoms.iter().chain(system_atoms) {
            if let Some(cp) = self.matches_cp(atom_str)
                && reachable.insert(cp.clone())
            {
                queue.push(cp);
            }
        }
        while let Some(cp) = queue.pop() {
            if skip.contains(&cp) {
                continue;
            }
            let Some(&i) = self.by_cp.get(&cp) else {
                continue;
            };
            for next in self.edges_of(i) {
                if reachable.insert(next.clone()) {
                    queue.push(next);
                }
            }
        }
        reachable
    }
}

/// `<version>` with a trailing `-r<digits>` revision suffix removed.
/// Returns the input unchanged when there is no such suffix.
pub(crate) fn strip_revision(version: &str) -> &str {
    match version.rsplit_once("-r") {
        Some((base, rev)) if !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => version,
    }
}

/// Arguments of [`rebuild_if_restart_parents`].
pub(crate) struct RebuildIfRestartParentsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    /// Parents a previous pass already seeded: naming one again would
    /// restart forever (Portage's `rebuild_list` membership check in
    /// `_trigger_rebuild`).
    pub(crate) already: &'a BTreeSet<(String, String)>,
    pub(crate) unbuilt: bool,
    pub(crate) new_rev: bool,
    pub(crate) new_ver: bool,
    pub(crate) rebuild_exclude: &'a [String],
    pub(crate) rebuild_ignore: &'a [String],
}

/// Portage's `_rebuild_config.trigger_rebuilds`
/// (`--rebuild-if-unbuilt` / `--rebuild-if-new-rev` /
/// `--rebuild-if-new-ver`): over the walk's reached set (merges *and*
/// kept-installed nodes), name the installed parents whose walked
/// buildtime edge lands on a dep this run merges. The caller restarts the
/// walk with those parents force-reinstalled; only `RDEPEND`/`PDEPEND`
/// edges never qualify, `--rebuild-exclude` matches the parent,
/// `--rebuild-ignore` the dep.
/// // Portage: _emerge/depgraph.py, _rebuild_config
pub(crate) fn rebuild_if_restart_parents(
    args: RebuildIfRestartParentsArgs<'_>,
) -> BTreeSet<(String, String)> {
    let RebuildIfRestartParentsArgs {
        root,
        entries,
        already,
        unbuilt,
        new_rev,
        new_ver,
        rebuild_exclude,
        rebuild_ignore,
    } = args;
    if !(unbuilt || new_rev || new_ver) {
        return BTreeSet::new();
    }
    // `cat/pkg` -> merged version, for every entry installed from an ebuild
    // this run. A dep merged from a binary is `built` and never triggers
    // (Portage `built=(type_name != "ebuild")`); a removal merges nothing.
    let mut merged: HashMap<(String, String), String> = HashMap::new();
    for e in entries {
        if e.source != CandidateSource::Ebuild {
            continue;
        }
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version)
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
            PretendOutcome::AlreadyInstalled { .. }
            | PretendOutcome::NoVisibleCandidate
            | PretendOutcome::Uninstall { .. } => None,
        };
        if let Some(v) = version {
            merged.insert((e.category.clone(), e.package.clone()), v.clone());
        }
    }
    if merged.is_empty() {
        return BTreeSet::new();
    }

    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for e in entries {
        // Only a kept-installed package can be rebuilt: Portage records the
        // edge only for a built parent, and a parent that already merges
        // needs no rebuild.
        let PretendOutcome::AlreadyInstalled { version } = &e.outcome else {
            continue;
        };
        let cp = (e.category.clone(), e.package.clone());
        if already.contains(&cp) {
            continue;
        }
        let parent_cpv = format!("{}/{}-{version}", e.category, e.package);
        if rebuild_exclude
            .iter()
            .any(|a| matches_cli_option_atom(a, &parent_cpv))
        {
            continue;
        }
        // The walked buildtime (`DEPEND`/`BDEPEND`) edges, as the pass
        // recorded them -- the parent's own expansion, not the vdb snapshot.
        let triggered = e
            .deps
            .iter()
            .filter(|d| d.priority.buildtime)
            .filter_map(|d| {
                let dep_cp = (d.category.clone(), d.package.clone());
                merged.get(&dep_cp).map(|v| (dep_cp, v.clone()))
            })
            .any(|((dc, dp), merged_ver)| {
                let dep_cpv = format!("{dc}/{dp}-{merged_ver}");
                if rebuild_ignore
                    .iter()
                    .any(|a| matches_cli_option_atom(a, &dep_cpv))
                {
                    return false;
                }
                if unbuilt {
                    return true;
                }
                let installed = installed_versions(root, &dc, &dp);
                if new_rev {
                    return !installed.iter().any(|v| v == &merged_ver);
                }
                // new_ver: revision ignored on both sides.
                let want = strip_revision(&merged_ver);
                !installed.iter().any(|v| strip_revision(v) == want)
            });
        if triggered {
            out.insert(cp);
        }
    }
    out
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
    for pkg in all_installed_packages(root).iter() {
        let cp = (pkg.category.clone(), pkg.package.clone());
        // Portage `_rebuild_config.add` records a rebuild edge only for a
        // parent already in the graph (an installed node with a buildtime
        // edge to the dep); `trigger_rebuilds` walks that graph alone. A
        // package this run never reaches is therefore never rebuilt, even
        // when its vdb build-dep names a merged package (S0: `-u
        // --rebuild-if-* dev-libs/rebuildtrigger` merges the trigger alone;
        // the unreached `rebuildconsumer` gets no `R` row).
        if !in_graph.contains(&cp) {
            continue;
        }
        // A reached package keeps the outcome the walk gave it: flipping it
        // post-walk would skip walking its own deps, while Portage restarts
        // the whole walk to rebuild it. That re-walk is a separate gap, so
        // neither arm synthesises anything today.
        if in_graph.contains(&cp) {
            continue;
        }
        let parent_cpv = pkg.cpv();
        if rebuild_exclude
            .iter()
            .any(|a| matches_cli_option_atom(a, &parent_cpv))
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
            .filter_map(|t| parse_dep_atom(t, "eliminate_flat_tokens"))
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
                    .any(|a| matches_cli_option_atom(a, &dep_cpv))
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
            build_time: None,
            deps: Vec::new(),
        });
    }
    out.sort_by(|a, b| (a.category.as_str(), a.package.as_str()).cmp(&(&b.category, &b.package)));
    out
}
