//! `--root-deps` build entries for the running root.

use crate::*;

/// Arguments of [`root_deps_satisfied_atoms`].
pub(crate) struct RootDepsSatisfiedAtomsArgs<'a> {
    pub(crate) metadata: &'a HashMap<String, String>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) running_root: &'a Path,
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) empty_groups_always_true: bool,
}

/// The `--root-deps` build-vs-running-root split, shared by both dep-walk sites
/// so they cannot drift apart. Returns only the tokens already satisfied by
/// the running root's vdb (evaluated); empty on any flatten failure.
pub(crate) fn root_deps_satisfied_atoms(args: RootDepsSatisfiedAtomsArgs<'_>) -> HashSet<String> {
    let RootDepsSatisfiedAtomsArgs {
        metadata,
        use_flags,
        repos,
        config,
        running_root,
        dep_keys,
        empty_groups_always_true,
    } = args;
    let mut build_depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            build_depstr.push_str(d);
            build_depstr.push(' ');
        }
    }
    let build_tokens: Vec<String> = build_depstr.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens: &build_tokens,
            uselist: use_flags,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true, // First tied wins: no multi-way tie here.
            tie_break: &mut |_: &[Vec<String>]| 0,
        },
        &mut |atoms: &[String]| {
            if atoms.iter().all(|a| {
                atom_currently_satisfiable(repos, a, config, &[])
                    || running_root_satisfies_atom(a, running_root, config)
            }) {
                portage_use_reduce::AltPreference::Available
            } else {
                portage_use_reduce::AltPreference::Unsatisfiable
            }
        },
    )
    .map(|flat| {
        flat.into_iter()
            .filter(|t| t != "||")
            // Evaluated against the producer's USE first; unparseable tokens
            // pass through as-is.
            .map(|t| {
                portage_dep::evaluate_atom_conditionals(&t, use_flags).unwrap_or_else(|| t.clone())
            })
            .filter(|t| running_root_satisfies_atom(t, running_root, config))
            .collect()
    })
    .unwrap_or_default()
}

/// Arguments of [`unsatisfied_root_deps_atoms`].
pub(crate) struct UnsatisfiedRootDepsAtomsArgs<'a> {
    pub(crate) metadata: &'a HashMap<String, String>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) running_root: &'a Path,
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) empty_groups_always_true: bool,
}

/// The complement of `root_deps_satisfied_atoms`: flattened atoms *not* already
/// satisfied by the running root's vdb, needing recursive resolution there. A
/// blocker atom is never a build target, so it is excluded like in the
/// ordinary queueing. Computed as its own separate flatten rather than
/// refactoring the shipped function to return both halves.
// Portage: depgraph.py, _add_pkg_deps (dep_root tuple)
pub(crate) fn unsatisfied_root_deps_atoms(args: UnsatisfiedRootDepsAtomsArgs<'_>) -> Vec<String> {
    let UnsatisfiedRootDepsAtomsArgs {
        metadata,
        use_flags,
        repos,
        config,
        running_root,
        dep_keys,
        empty_groups_always_true,
    } = args;
    let mut build_depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            build_depstr.push_str(d);
            build_depstr.push(' ');
        }
    }
    let build_tokens: Vec<String> = build_depstr.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens: &build_tokens,
            uselist: use_flags,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true, // First tied wins: no multi-way tie here.
            tie_break: &mut |_: &[Vec<String>]| 0,
        },
        &mut |atoms: &[String]| {
            if atoms.iter().all(|a| {
                atom_currently_satisfiable(repos, a, config, &[])
                    || running_root_satisfies_atom(a, running_root, config)
            }) {
                portage_use_reduce::AltPreference::Available
            } else {
                portage_use_reduce::AltPreference::Unsatisfiable
            }
        },
    )
    .map(|flat| {
        flat.into_iter()
            .filter(|t| t != "||")
            // Same producer-side evaluation, so this set is evaluated-keyed too.
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

/// Metadata and effective USE for `category/package`'s own `version`: the
/// highest-`repo_priority` candidate with that version. `None` when
/// gone/unreadable; callers walk its deps with its own USE.
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
    // Cold path; the shared `Rc` isn't worth threading through.
    Some(((*metadata).clone(), use_flags.as_ref().clone()))
}

/// Arguments of [`resolve_root_deps_build_entries`].
pub(crate) struct ResolveRootDepsBuildEntriesArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) running_root: &'a Path,
    pub(crate) atom_str: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) owner: (String, String),
    pub(crate) seen: &'a mut HashSet<(String, String)>,
    pub(crate) local_binpkg: &'a std::sync::Arc<BinaryIndex>,
}

/// Resolves `atom_str` against the running root and walks the resolved
/// package's own `DEPEND` + `BDEPEND` + `RDEPEND` + `IDEPEND` there too,
/// recursively (`--root-deps`: a build tool is installed on the running root,
/// so its runtime deps must be present there as well). `PDEPEND` stays a
/// target-`ROOT` concern, a documented cut.
/// `seen` dedups cross-package and guards cycles (inserted before recursing;
/// one `required_by` edge is lost where a cycle is cut).
/// Per outcome: merge-bound outcomes gain an entry plus recursion;
/// `AlreadyInstalled` and errors yield nothing.
/// `blockers` stay empty, and the display fields reuse the main walk's seam.
// Portage: depgraph.py, _add_pkg_deps (depend_root / dep_root selection)
pub(crate) fn resolve_root_deps_build_entries(
    args: ResolveRootDepsBuildEntriesArgs<'_>,
) -> Vec<GraphEntry> {
    let ResolveRootDepsBuildEntriesArgs {
        repos,
        running_root,
        atom_str,
        config,
        owner,
        seen,
        local_binpkg,
    } = args;
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
            // `--emptytree`, `--getbinpkg` and `--autounmask` never reach
            // running-root build entries.
            empty: false,
            getbinpkg: false,
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
        // A removal is never walked (it has no deps).
        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
        PretendOutcome::AlreadyInstalled { .. } => return Vec::new(),
    };

    // Both `false` above, so the outcome can only come from an ebuild.
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
    // Slot/repo identity plus USE display, diffed against the running vdb.
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
        for dep_atom in unsatisfied_root_deps_atoms(UnsatisfiedRootDepsAtomsArgs {
            metadata: &metadata,
            use_flags: &use_flags,
            repos,
            config,
            running_root,
            dep_keys: &["DEPEND", "BDEPEND", "RDEPEND", "IDEPEND"],
            empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
            ),
        }) {
            result.extend(resolve_root_deps_build_entries(
                ResolveRootDepsBuildEntriesArgs {
                    repos,
                    running_root,
                    atom_str: &dep_atom,
                    config,
                    owner: key.clone(),
                    seen,
                    local_binpkg,
                },
            ));
        }
    }

    result
}
