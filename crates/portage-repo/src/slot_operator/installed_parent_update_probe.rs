//! The update probe for an installed parent (`slot_operator_replace_installed`).

use crate::slot_operator::ChildUpdateArgs;
use crate::slot_operator::child_has_slot_update;
use crate::*;

/// Arguments of [`installed_parent_update_probe`].
pub(crate) struct InstalledParentUpdateProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    /// The pass's `runtime_pkg_mask`: a masked tree instance is no update.
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
    /// The installed dependency graph and the installed consumers the pass's
    /// complete graph can contain (`live_reachable_consumers`).
    pub(crate) installed: &'a InstalledReachability,
    pub(crate) live: &'a HashSet<(String, String)>,
    /// The installed packages the request's own walk reaches: their dependencies
    /// have edges in Portage's digraph, which exempts them from the check.
    pub(crate) walked: &'a HashSet<(String, String)>,
    /// Request atoms: one pinning the installed parent below its tree
    /// replacement refuses the probe (Portage's per-dep `want_update` gate).
    pub(crate) top_level_atoms: &'a HashSet<String>,
    /// Update mode: without it an installed parent's dep wants no update.
    pub(crate) update: bool,
    pub(crate) gate: CategoryGate<'a>,
}

/// The installed `(category, package)` consumers and children joining the
/// replace set: an installed parent whose tree replacement pulls a new child
/// sub-slot, while the installed child stays graphed.
pub(crate) type InstalledParentUpdateProbe = BTreeSet<(String, String)>;

/// Installed parents whose built `:S/SS=` dep a tree replacement parent
/// re-binds to a new child sub-slot: both join the replace set, as Portage's
/// `_slot_operator_update_backtrack` seeds its `_replace_installed_atom` for
/// the installed parent and the installed child. Only parents the pass graphs
/// (Portage probes `_slot_operator_deps`, never the whole vdb), only under
/// `--update`, never for an argument-pinned parent.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls, _slot_operator_update_probe, _slot_operator_update_backtrack
pub(crate) fn installed_parent_update_probe(
    args: InstalledParentUpdateProbeArgs<'_>,
) -> InstalledParentUpdateProbe {
    let InstalledParentUpdateProbeArgs {
        root,
        config,
        entries,
        with_bdeps,
        excluded,
        masked,
        installed,
        live,
        walked,
        top_level_atoms,
        update,
        gate,
    } = args;
    // Portage: `want_update_probe = dep.want_update or not dep.parent.installed`.
    if !update {
        return BTreeSet::new();
    }
    const DEP_KEYS: [&str; 5] = ["RDEPEND", "PDEPEND", "IDEPEND", "DEPEND", "BDEPEND"];
    let dep_keys: &[&str] = if with_bdeps {
        &DEP_KEYS
    } else {
        &DEP_KEYS[..3]
    };
    let mut replace: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in entries {
        let PretendOutcome::AlreadyInstalled { version } = &entry.outcome else {
            continue;
        };
        let parent = (entry.category.clone(), entry.package.clone());
        let parent_cpv = format!("{}/{}-{version}", parent.0, parent.1);
        if excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &parent_cpv))
        {
            continue;
        }
        let (parent_slot, _) = read_vdb_slot(root, &parent.0, &parent.1, version);
        if parent_slot.is_empty() {
            continue;
        }
        let Ok(tree) = gate.repo_candidates(&parent.0, &parent.1) else {
            continue;
        };
        // The ebuild replacements in the installed slot, not older than it.
        // Portage ranges `_iter_similar_available` over visible, unmasked,
        // unexcluded instances only.
        let replacements: Vec<&Candidate> = tree
            .iter()
            .filter(|c| {
                c.source == CandidateSource::Ebuild
                    && c.slot == parent_slot
                    && vercmp_ordering(&c.version, version) != Ordering::Less
                    && is_visible(c, &parent.0, &parent.1, config)
                    && !masked.get(&parent).is_some_and(|b| {
                        b.iter()
                            .any(|m| m.neg == format!("!={}/{}-{}", parent.0, parent.1, c.version))
                    })
                    && !excluded.iter().any(|ex| {
                        matches_cli_option_atom(
                            ex,
                            &format!("{}/{}-{}", parent.0, parent.1, c.version),
                        )
                    })
            })
            .collect();
        if replacements.is_empty()
            || arg_pins_installed(top_level_atoms, &parent, version, &replacements)
        {
            continue;
        }
        let use_flags: HashSet<String> =
            read_vdb_flag_set(root, &parent.0, &parent.1, version, "USE");
        // The parent's recorded atoms, USE-reduced per key, minus the `||`
        // branches Portage's `dep_zapdeps` demotes: only the installed-
        // selected arm ever becomes a parent atom, so a slot-operator atom
        // in a deselected branch is no probe atom.
        let mut probe_atoms: Vec<String> = Vec::new();
        for key in dep_keys {
            let depstr = read_vdb_string(root, &parent.0, &parent.1, version, key);
            if depstr.trim().is_empty() {
                continue;
            }
            let demoted = or_branch_vdb_deselected_set(root, &depstr, &use_flags);
            let Some(flat) = flat_dep_atoms(&depstr, &use_flags) else {
                continue;
            };
            for atom_str in flat {
                let Some(atom) = parse_dep_atom(&atom_str, "installed_parent_tokens") else {
                    continue;
                };
                if demoted.contains(atom_str.as_str())
                    || demoted.contains(reverse_dep_constraint_atom(&atom_str, &atom).as_str())
                {
                    continue;
                }
                probe_atoms.push(atom_str);
            }
        }
        for token in probe_atoms {
            let Some(atom) = parse_dep_atom(&token, "installed_parent_tokens") else {
                continue;
            };
            if atom.blocker != portage_dep::Blocker::None
                || atom.slot_operator != Some(portage_dep::SlotOperator::Equals)
                || atom.slot.is_none()
                || atom.sub_slot.is_none()
            {
                continue;
            }
            let child = (atom.category.clone(), atom.package.clone());
            let bound_slot = atom.slot.clone().unwrap_or_default();
            // An excluded installed child refuses the probe, like the parent.
            let child_ver = installed_candidates(root, &child.0, &child.1)
                .into_iter()
                .find(|(v, _, _)| read_vdb_slot(root, &child.0, &child.1, v).0 == bound_slot)
                .map(|(v, _, _)| v);
            if child_ver.as_ref().is_some_and(|v| {
                excluded
                    .iter()
                    .any(|ex| matches_cli_option_atom(ex, &format!("{}/{}-{v}", child.0, child.1)))
            }) {
                continue;
            }
            if child_has_slot_update(ChildUpdateArgs {
                root,
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
                installed,
                live,
                walked,
                gate,
            }) {
                // Portage files both filings: the installed parent's
                // replacement atom and the installed child's.
                replace.insert(parent.clone());
                replace.insert(child);
            }
        }
    }
    replace
}

/// Whether a request atom pins the installed parent below its update: it
/// matches the installed instance but no strictly newer tree replacement.
/// Stands in for Portage's per-dep `want_update` (an argument-pinned subtree
/// wants no update); portuale tracks no per-edge update-wanted flag.
fn arg_pins_installed(
    top_level_atoms: &HashSet<String>,
    parent: &(String, String),
    version: &str,
    replacements: &[&Candidate],
) -> bool {
    let inst_cpv = format!("{}/{}-{version}", parent.0, parent.1);
    top_level_atoms.iter().any(|t| {
        portage_dep::match_from_list(t, &[inst_cpv.as_str()]).is_some_and(|m| !m.is_empty())
            && !replacements
                .iter()
                .filter(|c| vercmp_ordering(&c.version, version) == Ordering::Greater)
                .any(|c| {
                    let cpv = format!("{}/{}-{}", parent.0, parent.1, c.version);
                    portage_dep::match_from_list(t, &[cpv.as_str()]).is_some_and(|m| !m.is_empty())
                })
    })
}
