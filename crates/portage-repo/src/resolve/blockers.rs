//! Blockers.

use crate::*;

/// A blocker atom found while flattening one package's own dependency strings,
/// not yet matched against anything -- collected during the BFS in
/// `resolve_pretend_graph` and resolved in a single post-pass (see
/// `resolve_blockers`) once the whole graph's New/Upgrade set is known, so
/// a match doesn't depend on BFS discovery order (two packages can block
/// each other regardless of which one the queue reaches first).
pub(crate) struct PendingBlocker {
    pub(crate) atom_str: String,
    pub(crate) strong: bool,
    /// #143 S1: real's `blocker.priority.buildtime` — the atom came
    /// from a build-time dep key (`DEPEND`/`BDEPEND`). Under
    /// `--buildpkgonly` real clears every blocker's uninstall edge
    /// unless it is hard (`!!`) *and* buildtime
    /// (`depgraph.py::_validate_blockers`), so `resolve_blockers`
    /// needs the provenance to apply the same gate. The all-vdb scan
    /// only reads IDEPEND/PDEPEND/RDEPEND (never buildtime); the two
    /// walk sites derive it from the key the token came from.
    pub(crate) buildtime: bool,
    pub(crate) target_category: String,
    pub(crate) target_package: String,
    pub(crate) owner_key: (String, String),
    pub(crate) owner_version: String,
    /// #68 S2: real's `parent.operation` -- `true` when the producing
    /// owner was walked as a merge (`enqueue_flat_deps`, a New/Upgrade/
    /// Downgrade/Reinstall selected node), `false` for the
    /// already-installed-parent recursion (`enqueue_dependencies`).
    /// `resolve_blockers` refines it from the owner's final outcome in
    /// `entries` when the owner is in the graph (an installed argument
    /// walked through `enqueue_flat_deps` is a `nomerge` node).
    pub(crate) owner_merging: bool,
    /// #77 A1: `true` when this blocker came from the all-installed-
    /// packages runtime-dep scan (`collect_unwalked_installed_blockers`,
    /// real `_validate_blockers`' `for pkg in vardb` loop,
    /// `depgraph.py:8919-9078`). That owner is the **vardb `Package`**,
    /// always `operation == "nomerge"` and a distinct instance from any
    /// same-cp merge entry, so `resolve_blockers` must not override its
    /// nomerge state from the cp's entry the way it does for a walked
    /// owner.
    pub(crate) owner_installed: bool,
}

/// A resolved blocker together with the owner version its renderer
/// needs: `resolve_blockers` files these, `file_blocker_conflicts`
/// hangs them on entries, removals, or (backlog #80) the orphan list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiledBlocker {
    pub owner: (String, String),
    pub owner_version: String,
    pub conflict: BlockerConflict,
}

/// Backlog #80: an unresolved blocker row whose owner has no display
/// entry anywhere -- a scan-collected owner absent from the graph, whose
/// block real still reports (`[blocks B]`, `Conflict: 1 block (1
/// unsatisfied)`, rc 1; A0 controls n1b/n6b). `file_blocker_conflicts`
/// used to drop these; now they ride out on `GraphResult::orphan_blockers`
/// and the renderer prints them in the trailing blocker group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanBlocker {
    pub owner_category: String,
    pub owner_package: String,
    pub owner_version: String,
    pub conflict: BlockerConflict,
}

/// Arguments of [`set_seed_holds_instance`].
pub(crate) struct SetSeedHoldsInstanceArgs<'a> {
    pub(crate) set_seed_atoms: &'a [String],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
}

/// #68 S1 (2026-09-16): an installed match whose slot is targeted by a
/// merge-bound entry at a **different** version is dropped before any
/// row is built -- real's package tracker discards an installed instance
/// superseded in its own slot, so `blocked_final` no longer holds it and
/// `_validate_blockers` uninstall-orders it away (the replacement merge
/// removes it); real prints no block row for it. This is the host
/// `@world` shape: `dev-build/gtk-doc-am`'s `<dev-util/gtk-doc-1.36.1`
/// against installed `gtk-doc-1.34.0` while 1.36.1 upgrades it in the
/// same run. The same-version `Reinstall` case is deliberately *not*
/// considered a replacement (it is merge-bound in the tracker), and a
/// replacement in a different slot leaves the installed row untouched.
///
/// Matches each `pending` blocker's target `category/package` against
/// both currently-installed candidates (`installed_candidates`, sub-slot
/// included -- see `Candidate::sub_slot`'s own doc comment) and this
/// graph's own resolved New/Upgrade set (`entries`, which may now hold
/// more than one slot for the same category/package -- every one of them
/// is a real candidate, not just the first), reusing
/// `portage_dep::match_from_list` exactly as every other atom-vs-candidate
/// check in this crate does (it ignores an atom's `blocker` field
/// entirely, so a `!`/`!!`-prefixed atom string matches candidates by
/// category/package/version/slot exactly like a normal one). A match
/// against the owner package's own resolved version is dropped
/// defensively (a package blocking itself is nonsensical, but cheap to
/// guard against). Returns `(owner_key, conflict)` pairs rather than
/// mutating `entries` directly, since `entries` is still needed
/// immutably to build the graph-resolved candidate list here.
///
/// `entries`' own contribution has no real sub-slot data at all --
/// `GraphEntry::slot` deliberately stays main-slot-only for now (a
/// documented, narrower scope cut than `Candidate::sub_slot`'s own repo/
/// vdb-backed fix), so it defaults sub-slot to the main slot itself,
/// the same fallback `split_slot` already uses for a plain (no `/`)
/// `SLOT` value -- "unknown" and "not yet split from an unslashed SLOT"
/// look identical here, and both mean "assume it matches the slot".
/// Backlog #289: real's complete graph adds the required sets' atoms as
/// `SetArg`s (`_expand_set_args(..., add_to_digraph=True)`), so an
/// installed instance a set atom *selects* is a digraph node with a parent
/// and a blocker that would unmerge it stays unresolved (`[blocks B]`,
/// `depgraph.py:9216-9224`). A set atom selects this instance only when it
/// matches it and no instance this run merges for the same cp (a bare
/// `bdprov` selects the new, higher slot instead): `bdprov:0` in world
/// holds the slot-0 instance a slot-1 update would otherwise uninstall.
/// Cut: only the set atoms themselves; an installed *holder* in the set
/// (a vdb edge of a package the walk never visited) is not modelled.
pub(crate) fn set_seed_holds_instance(args: SetSeedHoldsInstanceArgs<'_>) -> bool {
    let SetSeedHoldsInstanceArgs {
        set_seed_atoms,
        entries,
        category,
        package,
        version,
        slot,
        sub_slot,
    } = args;
    let candidate = format!("{category}/{package}-{version}:{slot}/{sub_slot}");
    let merged: Vec<String> = entries
        .iter()
        .filter(|e| e.category == category && e.package == package)
        .filter_map(|e| {
            let v = merge_bound_version(&e.outcome)?;
            Some(format!(
                "{category}/{package}-{v}:{}/{}",
                e.slot.as_deref().unwrap_or("0"),
                e.sub_slot.as_deref().or(e.slot.as_deref()).unwrap_or("0")
            ))
        })
        .collect();
    let matches = |atom: &str, cpv: &str| {
        portage_dep::match_from_list(atom, &[cpv]).is_some_and(|m| !m.is_empty())
    };
    set_seed_atoms.iter().any(|a| {
        let Some(parsed) = portage_dep::parse_atom(a) else {
            return false;
        };
        parsed.blocker == portage_dep::Blocker::None
            && parsed.category == category
            && parsed.package == package
            && matches(a, &candidate)
            && !merged.iter().any(|m| matches(a, m))
    })
}

/// [`resolve_blockers_with_seeds`] without set seeds (the solver bridge and
/// the unit tests carry none).
pub(crate) fn resolve_blockers(
    root: &Path,
    pending: &[PendingBlocker],
    entries: &[GraphEntry],
    blocker_retry_closure: &HashSet<(String, String)>,
    buildpkgonly: bool,
) -> Vec<FiledBlocker> {
    resolve_blockers_with_seeds(ResolveBlockersWithSeedsArgs {
        root,
        pending,
        entries,
        blocker_retry_closure,
        buildpkgonly,
        set_seed_atoms: &[],
    })
}

/// Arguments of [`graph_has_parent`].
pub(crate) struct GraphHasParentArgs<'a> {
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) exclude: &'a (String, String),
    pub(crate) entries: &'a [GraphEntry],
    /// #284: cp -> merge-bound slots this run. A merging parent rebinds a
    /// bare `:=` through its own walk to the resolved (merge-bound) slot,
    /// so such an edge no longer parents an installed instance in another
    /// slot -- the post-rebind reality real's per-instance digraph test
    /// (`depgraph.py:9218-9223`) sees. A pinned `:slot` (or built
    /// `:slot/sub=`) atom never rebinds and still counts.
    pub(crate) merge_slots: &'a HashMap<(String, String), HashSet<String>>,
}

/// #68 S3: real "we don't unmerge any package that has been pulled into
/// the graph" (`_validate_blockers`'s `digraph.contains(inst_pkg) and
/// digraph.parent_nodes(inst_pkg)`, `depgraph.py:9216-9224`, and the same
/// test for a nomerge parent at `:9195-9201`): is `category/package`'s
/// `version:slot/sub_slot` instance depended on by some **other** walked
/// graph entry? Every outcome counts (`AlreadyInstalled` included), which
/// is exactly the difference from the old vdb reverse scan: an installed
/// consumer the run never walked no longer marks the block unsolvable
/// (S0 cell d), while a walked one does (cell c). `exclude` is the
/// blocker owner itself. `DepEdge::atom` already carries the with-bdeps
/// key choice the entry was built with, so a build-time-only edge is only
/// present when build-time deps were walked.
pub(crate) fn graph_has_parent(args: GraphHasParentArgs<'_>) -> bool {
    let GraphHasParentArgs {
        category,
        package,
        version,
        slot,
        sub_slot,
        exclude,
        entries,
        merge_slots,
    } = args;
    let candidate = format!("{category}/{package}-{version}:{slot}/{sub_slot}");
    entries.iter().any(|e| {
        if (&e.category, &e.package) == (&exclude.0, &exclude.1) {
            return false;
        }
        if e.category == category && e.package == package {
            return false;
        }
        e.deps.iter().any(|d| {
            if d.category != category || d.package != package {
                return false;
            }
            // #284 rebind: this parent merges, its atom is a bare `:=`,
            // and the dep's cp merges in some other slot than the tested
            // instance -- the edge was re-pointed at the new slot, so it
            // is not a parent of this instance any more.
            if merge_bound_version(&e.outcome).is_some()
                && let Some(atom) = portage_dep::parse_atom(&d.atom)
                && atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
                && atom.slot.is_none()
                && merge_slots
                    .get(&(d.category.clone(), d.package.clone()))
                    .is_some_and(|slots| !slots.is_empty() && slots.iter().all(|s| s != slot))
            {
                return false;
            }
            portage_dep::match_from_list(&d.atom, &[candidate.as_str()])
                .is_some_and(|m| !m.is_empty())
        })
    })
}

/// #73: the owner-side half of real's "a digraph node with parents" test
/// (`_validate_blockers` step 4, `depgraph.py:9216-9224`). Real's
/// `_complete_graph` (`:8660-8745`) walks `_initial_arg_list` plus the
/// required sets appended as `SetArg`s and `_expand_set_args(...,
/// add_to_digraph=True)` (`:3271`) puts every set/argument node into the
/// digraph, so an installed owner selected from `@world`/`@selected`/
/// `@system` (or reached from one of those sets' installed dependency
/// walk) has at least one parent node. Portuale's `entries` only carry
/// the walked *dependency edges*, so that set/argument parent is
/// invisible to [`graph_has_parent`]; the retry closure
/// (`ResolveCtx::blocker_retry_closure`, the same forward-installed
/// closure the target arm consults) is exactly real's set-parent
/// membership.
///
/// The set is **cp-keyed** (`(category, package)`), not per-instance
/// (T10): real's `digraph.parent_nodes(inst_pkg)` is per-instance and
/// its own `_parent_atoms` edge records are cp-level too (see
/// `required_set_reachable_cps`'s doc comment -- "matched by `cat/pkg`").
/// For this arm that granularity is sufficient and exact: the owner is
/// the blocker's own single installed instance at
/// `PendingBlocker::owner_version`, and cp membership in this closure is
/// precisely what real's per-cp parent edge records.
///
/// #73 S0 (`TEST/findings/l0.md` "#73 S0") had real print `B`/rc 1 in
/// every shape -- owner in `@world` (g), owner as a plain `--oneshot`
/// argument (g2), owner reachable only through a world member's
/// installed dependency edge (g3), owner in `@system` (g4) -- and the
/// retry closure contained the owner in all four; `top_level_cps`
/// alone would have missed g3.
pub(crate) fn owner_set_parent(
    owner_key: &(String, String),
    blocker_retry_closure: &HashSet<(String, String)>,
) -> bool {
    blocker_retry_closure.contains(owner_key)
}

/// #68/#72 B3: the `GraphEntry` real `_validate_blockers` models as a
/// `Package(operation="uninstall")` task (`depgraph.py:9226-9246`): an
/// installed package a blocker resolves by removing. `version` is the
/// installed version being removed and `required_by` names the ordering
/// anchor(s) (`BlockerSatisfiedBy::Uninstall::anchor`): the blocker owner
/// for a merging owner, the merge-bound blocked instance for a nomerge
/// one. `merge_order::build_digraph` gives the removal its dedicated
/// after-the-anchor edge, and every merge/scheduling site treats it as a
/// non-merge (`PretendOutcome::Uninstall`'s own doc comment). `deps` stays
/// empty -- the removal has no dependency walk.
pub(crate) fn uninstall_entry(
    root: &Path,
    category: String,
    package: String,
    version: String,
    anchors: Vec<(String, String)>,
) -> GraphEntry {
    // The removed instance's own slot/sub-slot/repo, so `-pv` decorates
    // the row the way real's vardb-backed uninstall `Package` does
    // (`output.py:523-524`; repo falls back to `__unknown__`).
    let (slot, sub_slot) = read_vdb_slot(root, &category, &package, &version);
    let repo_name = installed_pkg_repo(root, &category, &package, &version);
    GraphEntry {
        discovery: 0,
        category,
        package,
        outcome: PretendOutcome::Uninstall { version },
        blockers: Vec::new(),
        slot: Some(slot),
        sub_slot: Some(sub_slot),
        repo_name: Some(repo_name),
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
        required_by: anchors,
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
}

/// Working set for `file_blocker_conflicts`: one removed `cpv`, its
/// ordering anchor(s) and the blocker rows that ride it when the owner
/// has no entry of its own (#77).
pub(crate) struct PendingRemoval {
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) anchors: Vec<(String, String)>,
    /// #77 A1: rows whose owner is absent from `entries` (the
    /// all-installed-packages scan's vardb `Package`). Real appends the
    /// solved blocker when the uninstall node is selected
    /// (`depgraph.py:10351-10358`), so the removal entry carries them and
    /// `blocker_row_disposition`'s `Uninstall` arm prints them inline
    /// right after its `[uninstall]` row.
    pub(crate) blockers: Vec<BlockerConflict>,
}

/// File each blocker conflict on its owner entry and append the
/// `Uninstall` removal entries (#68/#72 B3) the satisfied
/// `satisfied_by: Uninstall` rows describe.
///
/// A conflict whose owner is absent from `entries` keeps its row only
/// when it resolves by a removal: the removal entry is then the row's
/// home (#77 A1, the vdb-scan nomerge owner). Any other absent-owner row
/// (the solver-bridge path, or an unresolved B row) is still dropped --
/// real prints unresolved rows in its trailing group, but portuale has
/// no owner entry to hang them on yet (named cut).
///
/// One removal entry per removed `cpv`, carrying every ordering anchor
/// and row that pulled it in -- real creates one uninstall task per
/// `(parent, blocked)` pair in `_validate_blockers`, but the merge list's
/// uninstall node is the installed package itself.
pub(crate) fn file_blocker_conflicts(
    entries: &mut Vec<GraphEntry>,
    root: &Path,
    conflicts: Vec<FiledBlocker>,
    orphans: &mut Vec<OrphanBlocker>,
) {
    let mut removals: Vec<PendingRemoval> = Vec::new();
    for filed in conflicts {
        let FiledBlocker {
            owner: owner_key,
            owner_version,
            conflict,
        } = filed;
        let removal = match &conflict.satisfied_by {
            Some(BlockerSatisfiedBy::Uninstall { cpv, anchor }) => {
                Some((cpv.clone(), anchor.clone()))
            }
            _ => None,
        };
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| (e.category.clone(), e.package.clone()) == owner_key)
        {
            entry.blockers.push(conflict);
        } else if let Some((cpv, _)) = &removal {
            // #77 A1: no owner entry -- attach the row to its removal.
            let Some((category, package, version)) = split_cpv(cpv) else {
                continue;
            };
            if let Some(removal) = removals
                .iter_mut()
                .find(|r| r.category == category && r.package == package && r.version == version)
            {
                removal.blockers.push(conflict);
            } else {
                removals.push(PendingRemoval {
                    category,
                    package,
                    version,
                    anchors: Vec::new(),
                    blockers: vec![conflict],
                });
            }
        } else if conflict.unsolvable {
            // Backlog #80: no owner entry and no removal -- an
            // unresolved row of a scan-collected owner (A0 controls
            // n1b/n6b). Real still reports it (`[blocks B]`,
            // `Conflict: 1 block (1 unsatisfied)`, rc 1); dropping it
            // here is what made portuale print only the ebuild row.
            // A `Replacement` row can never land here: it always
            // carries `satisfied_by`, so it takes the removal branch
            // above (or an entry branch) instead.
            orphans.push(OrphanBlocker {
                owner_category: owner_key.0.clone(),
                owner_package: owner_key.1.clone(),
                owner_version,
                conflict,
            });
            continue;
        } else {
            continue;
        }
        let Some((cpv, anchor)) = removal else {
            continue;
        };
        let Some((category, package, version)) = split_cpv(&cpv) else {
            continue;
        };
        match removals
            .iter_mut()
            .find(|r| r.category == category && r.package == package && r.version == version)
        {
            Some(removal) => {
                if !removal.anchors.contains(&anchor) {
                    removal.anchors.push(anchor);
                }
            }
            None => removals.push(PendingRemoval {
                category,
                package,
                version,
                anchors: vec![anchor],
                blockers: Vec::new(),
            }),
        }
    }
    entries.extend(removals.into_iter().map(|r| {
        let mut entry = uninstall_entry(root, r.category, r.package, r.version, r.anchors);
        entry.blockers = r.blockers;
        entry
    }));
}

/// Arguments of [`collect_unwalked_installed_blockers`].
pub(crate) struct CollectUnwalkedInstalledBlockersArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) dynamic_deps: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) disj_constraints: &'a HashMap<(String, String), Vec<String>>,
    /// No `circular_dependency` map (backlogs #216/#221): this scan only
    /// processes installed packages whose cp was never walked (the
    /// walked-cp skip above), while both record sites key the map by
    /// merge-bound cycle-node cps (`find_hard_cycles`' merge-bound cpvs
    /// for #216's self-loops, `find_restart_cycles`' merge-bound indices
    /// for #221's rings) -- and every merge-bound cp is walked by
    /// definition. The demotion lookup therefore always misses here, so
    /// threading the live map would be exactly equivalent to the empty
    /// one real's per-parent `_select_atoms` would consult for a parent
    /// real never walks. (Real has no equivalent scan: it discovers
    /// installed blockers by walking installed packages in complete
    /// mode, which is `enqueue_dependencies`' job -- and that site also
    /// passes the empty map, per its own site decision below.)
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) pending_blockers: &'a mut Vec<PendingBlocker>,
    pub(crate) installed_meta_memo: &'a mut HashMap<(String, String, String, String), String>,
}

/// #77 A1: real `_validate_blockers`' collection step -- "Pull in blockers
/// from all installed packages that haven't already been pulled into the
/// depgraph, in order to ensure that they are respected (bug 128809)"
/// (`depgraph.py:8919-9078`). For every installed instance the walk never
/// reached, the recorded runtime deps (`Package._runtime_keys` =
/// `IDEPEND PDEPEND RDEPEND`, `depgraph.py:8923`) are flattened and every
/// blocker token becomes a [`PendingBlocker`] owned by that vardb
/// `Package` (`:9071-9078`): `operation == "nomerge"`, a distinct parent
/// from any same-cp merge entry. A0 (`TEST/findings/l0.md` "#77 A0")
/// captures the shape: installed `bparent-1.0` absent from the run's graph
/// still uninstall-orders itself against the merge-bound `blocked-1.5`.
///
/// The dynamic-deps view and USE evaluation are exactly
/// `enqueue_dependencies`' (the live ebuild metadata by default, the
/// vdb snapshot under `--dynamic-deps=n`, flattened against the recorded
/// vdb USE; `installed_dep_string` falls back to the raw record when the version
/// is no longer in the tree, real `_DynamicDepsNotApplicable`). The
/// disjunction closures are the same shared helpers, so a `||` branch is
/// resolved exactly as the walk would.
///
/// v1 approximations, both deliberate: an installed instance whose
/// `cat/pkg` already has an entry is skipped (real keys its reuse branch
/// `pkg_deps_added` per instance -- portuale's cp-keyed entries cannot
/// tell two slots apart here), and only the target root is scanned (real
/// iterates every tree; portuale's running-root split is the separate
/// `root_deps_running_root` concern).
pub(crate) fn collect_unwalked_installed_blockers(args: CollectUnwalkedInstalledBlockersArgs<'_>) {
    let CollectUnwalkedInstalledBlockersArgs {
        repos,
        root,
        config,
        dynamic_deps,
        ignore_built_slot_operator_deps,
        disj_constraints,
        root_deps_running_root,
        entries,
        pending_blockers,
        installed_meta_memo,
    } = args;
    // #83: with nothing merge-bound in `entries` the scan cannot
    // contribute a single row, so it is skipped outright. Every blocker
    // it produces is `owner_installed`, i.e. `owner_merging == false` in
    // `resolve_blockers`; the target candidate list is then the installed
    // matches alone (the merge-bound arm that extends it is empty), so
    // every match takes the `!owner_merging && installed_match &&
    // !merge_bound_match` early `continue` -- real's "an installed
    // parent's `blocked_initial` matches are ignored outright" cell.
    // The `Replacement` arm needs a same-slot merge-bound entry and the
    // `unsolvable` arm is `merge_bound_match && …`, so neither can fire
    // either. Exact shortcut, not an approximation.
    if !entries
        .iter()
        .any(|e| merge_bound_version(&e.outcome).is_some())
    {
        return;
    }
    // #83: the walked-cp test below ran a linear `entries` scan for every
    // installed package (O(installed x entries)); the set is the same
    // cp-keyed test, hoisted.
    let walked: HashSet<(&str, &str)> = entries
        .iter()
        .map(|e| (e.category.as_str(), e.package.as_str()))
        .collect();
    for pkg in all_installed_packages(root) {
        // Already walked (any outcome): the walk's own blockers are
        // pending, and the cp-keyed owner lookup in `resolve_blockers`
        // ties them to the right entry.
        if walked.contains(&(pkg.category.as_str(), pkg.package.as_str())) {
            continue;
        }
        // Backlog #86: the live view comes from the vdb-recorded repo
        // (`live_metadata_for_installed`), never a priority search.
        let live =
            live_metadata_for_installed(repos, root, &pkg.category, &pkg.package, &pkg.version);
        let use_flags = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
        let mut depstr = String::new();
        for key in ["IDEPEND", "PDEPEND", "RDEPEND"] {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            depstr.push_str(&installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category: &pkg.category,
                package: &pkg.package,
                version: &pkg.version,
                live: live.as_deref(),
                key,
                layer,
                memo: installed_meta_memo,
            }));
            depstr.push(' ');
        }
        // #83: this loop exists only to find blocker tokens, and a
        // blocker token always starts with `!` (`portage_dep::Blocker`).
        // `use_reduce` never introduces one -- it drops and expands
        // tokens, it does not rewrite them -- so a dep string with no `!`
        // anywhere cannot produce a blocker, and the reduce (with its
        // `dep_zapdeps` closures, the expensive half of the scan) can be
        // skipped outright. On this host that is 1804 of 2084 unwalked
        // installed packages; see `TEST/findings/l0.md` "#83".
        if !depstr.contains('!') {
            continue;
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let self_cp = (pkg.category.clone(), pkg.package.clone());
        // Track X Slice C (#242): the node key for the resolver map
        // (installed owner -- never a merge-bound cycle node, so the
        // target-root half, matching the pre-slice cp-keyed lookups).
        let self_node: RootCp = (false, pkg.category.clone(), pkg.package.clone());
        let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
            // Installed blocker scan: real `dep_check` passes
            // `eapi=None` for installed packages
            // (`dep_check.py:865-877`), whose defaults struct reads
            // `empty_groups_always_true = false`.
            false,
            &mut |atoms: &[String]| {
                disjunction_preference(
                    repos,
                    config,
                    root,
                    entries,
                    &self_node,
                    disj_constraints,
                    // Site decision (backlogs #216/#221 reconciliation):
                    // real keys `circular_dependency` by exact package
                    // node, and this scan only processes installed
                    // packages whose cp was never walked -- while the map
                    // is keyed by merge-bound cycle-node cps only -- so
                    // the demotion lookup always misses and the empty map
                    // is exactly faithful here. See the parameter doc.
                    &HashMap::new(),
                    root_deps_running_root,
                    atoms,
                    &[],
                    false,
                    entries,
                    &[],
                )
            },
            &mut |alts: &[Vec<String>]| {
                promote_tied_alternative(repos, config, root, entries, disj_constraints, alts, &[])
            },
        ) else {
            continue;
        };
        for tok in flat_deps {
            if tok == "||" {
                continue;
            }
            if let Some(dep_atom) = portage_dep::parse_atom(&tok)
                && dep_atom.blocker != portage_dep::Blocker::None
            {
                let duplicate = pending_blockers.iter().any(|p| {
                    p.owner_key == self_cp && p.owner_version == pkg.version && p.atom_str == tok
                });
                if !duplicate {
                    pending_blockers.push(PendingBlocker {
                        atom_str: tok,
                        strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                        // The scan reads IDEPEND/PDEPEND/RDEPEND only --
                        // never a build-time key (see the field docs).
                        buildtime: false,
                        target_category: dep_atom.category,
                        target_package: dep_atom.package,
                        owner_key: self_cp.clone(),
                        owner_version: pkg.version.clone(),
                        owner_merging: false,
                        owner_installed: true,
                    });
                }
            }
        }
    }
}
/// Arguments of [`resolve_blockers_with_seeds`].
pub(crate) struct ResolveBlockersWithSeedsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) pending: &'a [PendingBlocker],
    pub(crate) entries: &'a [GraphEntry],
    /// #68 S3 follow-up / #72 B2c: cps in real's required-set graph even
    /// when portuale's `entries` never carried their `deps` --
    /// `ResolveCtx::blocker_retry_closure`, the `@world`/`@selected`/
    /// `@system` forward-installed closure. Real seeds the required sets
    /// even for a plain single-atom resolve (`--implicit-system-deps`
    /// default y), so a blocked installed instance reachable from them is
    /// a digraph node with the set as parent. L0's `sys-apps/systemd-utils`
    /// probe needs it: installed `systemd` is in `@system`'s closure, real
    /// prints `B`/rc 1, portuale satisfied without it. B2c adds the retry
    /// half: when a block resolves by an uninstall, real re-runs the whole
    /// serialization in complete mode (`depgraph.py:10365-10383`), whose
    /// required-set walk is what makes B0's u4 (`--oneshot bparent`, world
    /// `{bconsumer}`) `B`/rc 1. A target outside the sets (S0 cell d)
    /// still sees no parents.
    ///
    /// #73 extends the same set test to the *owner* side of the nomerge
    /// arm (see `owner_set_parent`): a nomerge owner that is set-reachable
    /// is real's "digraph node with parents" even when portuale's
    /// `entries` never carried the edge (S0 cell g; g3 proves the
    /// closure-through-a-world-member half).
    pub(crate) blocker_retry_closure: &'a HashSet<(String, String)>,
    /// #143 S1: real's `--buildpkgonly` blocker gate
    /// (`depgraph.py::_validate_blockers`: `depends_on_order.clear()`
    /// for every blocker that is not hard-`!!` *and* buildtime, on top
    /// of `_add_pkg_dep_string` blanking RDEPEND/PDEPEND/IDEPEND for a
    /// non-built package). `true` drops soft and runtime-keyed blockers
    /// silently (real's irrelevant-blockers arm) and keeps only
    /// strong-buildtime ones, resolved by uninstall — the abort's rows.
    pub(crate) buildpkgonly: bool,
    /// #289: the set seed atoms (`@world`/`@selected` members) real adds as
    /// `SetArg` parents in its complete graph; see [`set_seed_holds_instance`].
    pub(crate) set_seed_atoms: &'a [String],
}

pub(crate) fn resolve_blockers_with_seeds(
    args: ResolveBlockersWithSeedsArgs<'_>,
) -> Vec<FiledBlocker> {
    let ResolveBlockersWithSeedsArgs {
        root,
        pending,
        entries,
        blocker_retry_closure,
        buildpkgonly,
        set_seed_atoms,
    } = args;
    let mut conflicts = Vec::new();
    // #284: cp -> slots with a merge-bound entry this run. Real's
    // per-instance test (`depgraph.py:9218-9223`) holds an uninstall
    // candidate only while it is a digraph node with parents; a cp the
    // run re-resolves into another slot leaves its old instance
    // parentless (the request parents the merge, and merging parents
    // rebind their live `:=` to the new slot -- see `graph_has_parent`).
    let merge_slots: HashMap<(String, String), HashSet<String>> = {
        let mut m: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for e in entries {
            if merge_bound_version(&e.outcome).is_some()
                && let Some(slot) = e.slot.clone()
            {
                m.entry((e.category.clone(), e.package.clone()))
                    .or_default()
                    .insert(slot);
            }
        }
        m
    };
    // #296: besides the set atoms themselves, real's complete graph holds an
    // instance through the live vdb edge of an installed package the
    // required sets reach (a nomerge node the walk never visited): its
    // recorded atoms join the seeds `set_seed_holds_instance` tests, built
    // lazily (one vdb scan, only when a blocker reaches that test) and
    // never from a cp this run merges (its edges are re-resolved).
    let hold_atoms: std::cell::OnceCell<Vec<String>> = std::cell::OnceCell::new();
    let hold_seeds = || -> &Vec<String> {
        hold_atoms.get_or_init(|| {
            let merge_bound: HashSet<(String, String)> = entries
                .iter()
                .filter(|e| merge_bound_version(&e.outcome).is_some())
                .map(|e| (e.category.clone(), e.package.clone()))
                .collect();
            let mut atoms: Vec<String> = set_seed_atoms.to_vec();
            for pkg in all_installed_packages(root) {
                let cp = (pkg.category.clone(), pkg.package.clone());
                if !blocker_retry_closure.contains(&cp) || merge_bound.contains(&cp) {
                    continue;
                }
                let use_flags =
                    read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
                for key in ["RDEPEND", "PDEPEND", "IDEPEND"] {
                    let dep = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key);
                    if let Some(found) = flat_dep_atoms(&dep, &use_flags) {
                        atoms.extend(found);
                    }
                }
            }
            atoms
        })
    };
    for pb in pending {
        // #143 S1: under `--buildpkgonly` a blocker that is not
        // strong-buildtime never pairs an uninstall (real's
        // `depends_on_order.clear()` → `_irrelevant_blockers`).
        if buildpkgonly && !(pb.strong && pb.buildtime) {
            continue;
        }
        let target_key = (pb.target_category.clone(), pb.target_package.clone());
        let installed_matches = installed_candidates(root, &pb.target_category, &pb.target_package);
        let mut candidates = installed_matches.clone();
        for entry in entries {
            if entry.category != pb.target_category || entry.package != pb.target_package {
                continue;
            }
            let version = match &entry.outcome {
                PretendOutcome::New { version } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            let (Some(version), Some(slot)) = (version, entry.slot.clone()) else {
                continue;
            };
            if !candidates
                .iter()
                .any(|(v, s, _ss)| *v == version && *s == slot)
            {
                candidates.push((version, slot.clone(), slot));
            }
        }
        let candidate_strs: Vec<String> = candidates
            .iter()
            .map(|(v, s, ss)| format!("{}/{}-{v}:{s}/{ss}", pb.target_category, pb.target_package))
            .collect();
        let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
        let Some(matched) = portage_dep::match_from_list(&pb.atom_str, &refs) else {
            continue;
        };
        let by_str: HashMap<&str, &(String, String, String)> = candidate_strs
            .iter()
            .map(String::as_str)
            .zip(candidates.iter())
            .collect();
        // The blocker atom's own `[use]` deps must actually match the
        // blocked package for the block to apply -- `match_from_list`
        // (which has only a cpv string, no USE) can't check them, so a
        // `mesa[-libglvnd(+)]` against a mesa that has since dropped the
        // `libglvnd` flag, or `shadow[su]` against a shadow built without
        // `su`, would spuriously fire. Real evaluates them against the
        // blocked pkg's recorded (installed) / effective (graphed) USE
        // and drops the satisfied no-op blocker from the merge list.
        let blocker_use_deps: Vec<portage_dep::UseDep> = portage_dep::parse_atom(&pb.atom_str)
            .and_then(|a| a.use_deps)
            .unwrap_or_default();
        let use_deps_apply = |version: &str| -> bool {
            if blocker_use_deps.is_empty() {
                return true;
            }
            let (enabled, iuse) = entries
                .iter()
                .find(|e| {
                    e.category == pb.target_category
                        && e.package == pb.target_package
                        && matches!(&e.outcome,
                            PretendOutcome::New { version: v }
                            | PretendOutcome::Reinstall { version: v, .. } if v == version)
                        || (e.category == pb.target_category
                            && e.package == pb.target_package
                            && matches!(&e.outcome,
                                PretendOutcome::Upgrade { to, .. }
                                | PretendOutcome::Downgrade { to, .. } if to == version))
                })
                .map(|e| {
                    let en: HashSet<String> = e
                        .use_flags_display
                        .iter()
                        .filter(|(_, on)| *on)
                        .map(|(f, _)| f.clone())
                        .collect();
                    let iu: HashSet<String> =
                        e.use_flags_display.iter().map(|(f, _)| f.clone()).collect();
                    (en, iu)
                })
                .unwrap_or_else(|| {
                    (
                        read_vdb_flag_set(
                            root,
                            &pb.target_category,
                            &pb.target_package,
                            version,
                            "USE",
                        ),
                        read_vdb_flag_set(
                            root,
                            &pb.target_category,
                            &pb.target_package,
                            version,
                            "IUSE",
                        ),
                    )
                });
            portage_dep::use_deps_satisfied(&blocker_use_deps, &iuse, &enabled)
        };
        // #68 S1: real's package tracker drops an installed instance that
        // a merge-bound entry replaces **in its own slot**
        // (`_package_tracker.add_pkg` discards the superseded installed
        // node), so the blocker atom never matches it there. The installed
        // match still exists in `blocked_initial`, but real
        // uninstall-orders it and, when it is not a graph node (the
        // replacement is), resolves the block to the satisfied `b`. This
        // is the host `@world` shape: `<gtk-doc-1.36.1` matching installed
        // `gtk-doc-1.34.0` while 1.36.1 upgrades it, and the B0b
        // p2c/p2d/p2e cells. #72 B1: the row is *kept* with
        // `satisfied_by: Replacement` instead of being dropped, because
        // real does display it when `_serialize_tasks` is stuck and
        // schedules the uninstall (`depgraph.py:9998`, `:10190`,
        // `:10351-10358`; `TEST/findings/l0.md` "#68/#72 B0b"). Whether
        // it displays is B2's mechanism predicate; until then the
        // renderer hides `Replacement` rows, exactly reproducing the old
        // dropped-row bytes.
        for m in matched {
            let Some((version, slot, sub_slot)) = by_str.get(m).copied() else {
                continue;
            };
            if !use_deps_apply(version) {
                continue;
            }
            let installed_match = installed_matches
                .iter()
                .any(|(v, s, _ss)| v == version && s == slot);
            let merge_bound_match = entries.iter().any(|e| {
                e.category == pb.target_category
                    && e.package == pb.target_package
                    && e.slot.as_deref() == Some(slot.as_str())
                    && merge_bound_version(&e.outcome).is_some_and(|v2| v2 == version)
            });
            let replaced_in_slot = installed_match
                && entries.iter().any(|e| {
                    e.category == pb.target_category
                        && e.package == pb.target_package
                        && e.slot.as_deref() == Some(slot.as_str())
                        && merge_bound_version(&e.outcome).is_some_and(|v2| v2 != version)
                });
            if replaced_in_slot {
                conflicts.push(FiledBlocker {
                    owner: pb.owner_key.clone(),
                    owner_version: pb.owner_version.clone(),
                    conflict: BlockerConflict {
                        atom_str: pb.atom_str.clone(),
                        strong: pb.strong,
                        matched_category: pb.target_category.clone(),
                        matched_package: pb.target_package.clone(),
                        matched_version: version.clone(),
                        unsolvable: false,
                        satisfied_by: Some(BlockerSatisfiedBy::Replacement {
                            cp: (pb.target_category.clone(), pb.target_package.clone()),
                            slot: slot.clone(),
                        }),
                        tree_scheduled_uninstall: false,
                    },
                });
                continue;
            }
            // #68 S2: real's `parent.operation` for this blocker owner --
            // a walked node's final outcome decides: a merge
            // (New/Upgrade/Downgrade/Reinstall) merges, an installed
            // (`AlreadyInstalled`) node is a `nomerge`. The producer bit
            // is only a fallback for an owner absent from `entries` (the
            // bridge path). #77 A1: a blocker from the all-installed scan
            // (`owner_installed`) is the vardb `Package` itself, always
            // `nomerge` -- a same-cp merge entry must not override it.
            // #142 S2 F2: the blocker must come from the merged
            // instance — a walked-but-unmerged candidate's blockers are
            // stale (real `_validate_blockers` only ever sees blockers
            // of graph nodes, so argv/walk order cannot move the
            // uninstall target). Match the owner entry by merged
            // version, not just cp; a merge-bound cp entry at another
            // version means the declaring instance lost. No merge-bound
            // cp entry at all keeps today's fallbacks (the nomerge and
            // bridge arms below own those blockers).
            let owner_entry = entries.iter().find(|e| {
                (e.category.as_str(), e.package.as_str())
                    == (pb.owner_key.0.as_str(), pb.owner_key.1.as_str())
                    && merge_bound_version(&e.outcome).is_some_and(|v| v == &pb.owner_version)
            });
            if !pb.owner_installed
                && owner_entry.is_none()
                && entries.iter().any(|e| {
                    (e.category.as_str(), e.package.as_str())
                        == (pb.owner_key.0.as_str(), pb.owner_key.1.as_str())
                        && merge_bound_version(&e.outcome).is_some()
                })
            {
                continue;
            }
            // Fallback for owners with no merge-bound entry: a walked
            // non-merge node (same cp, no merge version), else the
            // producer bit (the bridge path).
            let owner_entry = owner_entry.or_else(|| {
                entries.iter().find(|e| {
                    (e.category.as_str(), e.package.as_str())
                        == (pb.owner_key.0.as_str(), pb.owner_key.1.as_str())
                        && merge_bound_version(&e.outcome).is_none()
                })
            });
            let owner_merging = if pb.owner_installed {
                false
            } else {
                owner_entry
                    .map(|e| merge_bound_version(&e.outcome).is_some())
                    .unwrap_or(pb.owner_merging)
            };
            // The owner instance's own slot, from its entry when walked,
            // from the vdb when the scan produced it (the owner may have
            // no entry at all).
            let owner_slot = if pb.owner_installed {
                Some(read_vdb_slot(root, &pb.owner_key.0, &pb.owner_key.1, &pb.owner_version).0)
            } else {
                owner_entry.and_then(|e| e.slot.clone())
            };
            // Soft same-slot skip (cell h): real skips a soft blocker
            // whose target slot is the owner's own slot; a strong `!!`
            // blocker is exempt.
            if !pb.strong
                && target_key == pb.owner_key
                && owner_slot.as_deref() == Some(slot.as_str())
            {
                continue;
            }
            // Cell f: an installed parent's `blocked_initial` matches are
            // ignored outright (only blocked_final arms apply to it).
            if !owner_merging && installed_match && !merge_bound_match {
                continue;
            }
            // Real `_validate_blockers` step 4 (depgraph.py:9216-9246):
            // the tracker match is uninstall-ordered; it stays unresolved
            // only when the instance that must be unmerged is a walked
            // digraph node with parents. #68 S3 replaces the old vdb
            // reverse scan with exactly that graph test -- an installed
            // consumer the run never walked does not block the uninstall
            // (S0 cell d), while a walked one does (cell c).
            let unsolvable = if owner_merging {
                // A merge-bound match with a merging parent is unresolved
                // outright (cells b, e).
                //
                // #284: real's per-instance test (`depgraph.py:9218-9223`)
                // holds the old instance only while some graph node that
                // remains (after the rebind) still has it as a parent.
                // The cp-keyed retry closure over-approximates that: a
                // directly-requested cp is always a closure member, but
                // the request parents the merge, not the old instance.
                // So the closure veto lifts when the run re-resolves this
                // cp into another slot -- the old instance is parentless
                // then (case 1: nobody pulls `bdprov-1`; case 2: `bdcons`
                // rebinds its live `:=` to the new slot, which
                // `graph_has_parent` models by skipping the rebound edge).
                // A cp that does not move keeps the veto (the request or
                // set parents the staying instance, e.g. L0 systemd).
                let cp_moves_slot = merge_slots
                    .get(&target_key)
                    .is_some_and(|slots| slots.iter().any(|s| s != slot));
                merge_bound_match
                    || (installed_match
                        && (graph_has_parent(GraphHasParentArgs {
                            category: &pb.target_category,
                            package: &pb.target_package,
                            version,
                            slot,
                            sub_slot,
                            exclude: &pb.owner_key,
                            entries,
                            merge_slots: &merge_slots,
                        }) || (blocker_retry_closure.contains(&target_key) && !cp_moves_slot)
                            || set_seed_holds_instance(SetSeedHoldsInstanceArgs {
                                set_seed_atoms: hold_seeds(),
                                entries,
                                category: &pb.target_category,
                                package: &pb.target_package,
                                version,
                                slot,
                                sub_slot,
                            })))
            } else {
                // A nomerge parent is uninstall-ordered; it is unresolved
                // when the *owner* is a digraph node with parents
                // (`depgraph.py:9216-9224`): either a walked entry
                // depends on it (`graph_has_parent`, cell g's
                // consumer-walk shape) or it is in the required sets'
                // own parent membership (`owner_set_parent`, #73 S0 cells
                // g/g3/g4). Parenthesised: the set term must not be
                // reachable when `merge_bound_match` is false (T11; cell
                // f). #77 A1: a scan-sourced owner has no entry, so its
                // instance slot comes from the vdb.
                let owner_has_parent = if pb.owner_installed {
                    let (slot, sub_slot) =
                        read_vdb_slot(root, &pb.owner_key.0, &pb.owner_key.1, &pb.owner_version);
                    graph_has_parent(GraphHasParentArgs {
                        category: &pb.owner_key.0,
                        package: &pb.owner_key.1,
                        version: &pb.owner_version,
                        slot: &slot,
                        sub_slot: &sub_slot,
                        exclude: &pb.owner_key,
                        entries,
                        merge_slots: &merge_slots,
                    })
                } else {
                    owner_entry.is_some_and(|e| {
                        graph_has_parent(GraphHasParentArgs {
                            category: &pb.owner_key.0,
                            package: &pb.owner_key.1,
                            version: &pb.owner_version,
                            slot: e.slot.as_deref().unwrap_or_default(),
                            sub_slot: e.sub_slot.as_deref().unwrap_or_default(),
                            exclude: &pb.owner_key,
                            entries,
                            merge_slots: &merge_slots,
                        })
                    })
                };
                merge_bound_match
                    && (owner_has_parent || owner_set_parent(&pb.owner_key, blocker_retry_closure))
            };
            conflicts.push(FiledBlocker {
                owner: pb.owner_key.clone(),
                owner_version: pb.owner_version.clone(),
                conflict: BlockerConflict {
                    atom_str: pb.atom_str.clone(),
                    strong: pb.strong,
                    matched_category: pb.target_category.clone(),
                    matched_package: pb.target_package.clone(),
                    matched_version: version.clone(),
                    // #72 B3: a satisfied *merging-owner* row is resolved
                    // by removing the matched installed instance
                    // (`_validate_blockers`' `operation="uninstall"`
                    // task, `depgraph.py:9226-9246`), so it names that
                    // cpv; `run_pass`/the bridge turn the tag into the
                    // removal `GraphEntry`. #77 A1: the nomerge-owner
                    // arm's satisfied row is resolved by removing the
                    // **owner** (real `:9204-9206`,
                    // `depends_on_order.add((parent, pkg))`, own cpv from
                    // `owner_version`), and real orders the removal after
                    // the merge-bound blocked instance (`inst_task`,
                    // `:9240-9242`) -- hence the anchor. The all-installed
                    // scan that reaches this arm for an unwalked owner is
                    // `collect_unwalked_installed_blockers`; the capture
                    // is `TEST/findings/l0.md` "#77 A0" (n1).
                    satisfied_by: if unsolvable {
                        None
                    } else if owner_merging {
                        Some(BlockerSatisfiedBy::Uninstall {
                            cpv: format!("{}/{}-{version}", pb.target_category, pb.target_package),
                            anchor: pb.owner_key.clone(),
                        })
                    } else {
                        Some(BlockerSatisfiedBy::Uninstall {
                            cpv: format!(
                                "{}/{}-{}",
                                pb.owner_key.0, pb.owner_key.1, pb.owner_version
                            ),
                            anchor: (pb.target_category.clone(), pb.target_package.clone()),
                        })
                    },
                    unsolvable,
                    tree_scheduled_uninstall: false,
                },
            });
        }
    }
    conflicts
}
