//! Installed packages pulled into the merge-order graph.

use crate::merge_order::*;

/// Whether an installed package matches `edge`'s atom. For a slot-operator edge
/// the match must carry the resolved child's own slot/sub-slot.
/// `child_slot` `None` means no resolved child (plain atom check).
pub(crate) fn edge_satisfied_with(
    installed_by_cp: &HashMap<(String, String), Vec<String>>,
    edge: &DepEdge,
    child_slot: Option<(&str, &str)>,
) -> bool {
    let Some(cands) = installed_by_cp.get(&(edge.category.clone(), edge.package.clone())) else {
        return false;
    };
    let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(&edge.atom, &refs) else {
        return false;
    };
    if matched.is_empty() {
        return false;
    }
    if edge.priority.buildtime_slot_op || edge.priority.runtime_slot_op {
        let Some((slot, sub_slot)) = child_slot else {
            return true;
        };
        return matched.iter().any(|m| {
            portage_dep::parse_candidate(m).is_some_and(|c| {
                c.slot.as_deref() == Some(slot) && c.sub_slot.as_deref() == Some(sub_slot)
            })
        });
    }
    true
}

/// The `edge_satisfied_with` rule against the live vdb (the `satisfied` bit lives
/// on the digraph's edge copies only, so this predicate asks it itself).
pub fn dep_edge_satisfied_by_installed(
    root: &Path,
    edge: &DepEdge,
    child: Option<&GraphEntry>,
) -> bool {
    edge_satisfied_with(
        &installed_candidates_by_cp(root),
        edge,
        child.and_then(|c| c.slot.as_deref().zip(c.sub_slot.as_deref())),
    )
}

/// Every installed package's `cat/pkg-version:slot/sub_slot` candidate string,
/// grouped by `cat/pkg`. Memoised per `root` and VDB generation like
/// [`all_installed_packages`]: the per-package `SLOT` reads happen once per
/// generation, not once per caller.
pub(crate) fn installed_candidates_by_cp(
    root: &Path,
) -> std::sync::Arc<HashMap<(String, String), Vec<String>>> {
    type Candidates = std::sync::Arc<HashMap<(String, String), Vec<String>>>;
    static CACHE: crate::OnceLock<crate::RwLock<HashMap<std::path::PathBuf, (u64, Candidates)>>> =
        crate::OnceLock::new();
    let cache = CACHE.get_or_init(|| crate::RwLock::new(HashMap::new()));
    let fingerprint = portage_vdb::for_root(root).generation().unwrap_or(0);
    if let Ok(guard) = cache.read()
        && let Some((fp, found)) = guard.get(root)
        && *fp == fingerprint
    {
        return std::sync::Arc::clone(found);
    }
    let mut by_cp: HashMap<(String, String), Vec<String>> = HashMap::new();
    for p in all_installed_packages(root).iter() {
        let (slot, sub_slot) = read_vdb_slot(root, &p.category, &p.package, &p.version);
        by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push(format!(
                "{}/{}-{}:{slot}/{sub_slot}",
                p.category, p.package, p.version
            ));
    }
    let by_cp = std::sync::Arc::new(by_cp);
    if let Ok(mut guard) = cache.write() {
        guard.insert(
            root.to_path_buf(),
            (fingerprint, std::sync::Arc::clone(&by_cp)),
        );
    }
    by_cp
}

/// A synthetic `AlreadyInstalled` node for an installed package the resolve never
/// made an entry for. Filtered back out of the scheduled list before returning.
pub(crate) fn synthetic_installed_entry(
    category: String,
    package: String,
    version: String,
    deps: Vec<DepEdge>,
) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category,
        package,
        outcome: PretendOutcome::AlreadyInstalled { version },
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name: None,
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
        deps,
    }
}

/// Arguments of [`add_installed_dependency_closure`].
pub(crate) struct AddInstalledDependencyClosureArgs<'a> {
    pub(crate) entries: &'a mut Vec<GraphEntry>,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) system_atoms: &'a [String],
    pub(crate) virtuals_only: bool,
    pub(crate) dynamic_deps: bool,
}

/// Fill `deps` on every installed entry from its vdb `*DEPEND` and append a
/// synthetic node per missing installed dependency, to a fixpoint. Bounded by
/// the installed set; complete mode also seeds from the whole `@system` set.
/// Portage: depgraph.py, _complete_graph
pub(crate) fn add_installed_dependency_closure(args: AddInstalledDependencyClosureArgs<'_>) {
    let AddInstalledDependencyClosureArgs {
        entries,
        root,
        repos,
        system_atoms,
        virtuals_only,
        dynamic_deps,
    } = args;
    // `virtuals_only` (a plain `[ebuild N]` resolve): only `virtual/*` nodes get
    // their deps walked; the provider is added as a leaf, not queued.
    let installed = all_installed_packages(root);
    // Every installed version of a cp, not one (Portage keeps every installed slot).
    let by_cp: HashMap<(&str, &str), Vec<&crate::InstalledPackage>> = {
        let mut m: HashMap<(&str, &str), Vec<&crate::InstalledPackage>> = HashMap::new();
        for p in installed.iter() {
            m.entry((p.category.as_str(), p.package.as_str()))
                .or_default()
                .push(p);
        }
        m
    };
    // The installed version the edge's atom names (highest match, else highest overall).
    let pick_installed =
        |atom: Option<&str>, cat: &str, pkg: &str| -> Option<&crate::InstalledPackage> {
            let cands = by_cp.get(&(cat, pkg))?;
            let mut best: Option<&crate::InstalledPackage> = None;
            for p in cands {
                if let Some(a) = atom
                    && !portage_dep::match_from_list(
                        a,
                        &[format!("{cat}/{pkg}-{}:{}", p.version, p.slot).as_str()],
                    )
                    .is_some_and(|m| !m.is_empty())
                {
                    continue;
                }
                if best.is_none_or(|b| {
                    portage_versions::vercmp(&b.version, &p.version).is_some_and(|o| o < 0)
                }) {
                    best = Some(p);
                }
            }
            best.or_else(|| {
                cands.iter().copied().max_by(|a, b| {
                    portage_versions::vercmp(&a.version, &b.version)
                        .unwrap_or(0)
                        .cmp(&0)
                })
            })
        };

    // Closure edges come from the raw vdb snapshot, not the ebuild view the walk
    // uses; the injected-libc strip below is only sound on that Raw path.
    let libc_cps = crate::libc_provider_cps(root);
    let is_injected_libc = |atom: &str| -> bool {
        let Some(a) = portage_dep::parse_atom(atom) else {
            return false;
        };
        a.blocker == portage_dep::Blocker::None
            && a.operator == portage_dep::Operator::Ge
            && a.version.is_some()
            && a.slot.is_none()
            && a.sub_slot.is_none()
            && a.use_deps.as_ref().is_none_or(|u| u.is_empty())
            && libc_cps.contains(&(a.category.clone(), a.package.clone()))
    };

    let vdb_edges = |cat: &str, pkg: &str, ver: &str| -> Vec<DepEdge> {
        // Live md5-cache metadata for this installed cpv, from the vdb-recorded repo.
        let live = crate::live_metadata_for_installed(repos, root, cat, pkg, ver);
        let mut md: HashMap<String, String> = HashMap::new();
        let mut memo: HashMap<(String, String, String, String), String> = HashMap::new();
        // The closure stays on Raw under every option set.
        let layer = crate::InstalledMetaLayer::Raw;
        for k in ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            // The `false` below is dead under `Raw`; the overlay never reaches this seam.
            let s = crate::installed_dep_string(crate::InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps: false,
                category: cat,
                package: pkg,
                version: ver,
                live: live.as_deref(),
                key: k,
                layer,
                memo: &mut memo,
            });
            if !s.trim().is_empty() {
                md.insert(k.to_string(), s);
            }
        }
        let use_flags = read_vdb_flag_set(root, cat, pkg, ver, "USE");
        dep_edges_from_metadata(
            &md,
            &use_flags,
            &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"],
            true,
        )
        .into_iter()
        .filter(|e| layer != crate::InstalledMetaLayer::Raw || !is_injected_libc(&e.atom))
        .collect()
    };

    let mut present: HashSet<(String, String, String)> = entries
        .iter()
        .map(|e| {
            (
                e.category.clone(),
                e.package.clone(),
                outcome_version(e).unwrap_or("").to_string(),
            )
        })
        .collect();
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    // Seed 1: installed dependencies named by already-resolved entries that the
    // resolve never made a node for.
    let expandable = |cat: &str| !virtuals_only || cat == "virtual";
    let add_node = |entries: &mut Vec<GraphEntry>,
                    present: &mut HashSet<(String, String, String)>,
                    queue: &mut std::collections::VecDeque<usize>,
                    atom: Option<&str>,
                    cat: &str,
                    pkg: &str| {
        let Some(p) = pick_installed(atom, cat, pkg) else {
            return;
        };
        let key = (p.category.clone(), p.package.clone(), p.version.clone());
        if !present.insert(key) {
            return;
        }
        entries.push(synthetic_installed_entry(
            p.category.clone(),
            p.package.clone(),
            p.version.clone(),
            Vec::new(),
        ));
        if expandable(cat) {
            queue.push_back(entries.len() - 1);
        }
    };

    let seed_targets: Vec<(String, String, String)> = entries
        .iter()
        .flat_map(|e| e.deps.iter())
        .map(|d| (d.atom.clone(), d.category.clone(), d.package.clone()))
        .collect();
    for (atom, cat, pkg) in seed_targets {
        add_node(entries, &mut present, &mut queue, Some(&atom), &cat, &pkg);
    }

    // Seed 1b (complete mode only): each installed `@system` package as a node.
    //
    // Seed atoms are sorted so the DFS discovery order is deterministic.
    let mut system_atoms_sorted = system_atoms.to_vec();
    system_atoms_sorted.sort();
    if !virtuals_only {
        for atom_str in &system_atoms_sorted {
            let Some(atom) = portage_dep::parse_atom(atom_str) else {
                continue;
            };
            add_node(
                entries,
                &mut present,
                &mut queue,
                Some(atom_str),
                &atom.category,
                &atom.package,
            );
        }
    }

    // Seed 2: installed-outcome entries whose deps were never filled.
    for (i, e) in entries.iter().enumerate() {
        if matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. })
            && e.deps.is_empty()
            && expandable(&e.category)
        {
            queue.push_back(i);
        }
    }

    while let Some(i) = queue.pop_front() {
        let PretendOutcome::AlreadyInstalled { version } = entries[i].outcome.clone() else {
            continue;
        };
        if !entries[i].deps.is_empty() {
            continue;
        }
        let (cat, pkg) = (entries[i].category.clone(), entries[i].package.clone());
        let edges = vdb_edges(&cat, &pkg, &version);
        for e in &edges {
            add_node(
                entries,
                &mut present,
                &mut queue,
                Some(&e.atom),
                &e.category,
                &e.package,
            );
        }
        entries[i].deps = edges;
    }
}
