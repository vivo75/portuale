//! `--root-deps` build entries for the running root.

use crate::*;

/// Real `--root-deps`'s own `DEPEND`/`BDEPEND`-vs-`ESYSROOT` distinction
/// (see `running_root_satisfies_atom`'s own doc comment for the full real
/// grounding), factored out so both real dep-walk sites in this file
/// (the main New/Upgrade/Reinstall flatten and `enqueue_dependencies`'s
/// own AlreadyInstalled-recursion path) share one implementation rather
/// than drifting apart. Reads `metadata`'s own `dep_keys` (real
/// `["DEPEND", "BDEPEND", "IDEPEND"]` at both ordinary dep-walk sites --
/// `DEPEND`/`BDEPEND` are the classic `ESYSROOT` build deps, and
/// `IDEPEND` *always* targets the running root for every package, not
/// just recursed build entries -- `depgraph.py:4247-4252`),
/// flattens them the exact same way (`use_flags`/`repos`/`config`) the
/// caller already flattened its own combined dep string with, *except*
/// for one deliberate branch-selection difference: the disjunctive
/// (`||`) closure passed to `use_reduce_flat_disjunctive` here accepts a
/// branch when every atom in it is either ordinarily satisfiable
/// (`atom_currently_satisfiable`, tree-visibility) *or* running-root-
/// satisfied (`running_root_satisfies_atom`) -- so a `DEPEND`/`BDEPEND`
/// `||` group with no branch visible in the fixture tree at all still
/// flattens correctly here as long as some branch is already installed
/// on the running root, matching real portage's own effective behavior
/// (a build-time tool that's already present on the host never needs a
/// *visible* ebuild candidate to satisfy a real `--root-deps` build).
/// The returned tokens are evaluated against `use_flags` (#133: real
/// evaluates at every producer), so callers compare evaluated forms.
/// Returns only the tokens satisfied by `running_root`'s own real vdb
/// (`running_root_satisfies_atom`) -- callers drop these from their own
/// already-flattened `flat_deps` before queueing (real "no separate
/// graph node needed for an already-satisfied dep"). Degrades to an
/// empty set on any flatten failure -- never a false negative that
/// could silently drop a dep portuale actually needed to walk.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn root_deps_satisfied_atoms(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    running_root: &Path,
    dep_keys: &[&str],
    empty_groups_always_true: bool,
) -> HashSet<String> {
    let mut build_depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            build_depstr.push_str(d);
            build_depstr.push(' ');
        }
    }
    let build_tokens: Vec<String> = build_depstr.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat_disjunctive(
        &build_tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
        empty_groups_always_true,
        &mut |atoms: &[String]| {
            // `--root-deps` only needs the satisfiable-or-not split here
            // (there's no "prefer the installed branch" question for a
            // build-time `||` group resolved against the running root).
            if atoms.iter().all(|a| {
                atom_currently_satisfiable(repos, a, config, &[])
                    || running_root_satisfies_atom(a, running_root, config)
            }) {
                portage_use_reduce::AltPreference::Available
            } else {
                portage_use_reduce::AltPreference::Unsatisfiable
            }
        },
        // No "prefer an upgrade" question here either -- Available-vs-
        // Unsatisfiable never ties on more than one selectable
        // alternative in a way this call site's own logic distinguishes,
        // so the trivial "first tied wins" shim is a strict no-op.
        &mut |_: &[Vec<String>]| 0,
    )
    .map(|flat| {
        flat.into_iter()
            .filter(|t| t != "||")
            // #133: real evaluates every token's conditional use-deps at
            // the producer itself (`use_reduce(..., token_class=Atom)`'s
            // own per-token `token.evaluate_conditionals(uselist)` step,
            // before any branch selection or split) -- without this the
            // raw `[flip=]` form reached the satisfied check below, where
            // a conditional form imposes no state constraint at all, and
            // every conditional build dep was vacuously
            // running-root-satisfied. Unparseable tokens pass through
            // as-is (today's tolerance, the same pass-through both
            // dep-walk loops' own post-passes use).
            .map(|t| {
                portage_dep::evaluate_atom_conditionals(&t, use_flags).unwrap_or_else(|| t.clone())
            })
            .filter(|t| running_root_satisfies_atom(t, running_root, config))
            .collect()
    })
    .unwrap_or_default()
}

/// The complement of `root_deps_satisfied_atoms`: real `DEPEND`/
/// `BDEPEND` atoms (or, for the recursive walk into a package that is
/// *itself* being built against the running root, `RDEPEND` + `IDEPEND`
/// too -- see `dep_keys` and `resolve_root_deps_build_entries`'s own doc
/// comment) that flatten out of `metadata` but are *not* already
/// satisfied by `running_root`'s own vdb -- the set real portage would
/// need to recursively resolve (and potentially build) against the
/// running root itself, rather than the target `ROOT`. `dep_keys` is
/// `["DEPEND", "BDEPEND", "IDEPEND"]` at the two ordinary dep-walk sites
/// (real `DEPEND`/`BDEPEND`-vs-`ESYSROOT`, plus `IDEPEND` which always
/// targets the running root for every package), and `["DEPEND",
/// "BDEPEND", "RDEPEND", "IDEPEND"]` when recursing into an already-
/// `targets_running_root` entry (real `_add_pkg_deps`'s own `deps`
/// tuple: a package whose own `pkg.root` is the running root has its
/// `RDEPEND` resolved there too, and `IDEPEND` *always* targets the
/// running root regardless -- `depgraph.py:4247-4252`).
/// A blocker atom (`!foo/bar`) is never a real build target, so it's
/// excluded here the same way `enqueue_flat_deps`/`enqueue_dependencies`
/// already exclude one from their own ordinary queueing. Computed as its
/// own separate flatten (duplicating `root_deps_satisfied_atoms`'s own
/// work) rather than refactoring that already-shipped, already-tested
/// function to return both halves at once -- deliberately additive/
/// isolated, minimizing risk to it.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn unsatisfied_root_deps_atoms(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    running_root: &Path,
    dep_keys: &[&str],
    empty_groups_always_true: bool,
) -> Vec<String> {
    let mut build_depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            build_depstr.push_str(d);
            build_depstr.push(' ');
        }
    }
    let build_tokens: Vec<String> = build_depstr.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat_disjunctive(
        &build_tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
        empty_groups_always_true,
        &mut |atoms: &[String]| {
            // `--root-deps` only needs the satisfiable-or-not split here
            // (there's no "prefer the installed branch" question for a
            // build-time `||` group resolved against the running root).
            if atoms.iter().all(|a| {
                atom_currently_satisfiable(repos, a, config, &[])
                    || running_root_satisfies_atom(a, running_root, config)
            }) {
                portage_use_reduce::AltPreference::Available
            } else {
                portage_use_reduce::AltPreference::Unsatisfiable
            }
        },
        // No "prefer an upgrade" question here either -- Available-vs-
        // Unsatisfiable never ties on more than one selectable
        // alternative in a way this call site's own logic distinguishes,
        // so the trivial "first tied wins" shim is a strict no-op.
        &mut |_: &[Vec<String>]| 0,
    )
    .map(|flat| {
        flat.into_iter()
            .filter(|t| t != "||")
            // #133: same producer-side evaluation as
            // `root_deps_satisfied_atoms` above -- real evaluates at every
            // producer, so the complement set is evaluated-keyed too.
            .map(|t| {
                portage_dep::evaluate_atom_conditionals(&t, use_flags).unwrap_or_else(|| t.clone())
            })
            .filter(|t| {
                portage_dep::parse_atom(t).is_some_and(|a| a.blocker == portage_dep::Blocker::None)
            })
            .filter(|t| !running_root_satisfies_atom(t, running_root, config))
            .collect()
    })
    .unwrap_or_default()
}

/// Metadata (md5-cache) and effective USE flags for `category/package`'s
/// own `version`, resolved against `repos` -- the highest-`repo_priority`
/// candidate providing that exact version (the same re-lookup
/// `slot_changed`/`deps_changed` already use; theoretically imprecise if
/// the highest-priority provider of that version is masked while a
/// lower-priority visible one exists, an accepted edge case those two
/// functions share). `None` if the version is no longer in the tree or
/// its metadata is unreadable, the same tolerant "can't tell, don't
/// crash" fallback. Used by `resolve_root_deps_build_entries` to walk a
/// freshly-pulled running-root build entry's own dependency strings with
/// that package's *own* effective USE, not its requester's.
pub(crate) fn resolved_version_meta_and_use(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    config: &portage_profile::Config,
) -> Option<(HashMap<String, String>, HashSet<String>)> {
    let candidates = list_candidates(repos, category, package).ok()?;
    let resolved = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)?;
    let pf = format!("{package}-{version}");
    let metadata = repo_aux_metadata(&resolved.repo_location, category, &pf).ok()?;
    let (_iuse, use_flags) = candidate_iuse_and_use(resolved, category, package, config)?;
    // Cold path (`--root-deps` only); the shared `Rc` isn't worth
    // threading through `unsatisfied_root_deps_atoms`'s `&HashMap`.
    Some(((*metadata).clone(), use_flags.as_ref().clone()))
}

/// Real "recursively pull in and build new packages against the running
/// root" (`--root-deps`, `depgraph.py:4207-4271`'s own `depend_root`
/// selection + `_add_pkg_deps`'s per-key `dep_root` tuple). Resolves
/// `atom_str` against `running_root` the same way any dependency atom is
/// resolved -- reusing `resolve_pretend` wholesale, `is_top_level: false`/
/// `selective: true`, `usepkg`/`usepkgonly` both `false` (a build-time
/// tool needed to actually perform a build is never satisfied by a
/// `--usepkg` binary, and portuale's `--root-deps` scope has never
/// touched binary packages) -- and then walks the resolved package's
/// *own* `DEPEND` + `BDEPEND` + `RDEPEND` + `IDEPEND` against the running
/// root too, recursively: real portage resolves all four of those
/// against the running root when `pkg.root` is the running root (a
/// package pulled in as a build tool is installed *there*, so its
/// runtime + install-time deps must be present there as well, not under
/// the target `ROOT`; `IDEPEND` in real portage always targets the
/// running root regardless -- `depgraph.py:4247-4252`). Still not
/// walked: this entry's own `PDEPEND` (real portage keeps it a
/// target-`ROOT` concern) -- a documented cut, not an oversight.
///
/// `seen` (the shared `root_deps_build_seen` set, threaded through the
/// whole graph resolution) is both the cross-package dedup key *and* the
/// cycle guard: a `(category, package)` is inserted *before* its own
/// dependencies are walked, so two build tools whose `BDEPEND`s point at
/// each other (an unremarkable bootstrap pattern) terminate cleanly --
/// the second edge back into an in-progress package just yields no
/// further entry. One `required_by` edge is therefore lost at whichever
/// point a cycle is cut (real portage's own bidirectional digraph keeps
/// both); a bounded, documented imprecision, the same best-effort
/// `required_by` tracking already has elsewhere.
///
/// Per outcome: `New`/`Upgrade`/`Downgrade`/`Reinstall` -> a real
/// `GraphEntry` (`targets_running_root: true`, `required_by` naming the
/// *immediate* requester) plus the recursion into its own deps;
/// `NoVisibleCandidate` -> a real `NoVisibleCandidate` entry too, so an
/// unbuildable, not-installed build dependency is surfaced by the
/// renderer's own `!!! no visible ebuild for dependency` note exactly as
/// it is without `--root-deps` (before this slice, `--root-deps` silently
/// swallowed it), no recursion; `AlreadyInstalled` -> nothing (the
/// running root already has it -- `unsatisfied_root_deps_atoms` and
/// `resolve_pretend` disagreeing in some USE-dep edge the former's
/// narrower check misses), no recursion; a `resolve_pretend` error ->
/// nothing.
///
/// `blockers` stay empty (a running-root build entry's blockers are the
/// recursion's business, already walked). Since #141 the display fields
/// (`use_flags_display` / `use_expand_display` / `use_expand_display_p`)
/// and the slot/repo identity are **populated** through the same
/// `refresh_entry_use_display` seam the main walk uses, so a running-root
/// rebuild row prints real's `USE="-flip*"` column instead of skipping
/// the display -- real's row is `[ebuild   R    ] … to <root>
/// USE="-flip*"` (bed `l0-fx-20260922T192350Z`). `oldbest` stays empty:
/// real's `[oldver]` bracket is an Upgrade-row concern and S0's capture
/// shows none on this shape (`findings/l0.md` "## #141/#135/#138/#134/#136
/// S0").
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn resolve_root_deps_build_entries(
    repos: &[RepoConfig],
    running_root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
    owner: (String, String),
    seen: &mut HashSet<(String, String)>,
    // Threaded through to `resolve_pretend` (backlog #123); unconsulted
    // here since `usepkg`/`usepkgonly` are both `false` below.
    local_binpkg: &std::sync::Arc<BinaryIndex>,
) -> Vec<GraphEntry> {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return Vec::new();
    };
    let key = (atom.category.clone(), atom.package.clone());
    if !seen.insert(key.clone()) {
        return Vec::new();
    }
    let Ok(outcome) = resolve_pretend(
        &SelectSource {
            repos,
            root: running_root,
            config,
            local_binpkg,
        },
        atom_str,
        &SelectOptions {
            // `--emptytree` deliberately does not reach `--root-deps`
            // running-root build entries -- a narrow, documented
            // non-interaction (this resolver is its own path, and
            // `-e --root-deps` is an exotic combination).
            empty: false,
            // Likewise `--getbinpkg`: a `--root-deps` build entry is always
            // an ebuild built against the running root.
            getbinpkg: false,
            // `--autounmask` keyword / USE resolution is a target-`ROOT`
            // concern (`resolve_pretend_graph`); a `--root-deps` running-root
            // build entry never applies either.
            autounmask_keywords: false,
            ..SelectOptions::default()
        },
    ) else {
        return Vec::new();
    };

    // `Some(version)` for exactly the outcomes worth recursing into;
    // `None` for `NoVisibleCandidate` (an entry, but nothing to walk);
    // an early return for `AlreadyInstalled` (no entry, nothing to walk).
    let recurse_version = match &outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version.clone())
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
            Some(to.clone())
        }
        // #72 B3: a removal is never walked (it has no deps).
        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
        PretendOutcome::AlreadyInstalled { .. } => return Vec::new(),
    };

    // `usepkg`/`usepkgonly` are both `false` in the `resolve_pretend`
    // call above, so `outcome` can only ever have come from an ebuild
    // candidate (real `dbs` never grows a binary entry in that case).
    let mut entry = GraphEntry {
        discovery: 0,
        category: atom.category.clone(),
        package: atom.package.clone(),
        outcome,
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
        required_by: vec![owner],
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: true,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    };
    // #141: the slot/repo identity and the USE-display fields the
    // renderer's `USE="…"` column reads. `refresh_entry_use_display`
    // diffs the tree candidate's effective USE against the *running*
    // root's vdb record (the root `resolve_pretend` just resolved
    // against), so a Reinstall whose flag flipped renders real's
    // `-flag*` marker exactly like the non-`--root-deps` rebuild row.
    if let Some(version) = recurse_version.as_ref() {
        let (sub, repo, slot) = slot_conflict_meta(repos, &atom.category, &atom.package, version);
        entry.slot = Some(slot);
        entry.sub_slot = Some(sub);
        entry.repo_name = Some(repo);
    }
    refresh_entry_use_display(
        std::slice::from_mut(&mut entry),
        repos,
        running_root,
        &key,
        config,
    );
    let mut result = vec![entry];

    if let Some(version) = recurse_version
        && let Some((metadata, use_flags)) =
            resolved_version_meta_and_use(repos, &atom.category, &atom.package, &version, config)
    {
        for dep_atom in unsatisfied_root_deps_atoms(
            &metadata,
            &use_flags,
            repos,
            config,
            running_root,
            &["DEPEND", "BDEPEND", "RDEPEND", "IDEPEND"],
            md5_dict::eapi_empty_groups_always_true(
                metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
            ),
        ) {
            result.extend(resolve_root_deps_build_entries(
                repos,
                running_root,
                &dep_atom,
                config,
                key.clone(),
                seen,
                local_binpkg,
            ));
        }
    }

    result
}
