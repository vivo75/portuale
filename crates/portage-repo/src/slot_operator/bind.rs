//! Binding `:=` dependencies to the selected slot.

use crate::*;

/// Backlog #24 S4: real portage's graph-aware `:=`/`:S=` binder (real
/// `portage/dep/_slot_operator.py::_eval_deps`, run per atom from
/// `evaluate_slot_operator_equal_deps`). Real binds every slot-operator
/// atom against `_graph_trees[root]["vartree"]`, whose dbapi is the
/// `PackageTrackerDbapiWrapper` (`depgraph.py:744-775`) -- "the state
/// that the vdb will have after new packages have been installed". So a
/// merge-bound package wins over the installed instance: in portuale's
/// one-instance-per-cp model that is a merge-bound (New/Upgrade/
/// Downgrade/Reinstall) [`GraphEntry`] first, else
/// `<root>/var/db/pkg` (`installed_candidates`), highest matching
/// version (`vardb.match(x)[-1]`). An atom no candidate satisfies is
/// left unchanged, exactly like real's own "just leave it as-is for now
/// ... keeping the information in vdb".
pub(crate) fn bind_slot_operator_deps(depstr: &str, entries: &[GraphEntry], root: &Path) -> String {
    depstr
        .split_whitespace()
        .map(|token| bind_slot_operator_token(token, entries, root))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Backlog #90 (S1): does a bare `cat/pkg:=` atom's built binding point
/// at a different sub-slot than `existing_sub`? Real binds `:=` to the
/// sub-slot of the *installed* instance (the ABI it was built against),
/// never to whatever version this walk happened to select first: under
/// reversed walk order the raw `:=` would match the just-selected
/// 3.0.4:0/0 and swallow the genuine sub-slot shift the installed
/// 3.1.1:0/13 reports (the need_rebuild-trailer shape: ark's built
/// `:0/0=` alongside the ebuild `:=`). The binding's sub-slot is the
/// tree metadata's for the highest installed version (the vdb `SLOT`
/// file of a minimal root need not carry it); no installed instance --
/// or no tree sub-slot for it -- falls back to "not shifted" (the raw
/// match above stands).
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

/// Backlog #90 (S1), second half: a `:=` shift is only a *conflict* when
/// the bound consumer cannot be rebuilt -- real rebuilds a rebuildable
/// consumer in place (`R`, no conflict, no trailer) and only the
/// `--exclude` / `--useoldpkg-atoms` consumer is stuck (the two
/// need_rebuild-trailer reasons this pins; the masked-or-unavailable
/// ebuild reason stays the filed follow-up). Without this gate the
/// shift rule above would record a phantom conflict on every ordinary
/// sub-slotted update (mlocaml-4.02.1: the update IS the bound
/// version's move, mllablgl rebuilds), which backtracking then masks
/// into invisibility.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn built_equals_shift_unrebuildable(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    existing_sub: &str,
    pullers: &SlotPullers,
    excluded: &[String],
) -> bool {
    if !built_equals_shifted_slot(root, repos, category, package, existing_sub) {
        return false;
    }
    pullers
        .get(&(category.to_string(), package.to_string()))
        .is_some_and(|ps| {
            ps.iter().any(|(pcat, ppkg, pver, _)| {
                let cpv = format!("{pcat}/{ppkg}-{pver}");
                excluded
                    .iter()
                    .any(|ex| matches_config_entry(ex, &cpv, pcat, ppkg))
                    || useoldpkg_atom_matches(pcat, ppkg, pver)
            })
        })
}

/// One token of [`bind_slot_operator_deps`]. The rewrite is string
/// surgery on the atom's own slot-dep substring (`:=` / `:2=` /
/// `:2/3=`), the same reconstruction `ebuild_phases.rs::bind_slot_operator`
/// uses: that substring is distinctive enough to appear exactly once in a
/// well-formed atom, so `replacen(.., 1)` is safe (a version can't
/// contain `:`, a `::repo` carries no `=`, a `[usedep]` carries no `:`).
pub(crate) fn bind_slot_operator_token(token: &str, entries: &[GraphEntry], root: &Path) -> String {
    let Some(atom) = portage_dep::parse_atom(token) else {
        return token.to_string();
    };
    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
        return token.to_string();
    }
    let matches = |version: &str, slot: &str, sub_slot: &str| {
        let candidate = format!(
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

/// `match_from_list` against one candidate string -- the shape every
/// atom-vs-instance check below uses. Use-deps are not evaluated here
/// (the candidate string carries no USE state); a literal `[flag]` parent
/// atom the walk accepted therefore reads as matching, the same
/// direction `reverse_dep_constraint_atom` already documents for its own
/// use-dep cut.
pub(crate) fn atom_matches_str(atom: &str, candidate: &str) -> bool {
    portage_dep::match_from_list(atom, &[candidate]).is_some_and(|m| !m.is_empty())
}

/// Real `strip_libc_deps` (`portage/dep/libc.py`): remove every atom
/// whose `cat/pkg` is an installed libc provider from a rule-8 side.
/// Applied to the flattened set (real strips only each key's outer level;
/// the flat comparison has no levels left -- documented narrowing).
pub(crate) fn strip_libc_atoms(
    mut atoms: HashSet<String>,
    libc_cps: &HashSet<(String, String)>,
) -> HashSet<String> {
    if libc_cps.is_empty() {
        return atoms;
    }
    atoms.retain(|t| {
        portage_dep::parse_atom(t).is_none_or(|a| !libc_cps.contains(&(a.category, a.package)))
    });
    atoms
}

/// The tree ebuild's md5-cache metadata for `cp` at `version` -- the
/// source rule 8's *new* side is compared from (real
/// `pkgsettings.setcpv(pkg)` -> `configdict["pkg"]`). `None` when the
/// candidate or its metadata can't be read: the rebuild is kept, the
/// conservative direction.
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

/// Backlog #213 (v2 `#24d`): apply real's `prune_rebuilds` restart to
/// the working params (real `resolver/backtracking.py::_feedback_config`
/// `:247-257`): latch the flag, clear the whole replace set, and drop
/// the `slot_operator_mask_built` steers. Real pops that reason off the
/// runtime masks of the recorded packages and clears its own
/// `slot_operator_mask_built` set; portuale keeps no parallel set -- the
/// `MaskReason::SlotOperatorBuilt` negatives in `runtime_pkg_mask` ARE
/// the record, so removing them plus their `mask_order` slots is the
/// whole equivalent. The caller fires this once (the latch guards the
/// re-entry); the re-walk re-masks any still-stale binary through the
/// probe above, exactly like real's re-probe.
pub(crate) fn apply_prune_rebuilds(params: &mut BacktrackParams) {
    params.prune_rebuilds = true;
    params.slot_operator_replace_installed.clear();
    // The #236 A forcing half is real's replace-set probe upgrade: it goes
    // with the set (#295).
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
