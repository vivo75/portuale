//! Installed packages pulled into the merge-order graph.

use crate::merge_order::*;

/// Real `mypriority.satisfied = inst_pkg`: whether an installed package
/// matches `edge`'s atom (`installed_by_cp` is `installed_candidates_by_cp`
/// -- real `vardb.match_pkgs(atom)`), with the `:=`/`:*`-style narrowing
/// `build_digraph` documents: for a `buildtime_slot_op`/`runtime_slot_op`
/// edge the match must carry the resolved child's own slot/sub-slot, so a
/// sub-slot bump doesn't read as satisfied. `child_slot` `None` means the
/// caller has no resolved child (plain atom check).
///
/// Extracted from `build_digraph` so the #68/#72 B2 satisfied-blocker
/// predicate (`portuale`'s `replacement_wait_index`) reads exactly the
/// same rule instead of re-deriving it (T4).
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

/// The [`edge_satisfied_with`] rule against the live vdb, for callers that
/// did not already cache `installed_candidates_by_cp`. #68/#72 B2 is the
/// first such caller: `GraphEntry::deps` never carries the computed
/// `DepPriority::satisfied` bit (`build_digraph` sets it on its own edge
/// copies only), so the satisfied-blocker predicate must ask this itself.
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

/// Every installed package's `cat/pkg-version:slot/sub_slot` candidate
/// string, grouped by `cat/pkg` -- the input `DepPriority::satisfied`
/// needs (real `vardb.match_pkgs(atom)`).
pub(crate) fn installed_candidates_by_cp(root: &Path) -> HashMap<(String, String), Vec<String>> {
    let mut by_cp: HashMap<(String, String), Vec<String>> = HashMap::new();
    for p in all_installed_packages(root) {
        let (slot, sub_slot) = read_vdb_slot(root, &p.category, &p.package, &p.version);
        by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push(format!(
                "{}/{}-{}:{slot}/{sub_slot}",
                p.category, p.package, p.version
            ));
    }
    by_cp
}

/// A synthetic `AlreadyInstalled` graph node for an installed package
/// that the resolve never created an entry for -- pulled in only to
/// complete real `_complete_graph`'s installed-dependency tree for merge
/// ordering. `serialize_merge_order` filters every index `>= real_n`
/// (these) back out of the scheduled list before returning.
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

/// Real `_complete_graph`'s effect on `_serialize_tasks`: every installed
/// "nomerge" node in the digraph carries its own **recorded vdb
/// dependency tree**, recursively -- so leaf selection clears a shallow
/// installed subtree (`x11-libs/xtrans` -> `app-portage/elt-patches` ->
/// `sys-apps/gentoo-functions`) before a deep one (`x11-base/xorg-proto`
/// -> `dev-build/meson` -> the whole Python build stack) and merges the
/// package sitting on the shallow one first.
///
/// `build_digraph` only follows `GraphEntry::deps`, and an
/// `AlreadyInstalled` entry has none -- so portuale's graph truncated
/// every installed node at depth 1 (`(no children)` in the `--debug`
/// dump) and treated `meson`/`elt-patches` alike as instant leaves. This
/// walks the forward transitive closure: fill `deps` on every installed
/// entry from its vdb `*DEPEND` (USE-reduced against its recorded `USE`,
/// with real `pkg.built` priorities so build deps are `optional`), and
/// append a [`synthetic_installed_entry`] for each installed dependency
/// not already present, to a fixpoint.
///
/// Bounded by the installed set. In complete mode this also seeds from
/// the whole `@system` set (real `_complete_graph` seeds `@world` /
/// `@system` as `SetArg`s and walks them deep): an installed `@system`
/// package like `app-shells/bash` or `dev-libs/gmp` -- reached from
/// `sys-libs/glibc`'s optional `sys-devel/gcc` edge -- is a graph node
/// with its own tree even when nothing being merged reaches it. That
/// installed bulk is `_ignore_optional` ballast: real drains it one node
/// at a time in `DepPriorityNormalRange`, which paces the frontier so a
/// package sitting on a real runtime cycle (`dev-lang/perl` on the
/// `glibc`/`libcrypt` cycle) is not front-loaded past an unrelated merge
/// (`dev-cpp/eigen`) via a premature `drop_satisfied`. The nodes real's
/// own "Prune 'nomerge' root nodes" step then drops are removed again in
/// `serialize_merge_order` right after `build_digraph`.
pub(crate) fn add_installed_dependency_closure(args: AddInstalledDependencyClosureArgs<'_>) {
    let AddInstalledDependencyClosureArgs {
        entries,
        root,
        repos,
        system_atoms,
        virtuals_only,
        dynamic_deps,
    } = args;
    // `virtuals_only` (a plain `[ebuild N]` resolve, real not in complete
    // mode): real still expands an installed `virtual/*` node to its
    // provider (`virtual/pkgconfig` -> `dev-util/pkgconf`) -- new-style
    // virtuals are "free" and always walked -- but leaves every
    // non-virtual installed node a `(no children)` leaf. So only a node
    // whose own category is `virtual` gets its deps walked; the provider
    // it names is added as a leaf, not queued (unless it is itself a
    // virtual).
    let installed = all_installed_packages(root);
    // B1: every installed *version* of a cp, not one (a `cp -> single pkg`
    // map silently dropped all but the last-installed slot; real's
    // `_complete_graph` keeps every installed slot -- gtk:4's missing
    // `docbook-xml-dtd-{4.2,4.4,4.5}` nodes were exactly this).
    let by_cp: HashMap<(&str, &str), Vec<&crate::InstalledPackage>> = {
        let mut m: HashMap<(&str, &str), Vec<&crate::InstalledPackage>> = HashMap::new();
        for p in installed.iter() {
            m.entry((p.category.as_str(), p.package.as_str()))
                .or_default()
                .push(p);
        }
        m
    };
    // Pick the installed version an edge's atom names -- the highest
    // matching version when several match, the highest overall when the
    // atom is absent/unparseable. The candidate string carries the main
    // slot (`:4.2` style atoms are what pull sibling docbook slots in).
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

    // A2 (#26): the closure edges come from the raw vdb snapshot
    // (`installed_dep_string`, `InstalledMetaLayer::Raw`) -- deliberately
    // NOT the same view the walk uses (the walk reads the *current
    // ebuild*'s deps under `--dynamic-deps`). Phase 5 S1 removed the vdb
    // built-`:=` append as a documented cut, so the ebuild view would
    // lose the vdb's built :S/SS= atoms; the pre-A2 Raw graph (minus the
    // injected libc) stays the closer approximation, exactly as under
    // the old gate-off default. The injected-libc strip below is therefore only
    // sound on the Raw path (Gate G0.3): `_inject_libc_dep` appends a
    // bare `>=<libc-provider>-<version>` to every installed package's vdb
    // `RDEPEND` (bug #753500), which the ebuild never declared -- real's
    // default walk never sees it, but the `=n` snapshot does, and
    // portuale keeps the historical strip there. A genuine ebuild-written
    // `>=sys-libs/glibc-x` must NOT be stripped now that the default path
    // reads the ebuild.
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
        // Live md5-cache metadata for this exact installed cpv, from the
        // vdb-recorded repo (backlog #86: never a priority search) --
        // `None` when the version is gone there (the Raw fallback).
        let live = crate::live_metadata_for_installed(repos, root, cat, pkg, ver);
        let mut md: HashMap<String, String> = HashMap::new();
        let mut memo: HashMap<(String, String, String, String), String> = HashMap::new();
        // A2 follow-up (unchanged by backlog #25 S1b, which scopes the
        // overlay to the walk/scan sites): the closure stays on Raw
        // under every option set -- the pre-A2 scheduler graph (raw
        // minus the injected libc), which is closer to real's effective
        // view than the ebuild alone.
        let layer = crate::InstalledMetaLayer::Raw;
        for k in ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"] {
            // The `false` below is dead under `Raw` (the flag only gates
            // the Effective append); backlog #25 S1b scopes the overlay
            // to the walk/scan sites, so the closure keeps Raw. Thread
            // the real flag through `add_installed_dependency_closure`
            // and its callers if this layer ever changes.
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

    // Seed 1: every installed dependency named by an *already-resolved*
    // entry (merge-bound or installed) that the resolve never made a
    // node for -- an installed package satisfying a dep is not walked, so
    // `dev-perl/common-sense`'s edge to the installed `dev-lang/perl`
    // (and perl's own deep tree) was simply missing. Real
    // `_complete_graph` has every one of these.
    // `virtuals_only`: a node just added is queued for its own dep walk
    // only if the whole closure is wanted, or it is itself a `virtual/*`.
    // B1: `present` is keyed by cpv, and `add_node` selects the installed
    // version the edge's own atom names -- real keeps every installed
    // slot, and keying by cp alone dropped all but the first
    // (`docbook-xml-dtd:4.2`/`:4.4`/`:4.5` vs the already-present 4.1.2).
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

    // Seed 1b (complete mode only): real `_complete_graph` seeds a
    // `SetArg` for `@world`/`@system` and adds every atom in them, then
    // walks the lot deep. Add each installed `@system` package as a node
    // and queue its own dep walk. `serialize_merge_order`'s prune drops
    // the ones no merge-bound package reaches -- but the deep-only
    // members (`dev-libs/gmp` under `glibc` -> `gcc`, `sys-libs/readline`
    // under `bash`, ...) stay, exactly as real's own post-prune graph
    // keeps them.
    //
    // R5 (#17): real `_resolve` (`depgraph.py:5500`) processes this
    // `SetArg`'s own atom list `sorted(arg.pset.getAtoms(), key=str)` --
    // the same seed-order fact R3b already fixed for an *explicit*
    // `@world`/`@system` top-level target (`pretend.rs`'s
    // `expand_top_level_atoms`). This is the other place the identical
    // unsorted-profile-order seed survives: *every* complete-mode probe
    // (not just an explicit `@system`/`@world` argument) walks
    // `system_atoms` here to build the installed closure, and an
    // unsorted seed skews the DFS discovery order the pre-bias sort
    // preserves -- exactly the `_serialize_tasks` "installed-chain"
    // front-load family (`pyproject-metadata`, `freetype`, …) `TEST/
    // findings/l0.md` "## I" names as the outstanding wall.
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

    // Seed 2: every installed-outcome entry whose deps were never filled
    // (and, in `virtuals_only` mode, is a `virtual/*`).
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
