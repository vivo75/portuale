//! Constraints that installed reverse dependencies put on an upgrade.

use crate::*;

/// Whether `atom` is a **built** slot-operator atom (`cat/pkg:S/SS=`).
/// Slot dropped textually (equivalent); `[use-dep]` dropped too (allow-only).
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
pub(crate) fn is_built_slot_op(atom: &portage_dep::Atom) -> bool {
    atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
        && atom.slot.is_some()
        && atom.sub_slot.is_some()
}

pub(crate) fn reverse_dep_constraint_atom(atom_str: &str, atom: &portage_dep::Atom) -> String {
    let head = atom_str.split('[').next().unwrap_or(atom_str);
    if is_built_slot_op(atom) {
        head.split(':').next().unwrap_or(head).to_string()
    } else {
        head.to_string()
    }
}

/// Portage `Atom.with_slot("=")`: a built atom's `:S/SS=` binding relaxes to
/// bare `:=` (rebuild, never withhold). Non-built/unparseable returns verbatim.
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
pub(crate) fn relax_built_slot_operator_atom(atom_str: &str) -> String {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return atom_str.to_string();
    };
    if !is_built_slot_op(&atom) {
        return atom_str.to_string();
    }
    // Both slots are guaranteed present, so the needle occurs verbatim.
    let needle = format!(
        ":{}/{}=",
        atom.slot.as_deref().unwrap_or_default(),
        atom.sub_slot.as_deref().unwrap_or_default()
    );
    match atom_str.find(&needle) {
        Some(idx) => format!("{}:={}", &atom_str[..idx], &atom_str[idx + needle.len()..]),
        None => atom_str.to_string(),
    }
}

/// Whether an installed-settling edge's atom is a slot-operator rebuild trigger
/// (a built atom accepting the resolved merge once relaxed) rather than a slot
/// conflict. A version veto still records normally; unparseable never triggers.
// Portage: depgraph.py, _minimize_children (update probe)
pub(crate) fn built_slot_operator_rebuild_trigger(atom_str: &str, existing_str: &str) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if !is_built_slot_op(&atom) {
        return false;
    }
    let relaxed = relax_built_slot_operator_atom(atom_str);
    portage_dep::match_from_list(&relaxed, &[existing_str]).is_some_and(|m| !m.is_empty())
}

/// One installed consumer's recorded atom broken by this pass's upgrade, with
/// the consumer attached (Portage's nomerge node, as a triple).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RevDepPin {
    /// The upgraded `category/package`.
    pub(crate) cp: (String, String),
    /// The consumer's recorded atom, normalised by
    /// `reverse_dep_constraint_atom`.
    pub(crate) atom: String,
    /// Verbatim text (with `[use]` deps), for notices only.
    pub(crate) raw_atom: String,
    /// The installed consumer `(category, package, version)`.
    pub(crate) consumer: (String, String, String),
}

/// Arguments of [`rev_dep_pin_holdable`].
pub(crate) struct RevDepPinHoldableArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) cp: &'a (String, String),
    pub(crate) pin: &'a str,
    pub(crate) pin_slot: Option<&'a str>,
    pub(crate) upgraded_slots: &'a std::collections::HashSet<String>,
    pub(crate) hard_atoms: &'a [String],
}

/// Whether `pin` on `cp` still holds alongside every hard requirement. A
/// jointly-unsatisfiable pin is DROPPED (Portage merges anyway). Per-slot;
/// unparseable or candidate-less falls back to enforcing.
// Portage: depgraph.py, _select_pkg_highest_available (whole atom set)
pub(crate) fn rev_dep_pin_holdable(args: RevDepPinHoldableArgs<'_>) -> bool {
    let RevDepPinHoldableArgs {
        root,
        repos,
        cp,
        pin,
        pin_slot,
        upgraded_slots,
        hard_atoms,
    } = args;
    if portage_dep::parse_atom(pin).is_none() {
        return true;
    }
    // An unparseable hard atom cannot constrain anything: enforce.
    let mut hard_parsed: Vec<(&str, portage_dep::Atom)> = Vec::new();
    for h in hard_atoms {
        match portage_dep::parse_atom(h) {
            Some(a) => hard_parsed.push((h.as_str(), a)),
            None => return true,
        }
    }
    let Ok(cands) = list_candidates(repos, &cp.0, &cp.1) else {
        return true;
    };
    let constrained: Vec<&String> = upgraded_slots
        .iter()
        .filter(|s| pin_slot.is_none_or(|ps| ps == *s))
        .collect();
    if constrained.is_empty() {
        return true;
    }
    // Installed instances count too (withholding keeps them).
    let holds_for = |slot: &String, cands: &[String]| {
        cands.iter().any(|s| {
            let refs = [s.as_str()];
            if portage_dep::match_from_list(pin, &refs).is_none_or(|m| m.is_empty()) {
                return false;
            }
            hard_parsed.iter().all(|(text, h)| {
                if h.slot.is_some() && h.slot.as_deref() != Some(slot.as_str()) {
                    return true;
                }
                portage_dep::match_from_list(text, &refs).is_some_and(|m| !m.is_empty())
            })
        })
    };
    constrained.iter().any(|slot| {
        let repo_cands: Vec<String> = cands
            .iter()
            .filter(|c| &c.slot == *slot)
            .map(|c| {
                format!(
                    "{}/{}-{}:{}/{}::{}",
                    cp.0, cp.1, c.version, c.slot, c.sub_slot, c.repo_name
                )
            })
            .collect();
        if holds_for(slot, &repo_cands) {
            return true;
        }
        // Installed instances at this slot (withholding keeps them).
        let inst_cands: Vec<String> = all_installed_packages(root)
            .iter()
            .filter(|p| p.category == cp.0 && p.package == cp.1)
            .filter_map(|p| {
                let (inst_slot, inst_sub) =
                    read_vdb_slot(root, &p.category, &p.package, &p.version);
                (inst_slot == **slot).then(|| {
                    format!(
                        "{}/{}-{}:{}/{}::{}",
                        p.category,
                        p.package,
                        p.version,
                        inst_slot,
                        inst_sub,
                        installed_pkg_repo(root, &p.category, &p.package, &p.version)
                    )
                })
            })
            .collect();
        holds_for(slot, &inst_cands)
    })
}

/// Whether a dep key is build-time: Portage empties those keys for built
/// packages, so only build-time pins are unconditionally ignorable.
pub(crate) fn is_build_time_dep_key(key: impl AsRef<str>) -> bool {
    matches!(key.as_ref(), "DEPEND" | "BDEPEND")
}

/// The decided merge USE/IUSE for `category/package-version`. `None` when no
/// readable candidate exists; callers fall back to ignoring USE.
pub(crate) fn merge_use_state(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Option<(HashSet<String>, HashSet<String>)> {
    let cands = list_candidates(repos, category, package).ok()?;
    let cand = cands.iter().find(|c| c.version == version)?;
    let (iuse, use_flags) = candidate_iuse_and_use(cand, category, package, config)?;
    Some(((*use_flags).clone(), iuse))
}

/// Arguments of [`merge_use_changed_vs_installed`].
pub(crate) struct MergeUseChangedVsInstalledArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
}

/// Portage's USE-change trigger: merge IUSE or enabled-USE∩IUSE differs from
/// the installed instance's, independent of `--newuse`. `None` when unreadable.
// Portage: depgraph.py, _complete_graph (complete_if_new_use)
pub(crate) fn merge_use_changed_vs_installed(
    args: MergeUseChangedVsInstalledArgs<'_>,
) -> Option<bool> {
    let MergeUseChangedVsInstalledArgs {
        root,
        repos,
        config,
        category,
        package,
        version,
    } = args;
    let (merge_use, merge_iuse) = merge_use_state(repos, config, category, package, version)?;
    let inst_use = read_vdb_flag_set(root, category, package, version, "USE");
    let inst_iuse = read_vdb_flag_set(root, category, package, version, "IUSE");
    if merge_iuse != inst_iuse {
        return Some(true);
    }
    let merge_enabled: HashSet<&String> = merge_use.intersection(&merge_iuse).collect();
    let inst_enabled: HashSet<&String> = inst_use.intersection(&inst_iuse).collect();
    Some(merge_enabled != inst_enabled)
}
