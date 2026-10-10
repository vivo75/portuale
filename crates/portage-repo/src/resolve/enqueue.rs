//! Queueing one package's dependencies.

use crate::*;

/// The installed or newly selected package whose dependencies
/// [`enqueue_dependencies`] queues.
#[derive(Clone, Copy)]
pub(crate) struct DepOwner<'a> {
    /// Track X Slice B (#242): the walk's target root, so an installed
    /// parent's own deps resolve per group like every other owner (real
    /// `_add_pkg_deps` attributes the owner to its root first). `root`
    /// stays this parent's own vdb root for every metadata read.
    pub target_root: &'a Path,
    pub category: &'a str,
    pub package: &'a str,
    pub version: &'a str,
    pub child_depth: u32,
}

/// The emerge options [`enqueue_dependencies`] consults.
#[derive(Clone, Copy)]
pub(crate) struct EnqueueOptions<'a> {
    pub dynamic_deps: bool,
    pub ignore_built_slot_operator_deps: bool,
    pub with_bdeps: bool,
    pub root_deps_running_root: Option<&'a Path>,
    /// No `circular_dependency` map (backlogs #216/#221 reconciliation):
    /// this deep-walk's parents are `AlreadyInstalled` entries only (see
    /// the call site -- merge-bound upgrades go through the main
    /// New/Upgrade loop), while recorded nodes are merge-bound-only on
    /// both sides: real records serialize-digraph nodes that stranded on
    /// unmet buildtime deps (an installed node is always a leaf), keyed
    /// by node identity (`Package.__hash__ = Task.__hash__`), and
    /// portuale's two record sites only link merge-bound entries. Real's
    /// lookup for such a parent therefore always misses -- and portuale's
    /// cp-keying would over-demote on an installed-v1/merging-v2 cp
    /// collision real never exhibits -- so the empty map is exactly
    /// faithful here, like the unwalked-installed scan above.
    /// Backlog #90 (S1): `--update`, for the downgrade-guard's
    /// queued-update lookahead (same feed as the main walk's
    /// `ctx.update` at its own `disjunction_preference` call).
    pub update: bool,
}

/// The pass state [`enqueue_dependencies`] appends to.
pub(crate) struct EnqueueSinks<'a> {
    pub queue: &'a mut VecDeque<QueueItem>,
    pub pending_blockers: &'a mut Vec<PendingBlocker>,
    pub entries: &'a mut Vec<GraphEntry>,
    pub root_deps_build_seen: &'a mut HashSet<(String, String)>,
    /// Per-pass `installed_dep_string` memo (see its own doc comment):
    /// keyed `(category, package, version, key)`, Effective results only.
    pub installed_meta_memo: &'a mut HashMap<(String, String, String, String), String>,
    /// A4 (#27): this installed package's own flattened non-blocker atoms
    /// become slot-conflict pullers, exactly like the main
    /// New/Upgrade/Reinstall loop's own flat-deps recording. Without it an
    /// installed parent's built `:S/SS=` atom never reaches
    /// `SlotConflict::parents`, so `slot_conflict_need_rebuild` can never
    /// see the parent real's `_parent_atoms` would show.
    pub slot_pullers: &'a mut SlotPullers,
}

/// Reads `category/package-version`'s own DEPEND+RDEPEND+BDEPEND+PDEPEND+
/// IDEPEND metadata (from whichever repo actually carries this exact
/// version) and enqueues each flattened dependency token -- into
/// `pending_blockers` if it's a blocker atom, `queue` (at `child_depth`)
/// otherwise -- exactly the same lookup-and-flatten steps
/// `resolve_pretend_graph`'s own main loop already takes for a freshly
/// resolved New/Upgrade/Reinstall candidate, factored out here so
/// `--deep`'s AlreadyInstalled walk (see `Deep`) can reuse it without
/// duplicating that logic. Silently does nothing if `version` can't be
/// found in any repo, or its md5-cache entry can't be read -- matching
/// the same tolerance the main loop already has for those cases.
///
/// `dynamic_deps` (real `--dynamic-deps`, `create_depgraph_params.py:
/// 116-123`, ON by default for a source install): when `true`, an
/// AlreadyInstalled package's dependency walk uses the repo's **current**
/// ebuild metadata plus the vdb's own built `:=` atoms appended back on,
/// EAPI-gated exactly like real (`FakeVartree._apply_dynamic_deps`;
/// see `installed_dep_string`'s own doc comment), flattened against its
/// built (`vdb/USE`) flags.
/// When `false` (`--dynamic-deps=n`), it reads the package's own
/// vdb-recorded `*DEPEND` snapshot alone -- real portage's own
/// installed-time metadata. The two differ when the repo's copy of that
/// exact version's ebuild changed since it was installed.
///
/// `with_bdeps` (real `--with-bdeps`, see `resolve_pretend_graph`'s own
/// doc comment for the full grounding): when `false`, DEPEND and BDEPEND
/// are left out of the dep-key list entirely, so their own tokens are
/// never parsed, flattened, or queued -- RDEPEND/PDEPEND/IDEPEND are
/// unaffected. This is the one place that distinction is ever made:
/// `resolve_pretend_graph`'s own main loop (the New/Upgrade/Reinstall
/// path, which this function's own doc comment above says it mirrors)
/// deliberately does *not* take a `with_bdeps` parameter at all, since
/// real portage only ever drops build-time deps for an *already-built*
/// package -- see `resolve_pretend_graph`'s own doc comment.
///
/// `root_deps_running_root` (real `--root-deps`, see
/// `running_root_satisfies_atom`'s own doc comment for the full real
/// `ESYSROOT`-vs-`ROOT` grounding, and `root_deps_satisfied_atoms`'s own
/// doc comment for the shared implementation and its documented scope
/// cut): `None` for every pre-existing call site/test, and a strict
/// no-op when `None` -- the combined `dep_keys`/`depstr`/`tokens`/
/// `flat_deps` pipeline below is completely unchanged from before this
/// parameter existed, same order, same closure, same error behavior.
/// When `Some`, any already-flattened plain atom in `flat_deps` that
/// `root_deps_satisfied_atoms` reports as running-root-satisfied is
/// dropped from the queue entirely (real portage's own "no separate
/// graph node needed for an already-satisfied dep").
pub(crate) fn enqueue_dependencies(
    source: &SelectSource,
    owner: &DepOwner,
    opts: &EnqueueOptions,
    sinks: EnqueueSinks,
    // The `'backtrack` loop's accumulated per-`cat/pkg` `runtime_pkg_mask`
    // (`slot_constraints`), consulted by this walk's `||` branch
    // selection the same way the main New/Upgrade loop's is -- empty on
    // the first pass, so a strict no-op there.
    disj_constraints: &HashMap<(String, String), Vec<String>>,
) {
    let SelectSource {
        repos,
        root,
        config,
        local_binpkg,
    } = *source;
    let DepOwner {
        target_root,
        category,
        package,
        version,
        child_depth,
    } = *owner;
    let owner_key = (category.to_string(), package.to_string());
    let owner_version = version.to_string();
    let EnqueueOptions {
        dynamic_deps,
        ignore_built_slot_operator_deps,
        with_bdeps,
        root_deps_running_root,
        update,
    } = *opts;
    let EnqueueSinks {
        queue,
        pending_blockers,
        entries,
        root_deps_build_seen,
        installed_meta_memo,
        slot_pullers,
    } = sinks;
    // Backlog #86: the walk reads the vdb-recorded repo's current
    // metadata (never a priority search); `None` when the version is
    // gone there, same tolerance as the old search miss.
    let Some(metadata) = live_metadata_for_installed(repos, root, category, package, version)
    else {
        return;
    };

    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };

    // `--dynamic-deps` (default) walks the repo's *current* ebuild
    // `*DEPEND` strings (real `FakeVartree._apply_dynamic_deps`, with
    // the vdb built-`:=` append EAPI-gated exactly like real -- see
    // `installed_dep_string`'s own doc comment; EAPI-less fixture vdbs
    // read live, as before); `--dynamic-deps=n` walks the vdb's own
    // installed-time `*DEPEND` snapshot. Either way the USE conditionals
    // in those strings are evaluated against the package's *installed*
    // recorded USE (`vdb/USE`), never a fresh profile recompute -- real
    // `_pkg_use_enabled(pkg)` returns `pkg._metadata["USE"]` for a
    // `built` package, and `enqueue_dependencies`'s only caller is the
    // `--deep` recursion into an AlreadyInstalled package (never a
    // merge-bound one), so this is always a `built` package. Using the
    // effective profile USE here spuriously pulled `flag?( … )` deps for
    // a flag the installed build never had -- e.g. `-D @world` on a
    // system with `app-text/xmlto` built `-text` walked its current
    // `text? ( || ( virtual/w3m … ) )` and dragged in w3m + ~40 of its
    // own deps that real never touches.
    let use_flags = read_vdb_flag_set(root, category, package, version, "USE");
    // Track X Slice B (#242): per-key strings for the group-root tags
    // below (the merged `depstr` alone cannot recover key provenance).
    let mut keyed_depstr: HashMap<String, String> = HashMap::new();
    let depstr = {
        let mut depstr = String::new();
        for dep_key in dep_keys {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            let s = installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category,
                package,
                version,
                live: Some(&metadata),
                key: dep_key,
                layer,
                memo: installed_meta_memo,
            });
            depstr.push_str(&s);
            depstr.push(' ');
            keyed_depstr.insert(dep_key.to_string(), s);
        }
        depstr
    };
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    // Track X Slice B (#242): this installed owner's own per-group roots
    // (an installed running-root entry's build groups resolve running).
    // Inactive on every same-root run (strict no-op).
    let cross_active =
        root_deps_running_root.is_some_and(|rr| rr.as_os_str() != target_root.as_os_str());
    let owner_is_running = root.as_os_str() != target_root.as_os_str();
    let running_tags: HashSet<String> = if cross_active {
        dep_running_tags(&keyed_depstr, &use_flags, owner_is_running)
    } else {
        Default::default()
    };
    let group_running = |atoms: &[String]| -> bool {
        cross_active && atoms.iter().all(|a| running_tags.contains(a))
    };
    // #143 S1: real's `blocker.priority.buildtime` for this walk's own
    // blockers — flatten just the build-time keys the same way, so a
    // blocker token's provenance survives the combined-key flatten
    // (`depstr` above merges every key). The same `flatten_keys` idea
    // as the main walk's `buildtime_atoms`. Empty without `with_bdeps`,
    // when no build-time key is walked at all.
    let buildtime_tokens: HashSet<String> = if with_bdeps {
        let mut joined = String::new();
        for dep_key in ["DEPEND", "BDEPEND"] {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            joined.push_str(&installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category,
                package,
                version,
                live: Some(&metadata),
                key: dep_key,
                layer,
                memo: installed_meta_memo,
            }));
            joined.push(' ');
        }
        let toks: Vec<String> = joined.split_whitespace().map(String::from).collect();
        portage_use_reduce::use_reduce_flat(
            &toks,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .map(|v| v.into_iter().filter(|t| t != "||").collect())
        .unwrap_or_default()
    } else {
        HashSet::new()
    };
    // Real `--root-deps` branch-selection feed-in -- see the main
    // New/Upgrade/Reinstall loop's own identical fix, above, for the
    // full grounding (this is `resolve_pretend_graph`'s own
    // `--deep`/AlreadyInstalled-recursion counterpart to it).
    // Backlog #216: no self-exclusion on the first pass (see the main
    // walk's note above) -- the shared `disjunction_preference` check
    // applies here unchanged.
    // Track X Slice C (#242): the node key for the resolver map
    // (installed deep-walk owner -- never a merge-bound cycle node, so
    // the target-root half, matching the pre-slice cp-keyed lookups).
    // (The bare-cp `self_cp` this replaces also fed nothing else in this
    // walk, so it is gone rather than renamed.)
    let self_node: RootCp = (false, category.to_string(), package.to_string());
    // The whole `dep_zapdeps` choice-bin classification lives in the
    // single shared `disjunction_preference` helper -- see its own doc
    // comment (2026-09-11 review F1/F2: this closure used to be its own,
    // unfixed copy that never got the fine `unsat_use_*` bins the main
    // New/Upgrade walk's closure did).
    // Backlog #90 (S1): same drain-state snapshot as the main walk --
    // `queue` here is `enqueue_dependencies`' own `&mut VecDeque`.
    let queued: Vec<QueueItem> = queue.iter().cloned().collect();
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens: &tokens,
            uselist: &use_flags,
            mode: portage_use_reduce::MatchMode::Normal, // Installed `--deep` recursion: real `dep_check` passes
            // `eapi=None` for installed packages (`dep_check.py:865-877`),
            // whose defaults struct reads `empty_groups_always_true =
            // false`.
            empty_groups_always_true: false,
            tie_break: &mut |alts: &[Vec<String>]| {
                let grp_running = cross_active
                    && alts
                        .iter()
                        .flat_map(|v| v.iter())
                        .next()
                        .is_some_and(|a| running_tags.contains(a));
                let grp_root: &Path = if grp_running {
                    root_deps_running_root.expect("running-root group without a running root")
                } else {
                    root
                };
                let scoped_entries: Option<Vec<GraphEntry>> = cross_active.then(|| {
                    entries
                        .iter()
                        .filter(|e| e.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(entries);
                let scoped_queued: Option<Vec<QueueItem>> = cross_active.then(|| {
                    queued
                        .iter()
                        .filter(|q| q.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
                promote_tied_alternative(PromoteTiedAlternativeArgs {
                    repos,
                    config,
                    root: grp_root,
                    entries: entries_view,
                    constraints: disj_constraints,
                    alts,
                    queued: queued_view,
                })
            },
        },
        &mut |atoms: &[String]| {
            // Track X Slice B (#242): same root scoping as the main
            // walk's closures -- the group's own installed db and
            // same-root graph. (`root` here is already this installed
            // owner's own vdb root; only the entry/queue views narrow.)
            let grp_running = group_running(atoms);
            let grp_root: &Path = if grp_running {
                root_deps_running_root.expect("running-root group without a running root")
            } else {
                root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_active.then(|| {
                entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_active.then(|| {
                queued
                    .iter()
                    .filter(|q| q.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
            disjunction_preference(DisjunctionPreferenceArgs {
                repos,
                config,
                root: grp_root,
                entries: entries_view,
                self_cp: &self_node,
                constraints: disj_constraints, // Site decision (backlogs #216/#221 reconciliation): the
                // empty map. See the parameter doc: this deep-walk's
                // parents are `AlreadyInstalled` entries only, which
                // real's node-identity-keyed lookup always misses, so
                // threading the live map would over-demote on
                // installed-v1/merging-v2 cp collisions real never
                // exhibits.
                circular_dependency: &HashMap::new(),
                root_deps_running_root,
                atoms,
                queued: queued_view,
                update,
                gate_entries: entries,
                gate_queued: &queued,
            })
        },
    ) else {
        return;
    };

    let root_deps_satisfied: HashSet<String> = if with_bdeps {
        root_deps_running_root
            .map(|root| {
                root_deps_satisfied_atoms(RootDepsSatisfiedAtomsArgs {
                    metadata: &metadata,
                    use_flags: &use_flags,
                    repos,
                    config,
                    running_root: root,
                    dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"], // Installed walk: real `dep_check` passes
                    // `eapi=None` for installed packages
                    // (`dep_check.py:865-877`), whose defaults struct
                    // reads `empty_groups_always_true = false`.
                    empty_groups_always_true: false,
                })
            })
            .unwrap_or_default()
    } else {
        HashSet::new()
    };

    // Real "recursively pull in and build new packages against the
    // running root" -- the `--deep`/`AlreadyInstalled`-recursion
    // counterpart to the main New/Upgrade/Reinstall loop's own identical
    // step, above (see `resolve_root_deps_build_entries`'s own doc
    // comment, and, for why `flat_deps` below must also exclude this
    // exact set, that same step's own comment). Gated on `with_bdeps`
    // the same way `root_deps_satisfied` just above already is --
    // `DEPEND`/`BDEPEND` aren't even in `tokens` at all when it's
    // `false`. Kept as a `Vec` for deterministic entry order.
    // Track X Slice B (#242): flag-on coinciding-roots only (like the
    // main walk, the unified per-group walk owns every cross-root atom).
    let root_deps_split = root_deps_running_root.filter(|_| !cross_active);
    let root_deps_unsatisfied: Vec<String> = if with_bdeps {
        root_deps_running_root
            .map(|root| {
                unsatisfied_root_deps_atoms(UnsatisfiedRootDepsAtomsArgs {
                    metadata: &metadata,
                    use_flags: &use_flags,
                    repos,
                    config,
                    running_root: root,
                    dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"], // Installed walk, same real `eapi=None` default as
                    // `root_deps_satisfied` just above.
                    empty_groups_always_true: false,
                })
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(running_root) = root_deps_split {
        for atom_str in &root_deps_unsatisfied {
            entries.extend(resolve_root_deps_build_entries(
                ResolveRootDepsBuildEntriesArgs {
                    repos,
                    running_root,
                    atom_str,
                    config,
                    owner: owner_key.clone(),
                    seen: root_deps_build_seen,
                    local_binpkg,
                },
            ));
        }
    }

    // Backlog #90 (S1): same plains-reversed + `||`-last split as the
    // main walk's `enqueue_flat_deps` -- one deplist, one stack in
    // real, walked or not.
    let universe = or_group_universe(&tokens);
    let (mut disj, mut plains): (Vec<String>, Vec<String>) = flat_deps
        .into_iter()
        .partition(|t| t != "||" && universe.contains(t));
    plains.reverse();
    disj.reverse();
    for (tok, from_disjunction) in plains
        .into_iter()
        .map(|t| (t, false))
        .chain(disj.into_iter().map(|t| (t, true)))
    {
        if tok == "||" {
            continue;
        }
        // #132: real evaluates every token's PMS 8.3.4 conditional
        // use-deps (`flag?`/`!flag?`/`flag=`/`!flag=`) against the
        // *parent's* USE before the token is ever classified or queued
        // -- `use_reduce(..., token_class=Atom)`'s own per-token
        // `token.evaluate_conditionals(uselist)` step, with `uselist` =
        // `_pkg_use_enabled(pkg)`, which for a built package is its vdb
        // `USE`. `use_flags` above is exactly that set (the same one
        // this function already flattens the dep string with). Without
        // this an installed parent's `foo[bar=]` reached the candidate
        // filters verbatim, where a conditional form imposes no state
        // constraint at all (`use_deps_satisfied`), so every such dep
        // was vacuously satisfied by whatever happened to be installed
        // (#111 S0: installed qtdeclarative built `-wayland` accepted
        // installed qtbase built `+wayland`, rc 0, where real aborts).
        // Applied before the blocker split, like `enqueue_flat_deps`
        // and like real -- a blocker atom can carry use-deps too.
        //
        // `raw` is kept for the `unevaluated` queue field below only.
        // (#132 left the two `root_deps_*` membership tests keyed on
        // `raw` because those sets were raw-keyed; #133 evaluates the
        // producers too, so both sides are evaluated-keyed now and the
        // tests use `tok`.)
        let raw = tok;
        let evaluated = portage_dep::evaluate_atom_conditionals(&raw, &use_flags)
            .unwrap_or_else(|| raw.clone());
        let unevaluated = if evaluated != raw {
            Some(raw.clone())
        } else {
            None
        };
        let tok = evaluated;
        if let Some(dep_atom) = portage_dep::parse_atom(&tok)
            && dep_atom.blocker != portage_dep::Blocker::None
        {
            // #143 S1: provenance against the build-time-keys flatten
            // above (bound before `tok` moves into `atom_str`).
            let buildtime = buildtime_tokens.contains(&tok);
            pending_blockers.push(PendingBlocker {
                atom_str: tok,
                strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                buildtime,
                target_category: dep_atom.category,
                target_package: dep_atom.package,
                owner_key: owner_key.clone(),
                owner_version: owner_version.clone(),
                owner_merging: false,
                // A walked installed owner (`enqueue_dependencies`), not
                // the all-vdb scan producer.
                owner_installed: false,
            });
            continue;
        }
        if root_deps_split.is_some() && root_deps_satisfied.contains(&tok) {
            // Real "no separate graph node needed for an
            // already-satisfied dep": ESYSROOT (here, the real running
            // root) already has it.
            continue;
        }
        if root_deps_split.is_some() && root_deps_unsatisfied.contains(&tok) {
            // Real `DEPEND`/`BDEPEND` never targets `ROOT`/`ESYSROOT` at
            // all under portuale's own established `--root-deps`
            // simplification -- already handled above instead (either a
            // new `targets_running_root` entry, or silently dropped on
            // failure/cycle -- see `resolve_root_deps_build_entries`'s own
            // doc comment).
            continue;
        }
        // #74 S1: a dependency the installed parent itself satisfies is
        // the graph node real's `_add_dep` -> `_select_package` returns
        // from `find_existing_node` (`depgraph.py:8183-8203`) -- no
        // New/Upgrade/Reinstall decision is ever made for it (the
        // `_reinstall_for_flags` comparison lives in the candidate loop
        // *below* that early return, and `_add_dep` then drops the edge
        // at `:3765` unless it is an unsatisfied buildtime dep).
        // `dev-lang/rust-1.94.1`'s own current-ebuild BDEPEND is
        // `... || ( dev-lang/rust-bin:1.94.1 dev-lang/rust:1.94.1 ... )`;
        // without this, `--newuse` resolved the self alternative to a
        // Reinstall whose REQUIRED_USE re-check (see the
        // `already_installed_outcome` gate at the queue loop) failed the
        // whole run. A self atom on a *different* slot
        // (`dev-lang/rust:stable`, the RDEPEND form) is not satisfied by
        // this instance and still queues normally.
        if let Some(dep_atom) = portage_dep::parse_atom(&tok)
            && (dep_atom.category, dep_atom.package) == owner_key
            && best_installed_matching(root, &tok, config)
                .is_some_and(|version| version == owner_version)
        {
            continue;
        }
        // The `--deep` walk only ever reaches an already-installed
        // package's deps, which are satisfied edges -- never a `hard`
        // build-time cycle contributor (real `_ignore_runtime` would drop
        // them anyway).
        //
        // A4 (#27): record the installed parent as a puller of this
        // token before queueing it -- the main New/Upgrade/Reinstall loop
        // does the same for its own flat deps; without this an installed
        // parent's built `:S/SS=` atom never reaches
        // `SlotConflict::parents` and the `need_rebuild` scan stays
        // dormant. `tok` names a non-blocker `cat/pkg` here (blockers
        // `continue`d above).
        if let Some(dep_atom) = portage_dep::parse_atom(&tok) {
            slot_pullers
                .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                .or_default()
                .push((
                    owner_key.0.clone(),
                    owner_key.1.clone(),
                    owner_version.clone(),
                    tok.clone(),
                ));
        }
        queue.push_back(QueueItem {
            atom: tok,
            depth: child_depth,
            owner: Some(owner_key.clone()),
            // Track X Slice C (#242): the installed owner's own root
            // flag (first match -- this deep walk's parents are
            // `AlreadyInstalled` entries whose edges are never
            // build-time-hard, so owner-half imprecision cannot forge a
            // hard cycle; the child half above stays exact).
            owner_targets_running_root: entries
                .iter()
                .find(|e| e.category == owner_key.0 && e.package == owner_key.1)
                .is_some_and(|e| e.targets_running_root),
            unevaluated,
            // Track X Slice B (#242): the per-group root travels with the
            // atom -- keyed on the raw token like the main walk (the
            // evaluated `tok` may have shed `[use]` conditionals the tag
            // sets still carry).
            targets_running_root: running_tags.contains(&raw),
            buildtime_hard: false,
            from_disjunction,
        });
    }
}
