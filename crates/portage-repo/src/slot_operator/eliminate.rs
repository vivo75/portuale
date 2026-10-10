//! Eliminating unnecessary slot-operator rebuilds.

use crate::*;

/// Backlog #24 S4: real `_eliminate_rebuilds` (`depgraph.py:3859-4000`),
/// the undo path. For every cp in the S3 replace set whose walked entry
/// is a `Reinstall { slot_operator_rebuild: true }` at the installed
/// version, apply rules 1-8 of the plan's §1.2 *in real's order* and
/// return the cps whose rebuild is unnecessary (rule 6/8 are what keep a
/// genuine ABI rebuild): the caller drops them from the set and latches
/// them in `slot_operator_undone`.
///
/// Rules, with real's line refs:
/// 1. installed instance at the same cpv (a real upgrade is never undone);
/// 2. `--newuse`/`--changed-use` wins (`pkg in _reinstall_nodes`);
/// 3. `--changed-slot` (3898-3899): keep the rebuild when the tree
///    ebuild at the same cpv moved `(slot, sub_slot)` vs the installed
///    instance. Landed in S5; for an ebuild consumer rule 6 already
///    keeps every such rebuild (the entry's own slot *is* the tree
///    ebuild's), so the line is behaviour-neutral until the binary
///    halves land (v2 `#24c`) -- kept in real's position so that half
///    drops in without moving rules;
/// 4. every parent atom matches the installed instance;
/// 5. non-`selective` only: no user-asked parent;
/// 6. installed and new `(slot, sub_slot)` equal;
/// 7. built `provides`/`requires` equal -- not reachable in v1: the
///    binary halves are v2 `#24c`, so a binary entry keeps its rebuild;
/// 8. the dep comparison: [`flat_dep_atoms`] (use-reduced against the
///    *new* node's enabled USE) of [`bind_slot_operator_deps`]'s bound
///    tree string vs the vdb string, both through [`strip_libc_atoms`].
///
/// Skipped entirely under `--emptytree` (real `"empty" in myparams`) and
/// under any live slot conflict (`entries` cannot see the pass; the
/// caller checks, real bug 922038).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_eliminate_rebuilds(
    root: &Path,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    replace: &BTreeSet<(String, String)>,
    slot_want: &HashMap<(String, String), Vec<String>>,
    reverse_pins: &[RevDepPin],
    selective: bool,
    changed_slot: bool,
    top_level_cps: &HashSet<(String, String)>,
    empty: bool,
) -> BTreeSet<(String, String)> {
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
        // Real `match(root, atom, installed=False)` only ever sees a
        // *non-installed* graph pkg; the S3 flip is the only thing that
        // constructs `slot_operator_rebuild: true`, and an ordinary
        // Upgrade/Reinstall-for-USE entry keeps its rebuild like real's
        // `installed_instance.cpv != pkg.cpv` / `_reinstall_nodes` arms.
        let PretendOutcome::Reinstall {
            version,
            changed_flags,
            slot_operator_rebuild: true,
            ..
        } = &entry.outcome
        else {
            continue;
        };
        // Rule 7 (see the function doc): binary halves are v2. Hoisted
        // above rule 1 (real checks `pkg.built` between rule 6 and 8):
        // the pre-filter can only *keep* a rebuild, and rules 1-6/8 are
        // all keep-on-mismatch, so the only decision it can change is
        // the all-rules-pass demotion that IS rule 7's domain -- no
        // ordering counter-example exists.
        if entry.source != CandidateSource::Ebuild {
            continue;
        }
        // Real `installed_instance = match_pkgs(pkg.slot_atom)[0]`: the
        // installed instance in the *entry's* own main slot.
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
        // Rule 3 (real 3898-3899, S5): `--changed-slot` keeps a rebuild
        // whose tree ebuild at the same cpv moved (slot, sub_slot) vs
        // the installed instance (real `_changed_slot(installed_instance)`).
        // Real also checks `_changed_slot(pkg)`, always false for a
        // merge-bound ebuild entry -- its slot *is* the tree ebuild's --
        // and rule 6 below is the ebuild equivalent, so this is
        // behaviour-neutral until the binary halves land (v2 `#24c`).
        // Kept in real's position so that half drops in without moving
        // the rules.
        if changed_slot && slot_changed(root, repos, &cp.0, &cp.1, version) {
            continue;
        }
        let (i_slot, i_sub) = read_vdb_slot(root, &cp.0, &cp.1, version);
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
        let Some(metadata) = tree_metadata_for(repos, cp, version) else {
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
                // Real `except InvalidDependString: continue` keeps the
                // rebuild on an unparsable *installed* string; treating
                // either side's parse failure as "not equal" keeps it too.
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

/// #24 S3 legacy path, for the `--solver=pubgrub` / `--solver=resolvo`
/// bridges only (`solver_bridge.rs`). Those engines have no
/// `Backtracker`, so they cannot re-drive a pass with the consumer
/// seeded as a walked node the way the default resolver now does; they
/// keep the pre-S3 post-pass synthesiser: run
/// [`slot_operator_rebuild_scan`], build one
/// `Reinstall { slot_operator_rebuild: true }` entry per scheduled
/// consumer, and iterate until the set stops growing (the cascade the
/// scan itself no longer walks -- a rebuild lands at its *tree* ebuild's
/// sub-slot, which when it differs from the vdb's is itself a slot shift
/// the next round chases).
///
/// Same cuts as the v1 synthesiser it is: empty `deps` / `required_by`,
/// so a synthesised rebuild falls back to array position in
/// `topological_merge_order`, and its own `RDEPEND` is never re-walked.
/// The default `--solver=portage` path has none of that -- see
/// `BacktrackParams::slot_operator_replace_installed`.
///
/// #212 cut: the scan's stale-binary triples are dropped here. The
/// bridge resolves in one shot with no `Backtracker`, so there is no
/// re-pass a `slot_operator_mask_built` record could steer (real
/// enforces the mask through the backtrack-config restart,
/// `depgraph.py:5703-5712`); recording one would be dead state. The
/// bridge keeps its pre-#212 answers on stale-binary shapes.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_rebuild_entries(
    root: &Path,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    reachable: &HashSet<(String, String)>,
    with_bdeps: bool,
    excluded: &[String],
    update: bool,
    top_level_cps: &HashSet<(String, String)>,
    top_level_atoms: &HashSet<String>,
    config: &portage_profile::Config,
) -> (Vec<GraphEntry>, Vec<(String, String)>) {
    let installed = all_installed_packages(root);
    let mut scheduled: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out: Vec<GraphEntry> = Vec::new();
    let mut abi_rebuilds: Vec<(String, String)>;
    loop {
        let mut combined: Vec<GraphEntry> = entries.to_vec();
        combined.extend(out.iter().cloned());
        let (next, pairs, ..) = slot_operator_rebuild_scan(
            root,
            repos,
            &combined,
            reachable,
            &scheduled,
            &BTreeSet::new(),
            with_bdeps,
            // #211 I1: the `--solver=` bridge threads the same
            // `update`/`top_level` context the default path passes, so
            // both solvers agree on the new-slot arm (the request
            // carries both -- the "no context at this layer" cut that
            // kept this fixpoint same-slot-only is gone).
            update,
            top_level_cps,
            top_level_atoms,
            excluded,
            config,
        );
        abi_rebuilds = pairs;
        if next == scheduled {
            break;
        }
        scheduled = next;
        out = installed
            .iter()
            .filter(|pkg| scheduled.contains(&(pkg.category.clone(), pkg.package.clone())))
            .map(|pkg| {
                // The rebuild lands at the tree ebuild's SLOT, not the
                // vdb's (real re-reads `SLOT` at merge time).
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
                // The installed repo, `__unknown__` when the vdb records
                // none (real `portage.versions._unknown_repo`) -- the same
                // value `resolve_pretend`'s own oldbest assembly compares
                // (backlog #247).
                let installed_repo =
                    installed_pkg_repo(root, &pkg.category, &pkg.package, &pkg.version);
                // Real `output.py::_get_installed_best` (721-731): the
                // rebuilt cpv is already installed (`replace = True`), so
                // the `[oldver]` bracket shows when the rebuild lands at
                // a different slot/sub-slot than the installed instance
                // (the sub-slot bump driving the cascade) *or* when the
                // repo differs and repo display is not quieted (the third
                // disjunct -- #247 ported it to `resolve_pretend`, #274
                // ports it to this synthesiser assembly;
                // `--quiet-repo-display` stays an unimplemented option
                // exiting 2, so every successful run has it off and the
                // disjunct reduces to the repo comparison).
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
                // The entry is the tree package being merged (real
                // `pkg.repo`), not the installed instance -- the two agree
                // unless the repo drifted, in which case the row keeps
                // `::tree` and the bracket names the installed repo.
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

/// The `(category, package)` of every installed package reachable, over
/// the **installed** dependency graph, from the required sets
/// (`@world` ∪ `@selected` ∪ `@system`) -- real `_complete_graph`'s deep
/// re-walk of those sets in complete mode (`_select_pkg_from_graph`
/// never merges anything, it just pulls the reachable installed closure
/// into the digraph so downstream triggers can see it).
///
/// `world_selected_atoms` is the `world` file's own atoms unioned with
/// every `world_sets` nested set (real `@selected`; portuale's `@world`
/// expands to exactly this -- see `pretend.rs::expand_selected`);
/// `system_atoms` is the profile's own `packages` file.
///
/// The edges are every installed package's vdb `RDEPEND`/`PDEPEND`/
/// `DEPEND`/`BDEPEND`, USE-reduced against its own recorded `USE` and
/// matched by `cat/pkg` (real's `_parent_atoms` is cp-level for this
/// purpose). Used to gate `slot_operator_rebuild_entries` -- real only
/// slot-op-rebuilds a consumer that is in the graph.
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
        // reachability here (real `_parent_atoms` records the cp edge
        // regardless); an installed package is a single instance per cp
        // in portuale's model, so no `match_from_list` narrowing.
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

/// `<version>` with a trailing `-r<digits>` revision suffix removed
/// (real `catpkgsplit(cpv)[:-1]` -- "revision numbers are ignored" for
/// `--rebuild-if-new-ver`). Returns the input unchanged when there is no
/// such suffix.
pub(crate) fn strip_revision(version: &str) -> &str {
    match version.rsplit_once("-r") {
        Some((base, rev)) if !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => version,
    }
}

/// Real `_emerge/depgraph.py`'s `_rebuild_config` (`--rebuild-if-unbuilt`
/// / `--rebuild-if-new-rev` / `--rebuild-if-new-ver`): an already-
/// installed package whose own build-time dependency (`DEPEND`/`BDEPEND`
/// in its vdb, resolved against its *built* `USE`) is satisfied by a
/// package this run is merging is scheduled for a rebuild.
/// `_needs_rebuild(dep_pkg)` -- for portuale every merge is from source
/// (`dep_pkg.built` is always false), so: `unbuilt` triggers always (a
/// source build invalidates any binary parent); `new_rev` triggers
/// unless the merged `cat/pkg-version` (revision included) is already one
/// of the installed versions of that cp; `new_ver` is the same but with
/// the `-r<n>` revision stripped from both sides.
/// The caller resolves the real precedence (`unbuilt` > `new_rev` >
/// `new_ver`; `=n` disables) so at most one of the three is set here.
/// `--rebuild-exclude ATOMS` matches the *parent* (skip rebuilding it);
/// `--rebuild-ignore ATOMS` matches the *dep* (it never triggers a
/// rebuild) -- both the same two-tier `matches_config_entry` matcher
/// `excluded` uses. Structurally a sibling of
/// `slot_operator_rebuild_entries` above (same `all_installed_packages`
/// scan, same `Reinstall` rebuild-entry shape) -- `--rebuild-if-new-slot`
/// stays that function's job.
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn rebuild_if_entries(
    root: &Path,
    entries: &[GraphEntry],
    unbuilt: bool,
    new_rev: bool,
    new_ver: bool,
    rebuild_exclude: &[String],
    rebuild_ignore: &[String],
) -> Vec<GraphEntry> {
    if !(unbuilt || new_rev || new_ver) {
        return Vec::new();
    }
    // `cat/pkg` -> merged version, for every entry being installed this
    // run (New / Upgrade / Downgrade / Reinstall).
    let mut merged: HashMap<(String, String), String> = HashMap::new();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    for e in entries {
        let cp = (e.category.clone(), e.package.clone());
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version)
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
            // #72 B3: a removal merges nothing.
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
