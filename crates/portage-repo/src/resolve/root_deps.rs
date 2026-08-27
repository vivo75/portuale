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

/// Arguments of [`flatten_evaluated_root_deps_atoms`]: the inputs both
/// `--root-deps` halves share.
pub(crate) struct FlattenRootDepsAtomsArgs<'a> {
    pub(crate) metadata: &'a HashMap<String, String>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) running_root: &'a Path,
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) empty_groups_always_true: bool,
}

/// The evaluated flat build-depstring both `--root-deps` halves filter:
/// `dep_keys` joined, flattened with running-root availability as the tie
/// preference, and USE-evaluated against the producer's flags
/// (unparseable tokens pass through as-is). Empty on any flatten failure.
fn flatten_evaluated_root_deps_atoms(args: FlattenRootDepsAtomsArgs<'_>) -> Vec<String> {
    let FlattenRootDepsAtomsArgs {
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
            .collect()
    })
    .unwrap_or_default()
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
    flatten_evaluated_root_deps_atoms(FlattenRootDepsAtomsArgs {
        metadata,
        use_flags,
        repos,
        config,
        running_root,
        dep_keys,
        empty_groups_always_true,
    })
    .into_iter()
    .filter(|t| running_root_satisfies_atom(t, running_root, config))
    .collect()
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
/// ordinary queueing. Shares the flatten with the shipped function above,
/// filtering the complement instead of flattening twice.
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
    flatten_evaluated_root_deps_atoms(FlattenRootDepsAtomsArgs {
        metadata,
        use_flags,
        repos,
        config,
        running_root,
        dep_keys,
        empty_groups_always_true,
    })
    .into_iter()
    // Same producer-side evaluation, so this set is evaluated-keyed too.
    .filter(|t| portage_dep::parse_atom(t).is_some_and(|a| a.blocker == portage_dep::Blocker::None))
    .filter(|t| !running_root_satisfies_atom(t, running_root, config))
    .collect()
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
