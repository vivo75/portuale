//! Binding `:=` dependencies to the selected slot.

use crate::*;

/// Binds every `:=`/`:S=` atom to the selected slot: a merge-bound
/// [`GraphEntry`] first, else the highest matching installed version.
/// An atom no candidate satisfies is left unchanged.
// Portage: dep/_slot_operator.py, _eval_deps
pub(crate) fn bind_slot_operator_deps(depstr: &str, entries: &[GraphEntry], root: &Path) -> String {
    depstr
        .split_whitespace()
        .map(|token| bind_slot_operator_token(token, entries, root))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The built `:=` binding for `cat/pkg`: the tree sub-slot of the highest
/// installed version, or `None` when there is none.
pub(crate) fn built_equals_binding(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
) -> Option<(String, String)> {
    let inst_ver = installed_candidates(root, category, package)
        .into_iter()
        .map(|(v, _, _)| v)
        .max_by(|x, y| vercmp_ordering(x, y))?;
    let (bound_sub, _, _) = slot_conflict_meta(repos, category, package, &inst_ver);
    if bound_sub.is_empty() {
        return None;
    }
    Some((inst_ver, bound_sub))
}

pub(crate) fn built_equals_shifted_slot(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    existing_sub: &str,
) -> bool {
    built_equals_binding(root, repos, category, package)
        .is_some_and(|(_, bound_sub)| bound_sub != existing_sub)
}

/// Arguments of [`built_equals_shift_unrebuildable`].
pub(crate) struct BuiltEqualsShiftUnrebuildableArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) existing_sub: &'a str,
    pub(crate) pullers: &'a SlotPullers,
    pub(crate) excluded: &'a [String],
}

/// A `:=` shift is only a conflict when the bound consumer cannot be
/// rebuilt (masked or `--exclude`d); a rebuildable consumer rebuilds in
/// place with no conflict.
pub(crate) fn built_equals_shift_unrebuildable(
    args: BuiltEqualsShiftUnrebuildableArgs<'_>,
) -> bool {
    let BuiltEqualsShiftUnrebuildableArgs {
        root,
        repos,
        category,
        package,
        existing_sub,
        pullers,
        excluded,
    } = args;
    if !built_equals_shifted_slot(root, repos, category, package, existing_sub) {
        return false;
    }
    pullers
        .get(&(category.to_string(), package.to_string()))
        .is_some_and(|ps| {
            ps.iter().any(|(pcat, ppkg, pver, _)| {
                let cpv = format!("{pcat}/{ppkg}-{pver}");
                excluded.iter().any(|ex| matches_cli_option_atom(ex, &cpv))
                    || useoldpkg_atom_matches(pcat, ppkg, pver)
            })
        })
}

/// One token of [`bind_slot_operator_deps`]: string surgery on the atom's
/// own slot-dep substring (`:=` / `:2=` / `:2/3=`), which appears exactly
/// once in a well-formed atom, so `replacen(.., 1)` is safe.
pub(crate) fn bind_slot_operator_token(token: &str, entries: &[GraphEntry], root: &Path) -> String {
    // drop-ok: unparseable token returned unchanged, not dropped
    let Some(atom) = portage_dep::parse_atom(token) else {
        return token.to_string();
    };
    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
        return token.to_string();
    }
    let matches = |version: &str, slot: &str, sub_slot: &str| {
        let candidate = format!(
            // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
            "{}/{}-{version}:{slot}/{sub_slot}",
            atom.category, atom.package
        );
        atom_matches_str(token, &candidate)
    };
    let mut best: Option<(String, String, String)> = None;
    for e in entries {
        if e.category != atom.category || e.package != atom.package {
            continue;
        }
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => version,
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
            _ => continue,
        };
        let (Some(slot), Some(sub_slot)) = (e.slot.as_deref(), e.sub_slot.as_deref()) else {
            continue;
        };
        if !matches(version, slot, sub_slot) {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(v, _, _)| vercmp_ordering(version, v) == Ordering::Greater)
        {
            best = Some((version.clone(), slot.to_string(), sub_slot.to_string()));
        }
    }
    let best = best.or_else(|| {
        installed_candidates(root, &atom.category, &atom.package)
            .into_iter()
            .filter(|(version, slot, sub_slot)| matches(version, slot, sub_slot))
            .max_by(|(a, _, _), (b, _, _)| vercmp_ordering(a, b))
    });
    let Some((_, slot, sub_slot)) = best else {
        return token.to_string();
    };
    let old_slotdep = match (&atom.slot, &atom.sub_slot) {
        (None, _) => ":=".to_string(),
        (Some(s), None) => format!(":{s}="),
        (Some(s), Some(ss)) => format!(":{s}/{ss}="),
    };
    token.replacen(&old_slotdep, &format!(":{slot}/{sub_slot}="), 1)
}

/// `match_from_list` against one candidate string. USE deps are not
/// evaluated here (the candidate carries no USE state).
pub(crate) fn atom_matches_str(atom: &str, candidate: &str) -> bool {
    portage_dep::match_from_list(atom, &[candidate]).is_some_and(|m| !m.is_empty())
}

/// Removes every atom on an installed libc provider from a flattened set.
// Portage: dep/libc.py, strip_libc_deps
pub(crate) fn strip_libc_atoms(
    mut atoms: HashSet<String>,
    libc_cps: &HashSet<(String, String)>,
) -> HashSet<String> {
    if libc_cps.is_empty() {
        return atoms;
    }
    atoms.retain(|t| {
        // drop-ok: kept
        portage_dep::parse_atom(t).is_none_or(|a| !libc_cps.contains(&(a.category, a.package)))
    });
    atoms
}

/// The tree ebuild's md5-cache metadata for `cp` at `version`.
/// `None` when it cannot be read: the rebuild is kept (conservative).
pub(crate) fn tree_metadata_for(
    repos: &[RepoConfig],
    cp: &(String, String),
    version: &str,
) -> Option<std::sync::Arc<HashMap<String, String>>> {
    let candidates = list_candidates(repos, &cp.0, &cp.1).ok()?;
    let resolved = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)?;
    repo_aux_metadata(
        &resolved.repo_location,
        &cp.0,
        &format!("{}-{version}", cp.1),
    )
    .ok()
}

/// Applies Portage's `prune_rebuilds` restart to the working params:
/// latches the flag, clears the replace set, and drops the
/// `slot_operator_mask_built` steers.
// Portage: resolver/backtracking.py, _feedback_config
pub(crate) fn apply_prune_rebuilds(params: &mut BacktrackParams) {
    params.prune_rebuilds = true;
    params.slot_operator_replace_installed.clear();
    params.probe_replaced.clear();
    // The forcing half goes with the set.
    params.slot_operator_forced_upgrade.clear();
    let mut removed: Vec<((String, String), String)> = Vec::new();
    params.runtime_pkg_mask.retain(|cp, bucket| {
        bucket.retain(|entry| {
            let drop_it = matches!(entry.reason, MaskReason::SlotOperatorBuilt);
            if drop_it {
                removed.push((cp.clone(), entry.neg.clone()));
            }
            !drop_it
        });
        !bucket.is_empty()
    });
    params.mask_order.retain(|slot| !removed.contains(slot));
}
