//! The slot-conflict ABI probe.

use crate::*;

/// One installed parent's atom on a provider, for the update-probe refusal:
/// the parent's identity plus the effective atom text to match the fresh
/// candidate against.
#[derive(Debug, Clone)]
pub(crate) struct ProbeParent {
    /// The installed parent `(category, package)`.
    pub(crate) parent: (String, String),
    /// Effective atom: [`relax_built_slot_operator_atom`] for built
    /// slot-operator atoms, verbatim otherwise.
    pub(crate) atom: String,
}

/// Display continuity for conflict-fired rebuilds: recomputed here from the
/// settled state, since the firing pass restarts the search. For every
/// `slot_operator_rebuild` reinstall entry, each recorded built `:=` atom on
/// a provider cp merged in the same slot at a different sub-slot contributes
/// its `(provider-cpv, consumer-cpv)` pair.
// Portage: _emerge/depgraph.py, _compute_abi_rebuild_info
pub(crate) fn slot_conflict_abi_display_pairs(
    root: &Path,
    entries: &[GraphEntry],
    replace_set: &BTreeSet<(String, String)>,
    with_bdeps: bool,
) -> Vec<(String, String)> {
    let walked: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for e in entries {
        let PretendOutcome::Reinstall {
            version,
            slot_operator_rebuild: true,
            ..
        } = &e.outcome
        else {
            continue;
        };
        let ccp = (e.category.clone(), e.package.clone());
        if !replace_set.contains(&ccp) {
            continue;
        }
        let consumer_cpv = format!("{}/{}-{version}", e.category, e.package);
        let dep_keys: &[&str] = match with_bdeps && walked.contains(&ccp) {
            true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
            false => &["RDEPEND", "PDEPEND", "IDEPEND"],
        };
        for key in dep_keys {
            let recorded = read_vdb_string(root, &e.category, &e.package, version, key);
            for tok in recorded.split_whitespace() {
                let Some(atom) = portage_dep::parse_atom(tok) else {
                    continue;
                };
                if !is_built_slot_op(&atom) {
                    continue;
                }
                let Some(bound_slot) = atom.slot.clone() else {
                    continue;
                };
                let bound_sub = atom.sub_slot.clone().unwrap_or_default();
                // Only a provider that is a graph child of the replacement
                // consumer counts: a rebuild whose new build no longer pulls
                // the provider was not caused by it.
                for cand in entries.iter().filter(|o| {
                    o.category == atom.category
                        && o.package == atom.package
                        && merge_bound_version(&o.outcome).is_some()
                        && o.slot.as_deref() == Some(bound_slot.as_str())
                        && o.sub_slot.as_deref().is_some_and(|s| *s != bound_sub)
                        && o.required_by.contains(&ccp)
                }) {
                    let Some(pver) = merge_bound_version(&cand.outcome) else {
                        continue;
                    };
                    pairs.push((
                        format!("{}/{}-{pver}", atom.category, atom.package),
                        consumer_cpv.clone(),
                    ));
                }
            }
        }
    }
    pairs
}

/// The same-slot vdb scan (Portage's `_slot_operator_trigger_reinstalls`):
/// an installed consumer whose built `:S/SS=` atom no longer matches how this
/// run leaves that slot is rebuilt. Returns the consumer set plus display
/// pairs and builds no `GraphEntry`: the consumer becomes a walked node on the
/// next pass. Only a provider entry that replaces an installed version in a
/// slot counts, and only for a consumer bound to that slot at a different
/// sub-slot. The parameter gates (`reachable`, `already`, `undone`,
/// `with_bdeps`) are recorded in the notes.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls
/// Highest visible `(slot, version)` for a cp: the slot a `--update` request
/// keeps unpinned while every lower installed slot gains a `cat/pkg:S` pin.
pub(crate) fn highest_visible_slot(
    repos: &[RepoConfig],
    cp: &(String, String),
) -> Option<(String, String)> {
    list_candidates(repos, &cp.0, &cp.1)
        .ok()?
        .iter()
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
        .map(|c| (c.slot.clone(), c.version.clone()))
}

/// The requested cps Portage's `--update` greedy expansion actually pins:
/// each directly-requested atom expands into per-slot `cat/pkg:S` args, but
/// only when the atom spans slots (an explicit slot or exact-version request
/// pins nothing). Visibility/masking is unfiltered here.
// Portage: _emerge/depgraph.py, _greedy_slots
pub(crate) fn greedy_pinned_cps(
    atoms: &HashSet<String>,
    repos: &[RepoConfig],
    installed_by_cp: &HashMap<(String, String), Vec<(String, String)>>,
) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for atom_str in atoms {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        let cp = (atom.category.clone(), atom.package.clone());
        if out.contains(&cp) {
            continue;
        }
        let mut slots: HashSet<String> = HashSet::new();
        if let Some(insts) = installed_by_cp.get(&cp) {
            for (version, slot) in insts {
                let r = format!("{}/{}-{version}:{slot}", atom.category, atom.package);
                if portage_dep::match_from_list(atom_str, &[r.as_str()])
                    .is_some_and(|m| !m.is_empty())
                {
                    slots.insert(slot.split('/').next().unwrap_or(slot).to_string());
                }
            }
        }
        if let Ok(tree) = list_candidates(repos, &atom.category, &atom.package) {
            let best = tree
                .iter()
                .filter(|c| {
                    let r = format!(
                        "{}/{}-{}:{}/{}",
                        atom.category, atom.package, c.version, c.slot, c.sub_slot
                    );
                    portage_dep::match_from_list(atom_str, &[r.as_str()])
                        .is_some_and(|m| !m.is_empty())
                })
                .max_by(|a, b| vercmp_ordering(&a.version, &b.version));
            if let Some(h) = best {
                slots.insert(h.slot.clone());
            }
        }
        if slots.len() >= 2 {
            out.insert(cp);
        }
    }
    out
}
