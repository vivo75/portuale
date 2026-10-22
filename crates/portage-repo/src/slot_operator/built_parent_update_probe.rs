//! The update probe for a merge-bound binary parent (`slot_operator_mask_built`).

use crate::*;

/// Arguments of [`built_parent_update_probe`].
pub(crate) struct BuiltParentUpdateProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) local_binpkg: &'a BinaryIndex,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    /// The pass's `runtime_pkg_mask`: a masked tree instance is no update.
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
}

/// [`built_parent_update_probe`]'s result: the installed children joining the
/// replace set and the `(category, package, version)` binary parents to mask.
pub(crate) type BuiltParentUpdateProbe = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// Merge-bound binary parents whose built `:S=` dep is missing a slot ABI
/// update: the installed child in the bound slot has a tree instance with
/// another sub-slot that an ebuild replacement of the parent accepts. Each
/// returned binary `(category, package, version)` is masked so the restart
/// resolves the parent from the tree; the installed child joins the replace set,
/// as Portage's `_slot_operator_update_backtrack` seeds its `_replace_installed_atom`.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls, _slot_operator_update_probe, _slot_operator_update_backtrack
pub(crate) fn built_parent_update_probe(
    args: BuiltParentUpdateProbeArgs<'_>,
) -> BuiltParentUpdateProbe {
    let BuiltParentUpdateProbeArgs {
        root,
        repos,
        config,
        local_binpkg,
        entries,
        with_bdeps,
        excluded,
        masked,
    } = args;
    const DEP_KEYS: [&str; 5] = ["RDEPEND", "PDEPEND", "IDEPEND", "DEPEND", "BDEPEND"];
    let dep_keys: &[&str] = if with_bdeps {
        &DEP_KEYS
    } else {
        &DEP_KEYS[..3]
    };
    let mut masks = BTreeSet::new();
    let mut replace: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in entries {
        if entry.source != CandidateSource::Binary {
            continue;
        }
        let Some(bin_version) = merge_bound_version(&entry.outcome) else {
            continue;
        };
        let Some(bin_slot) = entry.slot.as_deref() else {
            continue;
        };
        let parent = (entry.category.clone(), entry.package.clone());
        let parent_cpv = format!("{}/{}-{bin_version}", parent.0, parent.1);
        if excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &parent_cpv))
        {
            continue;
        }
        let Some(bin_meta) = read_binary_metadata_any(ReadBinaryMetadataAnyArgs {
            config,
            root,
            local: local_binpkg,
            category: &parent.0,
            package: &parent.1,
            version: bin_version,
        }) else {
            continue;
        };
        let use_flags: HashSet<String> = entry
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let Ok(tree) = list_candidates(repos, &parent.0, &parent.1) else {
            continue;
        };
        // The ebuild replacements in the binary's slot, not older than it.
        let replacements: Vec<&Candidate> = tree
            .iter()
            .filter(|c| {
                c.source == CandidateSource::Ebuild
                    && c.slot == bin_slot
                    && vercmp_ordering(&c.version, bin_version) != Ordering::Less
                    && is_visible(c, &parent.0, &parent.1, config)
            })
            .collect();
        if replacements.is_empty() {
            continue;
        }
        let bin_tokens: Vec<String> = dep_keys
            .iter()
            .filter_map(|key| {
                flat_dep_atoms(
                    bin_meta.get(*key).map(String::as_str).unwrap_or_default(),
                    &use_flags,
                )
            })
            .flatten()
            .collect();
        let mut hit: Option<(String, String)> = None;
        for token in bin_tokens {
            let Some(atom) = portage_dep::parse_atom(&token) else {
                continue;
            };
            if atom.blocker != portage_dep::Blocker::None || !is_built_slot_op(&atom) {
                continue;
            }
            let child = (atom.category.clone(), atom.package.clone());
            let bound_slot = atom.slot.clone().unwrap_or_default();
            if child_has_slot_update(ChildUpdateArgs {
                root,
                repos,
                config,
                entries,
                parent: &parent,
                replacements: &replacements,
                use_flags: &use_flags,
                dep_keys,
                child: &child,
                bound_slot: &bound_slot,
                excluded,
                masked,
            }) {
                hit = Some(child.clone());
                break;
            }
        }
        if let Some(child) = hit {
            replace.insert(child);
            masks.insert((parent.0, parent.1, bin_version.clone()));
        }
    }
    (replace, masks)
}

/// Arguments of [`child_has_slot_update`].
struct ChildUpdateArgs<'a> {
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a portage_profile::Config,
    entries: &'a [GraphEntry],
    parent: &'a (String, String),
    replacements: &'a [&'a Candidate],
    use_flags: &'a HashSet<String>,
    dep_keys: &'a [&'a str],
    child: &'a (String, String),
    bound_slot: &'a str,
    excluded: &'a [String],
    masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
}

/// Whether the installed `child` in `bound_slot` has a tree instance with another
/// sub-slot that every atom on `child` in some replacement of `parent` accepts.
/// A child the graph already merges is left alone: the binary's dep then names
/// a different instance than the installed one.
// Portage: _emerge/depgraph.py, _slot_operator_update_probe (replacement_parent loop, candidate filter)
fn child_has_slot_update(args: ChildUpdateArgs<'_>) -> bool {
    let ChildUpdateArgs {
        root,
        repos,
        config,
        entries,
        parent,
        replacements,
        use_flags,
        dep_keys,
        child,
        bound_slot,
        excluded,
        masked,
    } = args;
    let (child_cat, child_pkg) = (child.0.as_str(), child.1.as_str());
    if entries.iter().any(|e| {
        e.category == child_cat
            && e.package == child_pkg
            && merge_bound_version(&e.outcome).is_some()
    }) {
        return false;
    }
    // The installed instance in the bound slot, the one the binary was built against.
    let Some((child_ver, _, _)) = installed_candidates(root, child_cat, child_pkg)
        .into_iter()
        .find(|(v, _, _)| read_vdb_slot(root, child_cat, child_pkg, v).0 == bound_slot)
    else {
        return false;
    };
    // An excluded installed child refuses the probe.
    if excluded
        .iter()
        .any(|ex| matches_cli_option_atom(ex, &format!("{child_cat}/{child_pkg}-{child_ver}")))
    {
        return false;
    }
    let child_sub = read_vdb_slot(root, child_cat, child_pkg, &child_ver).1;
    let child_sub = if child_sub.is_empty() {
        bound_slot.to_string()
    } else {
        child_sub
    };
    let Ok(child_cands) = list_candidates(repos, child_cat, child_pkg) else {
        return false;
    };
    for replacement in replacements {
        let pf = format!("{}-{}", parent.1, replacement.version);
        let Ok(meta) = repo_aux_metadata(&replacement.repo_location, &parent.0, &pf) else {
            continue;
        };
        // The replacement's atoms on the child's cp, USE-evaluated with the binary's USE.
        let mut atoms: Vec<String> = Vec::new();
        for key in dep_keys {
            let Some(tokens) = flat_dep_atoms(
                meta.get(*key).map(String::as_str).unwrap_or_default(),
                use_flags,
            ) else {
                continue;
            };
            for token in tokens {
                let Some(atom) = portage_dep::parse_atom(&token) else {
                    continue;
                };
                if atom.blocker == portage_dep::Blocker::None
                    && atom.category == child_cat
                    && atom.package == child_pkg
                {
                    atoms.push(portage_dep::without_use(&token).to_string());
                }
            }
        }
        if atoms.is_empty() {
            continue;
        }
        for cand in child_cands.iter() {
            // Portage: `_select_package` honours `runtime_pkg_mask`.
            let plain_neg = format!("!={child_cat}/{child_pkg}-{}", cand.version);
            if cand.source != CandidateSource::Ebuild
                || masked
                    .get(child)
                    .is_some_and(|b| b.iter().any(|m| m.neg == plain_neg))
                || cand.slot != bound_slot
                || cand.sub_slot == child_sub
                || vercmp_ordering(&cand.version, &child_ver) == Ordering::Less
                || !is_visible(cand, child_cat, child_pkg, config)
                || excluded.iter().any(|ex| {
                    matches_cli_option_atom(
                        ex,
                        &format!("{child_cat}/{child_pkg}-{}", cand.version),
                    )
                })
            {
                continue;
            }
            let cpv = format!(
                "{child_cat}/{child_pkg}-{}:{}/{}::{}",
                cand.version, cand.slot, cand.sub_slot, cand.repo_name
            );
            if atoms.iter().all(|a| {
                portage_dep::match_from_list(a, &[cpv.as_str()]).is_some_and(|m| !m.is_empty())
            }) {
                return true;
            }
        }
    }
    false
}
