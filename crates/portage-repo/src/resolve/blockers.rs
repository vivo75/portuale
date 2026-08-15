//! Blocker collection and resolution.

use crate::*;

/// A blocker atom from one package's dependency strings, not yet matched --
/// collected during the BFS and resolved in one post-pass once the whole
/// graph's New/Upgrade set is known, so a match never depends on discovery
/// order.
pub(crate) struct PendingBlocker {
    pub(crate) atom_str: String,
    pub(crate) strong: bool,
    /// Whether the atom came from a build-time key (`DEPEND`/`BDEPEND`).
    /// Under `--buildpkgonly` Portage clears every blocker's uninstall edge
    /// unless it is hard (`!!`) *and* buildtime.
    pub(crate) buildtime: bool,
    pub(crate) target_category: String,
    pub(crate) target_package: String,
    pub(crate) owner_key: (String, String),
    pub(crate) owner_version: String,
    /// Whether the producing owner was walked as a merge. Refined from the
    /// owner's final outcome when the owner is in the graph.
    pub(crate) owner_merging: bool,
    /// Whether this blocker came from the all-installed-packages scan: that
    /// owner is always `nomerge`, so its state must never be overridden from
    /// a same-cp merge entry.
    pub(crate) owner_installed: bool,
}

/// A resolved blocker with the owner version its renderer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiledBlocker {
    pub owner: (String, String),
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

/// Whether a set seed atom selects an installed instance: it matches it and
/// no instance this run merges for the same cp. Entries carry main-slot
/// data only, so sub-slot defaults to the main slot.
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

/// Arguments of [`graph_has_parent`].
pub(crate) struct GraphHasParentArgs<'a> {
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) exclude: &'a (String, String),
    pub(crate) entries: &'a [GraphEntry],
    /// Merge-bound slots per cp this run: a merging parent rebinds a bare `:=`
    /// through its own walk to the resolved slot, so its edge no longer
    /// parents an installed instance in another slot.
    pub(crate) merge_slots: &'a HashMap<(String, String), HashSet<String>>,
}

/// Whether some other walked entry depends on `category/package`'s
/// `version:slot/sub_slot` instance. Every outcome counts; `exclude` is the
/// blocker owner itself.
// Portage: depgraph.py, _validate_blockers (digraph parent test)
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
            // Rebound edge: this parent merges, its atom is a bare `:=`, and the
            // dep's cp merges in another slot -- no longer a parent here.
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

/// The owner-side half of the "a digraph node with parents" test: whether the
/// blocker owner is in the required-set closure. The set is cp-keyed, which
/// is exact for this single-instance arm.
// Portage: depgraph.py, _validate_blockers (step 4)
pub(crate) fn owner_set_parent(
    owner_key: &(String, String),
    blocker_retry_closure: &HashSet<(String, String)>,
) -> bool {
    blocker_retry_closure.contains(owner_key)
}
