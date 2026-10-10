//! The "required by" chain printed under a masked dependency.

use crate::*;

/// Arguments of [`chain_node_usedep_suffix`].
pub(crate) struct ChainNodeUsedepSuffixArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) node: &'a GraphEntry,
    pub(crate) version: &'a str,
    pub(crate) installed: bool,
    pub(crate) child: &'a (String, String),
}

/// Portage `_get_dep_chain`'s affecting-USE `pkg[flag]` suffix for one chain
/// node (depgraph.py:6257+ via `portage.dep.extract_affecting_use`): flags in
/// the node's dep strings whose `flag?` conditionals gate the edge to `child`,
/// `[flag]` when enabled in the node's USE, `[-flag]` when disabled, minus
/// use.mask/use.force. `""` when nothing gates the edge or metadata can't be
/// read (the chain keeps its bare row).
///
/// Approximations: every dep key is scanned (no buildtime/runtime split);
/// the linking atom is re-derived as the first token with the child's cp;
/// multi-flag rows sort (Portage iterates a `set`); malformed strings are
/// skipped (a display path must not fail the resolve). Mask/force for an
/// installed node reuses the same-version repo candidate's keywords.
pub(crate) fn chain_node_usedep_suffix(args: ChainNodeUsedepSuffixArgs<'_>) -> String {
    let ChainNodeUsedepSuffixArgs {
        repos,
        config,
        root,
        node,
        version,
        installed,
        child,
    } = args;
    // Same-version repo candidate when one exists: metadata for merge-bound
    // nodes, keyword source for the stability decision.
    let candidate = list_candidates(repos, &node.category, &node.package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .cloned()
        });
    let strings: Vec<String> = if installed {
        ["RDEPEND", "PDEPEND", "DEPEND"]
            .iter()
            .map(|k| read_vdb_string(root, &node.category, &node.package, version, k))
            .collect()
    } else {
        let Some(ref cand) = candidate else {
            return String::new();
        };
        let pf = format!("{}-{version}", node.package);
        let Ok(metadata) = repo_aux_metadata(&cand.repo_location, &node.category, &pf) else {
            return String::new();
        };
        ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"]
            .iter()
            .map(|k| metadata.get(*k).cloned().unwrap_or_default())
            .collect()
    };
    // The linking atom, re-derived: the first token with the child's cp
    // (Portage records the per-edge parent atom; `required_by` keeps pairs).
    let mut linking: Option<&str> = None;
    'scan: for s in &strings {
        for tok in s.split_whitespace() {
            if portage_dep::parse_atom(tok)
                .is_some_and(|a| a.category == child.0 && a.package == child.1)
            {
                linking = Some(tok);
                break 'scan;
            }
        }
    }
    let Some(linking) = linking else {
        return String::new();
    };
    let mut affecting: HashSet<String> = HashSet::new();
    for s in &strings {
        if s.trim().is_empty() {
            continue;
        }
        if let Some(flags) = portage_dep::extract_affecting_use(s, linking) {
            affecting.extend(flags);
        }
    }
    if affecting.is_empty() {
        return String::new();
    }
    // Portage `affecting_use.difference_update(node.use.mask, node.use.force)`.
    if let Some(ref cand) = candidate {
        let candidate_str = format!(
            "{}/{}-{}:{}/{}::{}",
            node.category, node.package, cand.version, cand.slot, cand.sub_slot, cand.repo_name
        );
        let stable = is_stable(IsStableArgs {
            keywords: &cand.keywords,
            candidate_str: &candidate_str,
            category: &node.category,
            package: &node.package,
            accept_keywords: &config.accept_keywords,
            package_accept_keywords: &config.package_accept_keywords,
        });
        let mut untouchable = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
            which: MaskOrForce::Mask,
            config,
            candidate_str: &candidate_str,
            category: &node.category,
            package: &node.package,
            stable,
        })
        .as_ref()
        .clone();
        untouchable.extend(
            resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
                which: MaskOrForce::Force,
                config,
                candidate_str: &candidate_str,
                category: &node.category,
                package: &node.package,
                stable,
            })
            .iter()
            .cloned(),
        );
        affecting.retain(|f| !untouchable.contains(f));
    }
    if affecting.is_empty() {
        return String::new();
    }
    // The node's own enabled set: effective USE merge-bound, recorded vdb
    // USE installed.
    let enabled: HashSet<String> = if installed {
        if !vdb_entry_is_dir(root, &node.category, &node.package, version) {
            return String::new();
        }
        read_vdb_flag_set(root, &node.category, &node.package, version, "USE")
    } else {
        let Some(ref cand) = candidate else {
            return String::new();
        };
        let Some((_, use_flags)) =
            candidate_iuse_and_use(cand, &node.category, &node.package, config)
        else {
            return String::new();
        };
        use_flags.as_ref().clone()
    };
    let mut flags: Vec<String> = affecting
        .into_iter()
        .map(|f| {
            if enabled.contains(&f) {
                f
            } else {
                format!("-{f}")
            }
        })
        .collect();
    flags.sort();
    format!("[{}]", flags.join(","))
}

/// `masked_dep_chain`'s `(node, type)` for one chain entry, or `None` to stop.
/// `child` is the cp below this node (the failed dependency for the first
/// row); its linking atom decides the affecting-USE suffix.
pub(crate) fn chain_node_line(
    entry: &GraphEntry,
    root: &Path,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    child: &(String, String),
) -> Option<(String, String)> {
    let version = match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::AlreadyInstalled { version } => version.clone(),
        // #72 B3: a removal is not a dependency-chain node.
        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => return None,
    };
    let (installed, repo, ty) = match &entry.outcome {
        PretendOutcome::AlreadyInstalled { .. } => (
            true,
            installed_pkg_repo(root, &entry.category, &entry.package, &version),
            "installed".to_string(),
        ),
        _ => (
            false,
            entry.repo_name.clone().unwrap_or_default(),
            if entry.source == CandidateSource::Binary {
                "binary".to_string()
            } else {
                "ebuild".to_string()
            },
        ),
    };
    let suffix = chain_node_usedep_suffix(ChainNodeUsedepSuffixArgs {
        repos,
        config,
        root,
        node: entry,
        version: &version,
        installed,
        child,
    });
    Some((
        format!(
            "{}/{}-{version}::{repo}{suffix}",
            entry.category, entry.package
        ),
        ty,
    ))
}

/// `masked_dep_chain`'s argument lines: the top-level atom texts
/// targeting `cp`, in request order.
pub(crate) fn chain_arg_lines(atoms: &[String], cp: &(String, String)) -> Vec<(String, String)> {
    atoms
        .iter()
        .filter(|a| {
            portage_dep::parse_atom(a).is_some_and(|at| {
                at.blocker == portage_dep::Blocker::None
                    && at.category == cp.0
                    && at.package == cp.1
            })
        })
        .map(|a| (a.clone(), "argument".to_string()))
        .collect()
}

/// Arguments of [`masked_dep_chain`].
pub(crate) struct MaskedDepChainArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) atoms: &'a [String],
    pub(crate) root: &'a Path,
    /// Backlog #286: the disclosed miss atom. Direct parents are restricted to
    /// those pulling this atom (Portage starts from the one failing parent
    /// edge); the restriction is vacuous when no direct parent matches.
    pub(crate) atom: &'a str,
}

/// Portage `_get_dep_chain` for one masked-dependency disclosure
/// (depgraph.py:6257+): `(node, type)` pairs from the failed dependency's
/// direct parents up to the top-level argument(s), innermost first.
/// Narrowings: every matching top-level atom is shown (Portage shows the
/// pulling one -- untracked here); one sub-chain per direct parent (Portage
/// shows only its DFS-first abandon; #19 parked); the affecting-USE suffix
/// is Portage's own ([`chain_node_usedep_suffix`] has the approximations).
pub(crate) fn masked_dep_chain(args: MaskedDepChainArgs<'_>) -> Vec<(String, String)> {
    let MaskedDepChainArgs {
        repos,
        config,
        entries,
        category,
        package,
        atoms,
        root,
        atom,
    } = args;
    /// This `cp`'s chain node: the merge-bound entry first (superseding any
    /// same-cp `AlreadyInstalled` shadow), else the installed one. `None`
    /// for `NoVisibleCandidate` or a missing entry -- the chain stops.
    fn select_entry<'a>(
        entries: &'a [GraphEntry],
        cp: &(String, String),
    ) -> Option<&'a GraphEntry> {
        entries
            .iter()
            .find(|e| {
                (e.category.clone(), e.package.clone()) == *cp
                    && matches!(
                        e.outcome,
                        PretendOutcome::New { .. }
                            | PretendOutcome::Upgrade { .. }
                            | PretendOutcome::Downgrade { .. }
                            | PretendOutcome::Reinstall { .. }
                    )
            })
            .or_else(|| {
                entries.iter().find(|e| {
                    (e.category.clone(), e.package.clone()) == *cp
                        && matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. })
                })
            })
    }
    let start = entries.iter().find(|e| {
        e.category == category
            && e.package == package
            && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)
    });
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let direct_all: Vec<(String, String)> =
        start.map(|e| e.required_by.clone()).unwrap_or_default();
    // Backlog #286: keep only the direct parents pulling the disclosed atom;
    // vacuous when none match.
    let direct: Vec<(String, String)> = {
        let pulling: Vec<(String, String)> = direct_all
            .iter()
            .filter(|(c, p)| {
                entries.iter().any(|e| {
                    e.category == *c
                        && e.package == *p
                        && e.deps.iter().any(|d| {
                            d.category == category
                                && d.package == package
                                && (d.atom == atom || d.evaluated == atom)
                        })
                })
            })
            .cloned()
            .collect();
        if pulling.is_empty() {
            direct_all
        } else {
            pulling
        }
    };
    for parent in direct {
        if !visited.insert(parent.clone()) {
            continue;
        }
        // The cp below the current node: the failed dependency first, the
        // previous chain node after each ascent step.
        let mut child = (category.to_string(), package.to_string());
        let mut cur = parent;
        while let Some(entry) = select_entry(entries, &cur) {
            let Some((node, ty)) = chain_node_line(entry, root, repos, config, &child) else {
                break;
            };
            chain.push((node, ty));
            // A chain top targeted from the command line ends in its argument
            // line(s), like Portage stopping at args.
            let args = chain_arg_lines(atoms, &cur);
            if !args.is_empty() {
                chain.extend(args);
                break;
            }
            let Some(next) = entry
                .required_by
                .iter()
                .find(|c| !visited.contains(*c))
                .cloned()
            else {
                break;
            };
            visited.insert(next.clone());
            child = cur;
            cur = next;
        }
    }
    chain
}
