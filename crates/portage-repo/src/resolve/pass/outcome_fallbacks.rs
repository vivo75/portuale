//! The installed fallbacks and reinstall flips applied to a resolved outcome.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`apply_outcome_fallbacks`] reads from its caller.
pub(crate) struct ApplyOutcomeFallbacksArgs<'a> {
    pub(crate) outcome: PretendOutcome,
    pub(crate) current_atom: &'a str,
    pub(crate) atom: &'a Atom,
    pub(crate) key: &'a (String, String),
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) state: &'a mut PassState,
    pub(crate) atom_root: &'a Path,
    pub(crate) item_targets_running_root: bool,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) depth: u32,
    pub(crate) buildtime_hard: bool,
    pub(crate) from_disjunction: bool,
}

/// The installed fallbacks and reinstall flips applied to a resolved outcome.
///
/// A slot-operator miss with an installed satisfier settles `AlreadyInstalled`;
/// the reinstall flips then force the re-merge verdicts. Keyed on `cat/pkg`,
/// so whichever visit comes first flips.
pub(crate) fn apply_outcome_fallbacks(
    args: ApplyOutcomeFallbacksArgs<'_>,
) -> Result<PretendOutcome, Error> {
    let ApplyOutcomeFallbacksArgs {
        mut outcome,
        current_atom,
        atom,
        key,
        ctx,
        config,
        bp,
        union_constraints,
        state,
        atom_root,
        item_targets_running_root,
        owner,
        depth,
        buildtime_hard,
        from_disjunction,
    } = args;
    // An argument-pinned installed instance satisfying this atom reuses it
    // instead of scheduling a merge (fresh verdicts only, never `||`-chosen).
    // See `arg_pinned_installed_reuse`.
    if !from_disjunction
        && let Some(fresh) = match &outcome {
            PretendOutcome::New { version } => Some(version.clone()),
            PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
            _ => None,
        }
        && let Some(kept) = arg_pinned_installed_reuse(ArgPinnedInstalledReuseArgs {
            ctx,
            config,
            bp,
            union_constraints,
            cache: &mut state.top_pinned_installed,
            atom_root,
            key,
            current_atom,
            atom,
            fresh_version: &fresh,
        })
    {
        outcome = PretendOutcome::AlreadyInstalled { version: kept };
    }

    // An installed node an earlier dep already put in the graph, in the fresh
    // pick's slot and accepted by this atom, is reused too (any dep, `||`-chosen
    // included): Portage's `find_existing_node` pass.
    if let Some(fresh) = match &outcome {
        PretendOutcome::New { version } => Some(version.clone()),
        PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
        PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
        _ => None,
    } && let Some(kept) = graph_installed_reuse(GraphInstalledReuseArgs {
        ctx,
        config,
        state: &*state,
        atom_root,
        key,
        current_atom,
        atom,
        fresh_version: &fresh,
        item_targets_running_root,
    }) {
        outcome = PretendOutcome::AlreadyInstalled { version: kept };
    }

    // A dependency no ebuild or binary satisfies but an installed instance
    // does settles `AlreadyInstalled`, never a miss (Portage iterates the
    // installed db on every resolve). Slot and slot-operator atoms keep the
    // long-standing rule (an installed match suffices: #233's probe has a
    // stale `:0/1` matching the installed instance in vartree); bare and
    // version-range atoms additionally require the cp to have no tree or
    // binary candidate at all, so a package whose ebuilds are all masked
    // keeps its masked-package diagnostics. Scoped to genuine dependency
    // edges; `||`-chosen edges keep their miss (the disjunction layer
    // classifies installed-ness itself). Build-time-only edges satisfy here
    // too (Portage consults the installed db for every dep kind: the S0
    // `s0-e-gbapp-pvuD` capture keeps installed-only `pg34-gone` for a
    // `BDEPEND` edge). Match requires the installed USE to satisfy the
    // atom's use-deps, and reads this atom's own resolving root.
    // Portage: depgraph.py, _wrapped_select_pkg_highest_available_imp.
    if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && owner.is_some()
        && depth != 0
        && !from_disjunction
        && !ctx.empty
        && (atom.slot.is_some()
            || atom.slot_operator.is_some()
            || !cp_has_tree_or_binary_candidate(ctx.gate(), &ctx.local_binpkg, &key.0, &key.1))
        && let Some(version) = best_installed_matching(atom_root, current_atom, config)
            .filter(|v| !installed_version_masked(&bp.masked_negatives_for(key), &key.0, &key.1, v))
    {
        outcome = PretendOutcome::AlreadyInstalled { version };
    }
    // Portage ignores visibility when every match is masked
    // (`_select_pkg_highest_available_imp`, `depgraph.py:8525-8532`: "all
    // are masked, so ignore visibility"): a bare atom matching an
    // installed instance keeps it even when the tree copies are all
    // masked. This covers `||`-chosen edges too -- their disjunction
    // already selected this alternative, so the miss must not stand and
    // the MissingDep trigger must not fire (backlog #615: `-uD[N]
    // dev-libs/omasked` merges `omasked-1.0` on host Portage with an
    // installed-masked warning, while portuale aborted rc 1 "All ebuilds
    // ... have been masked"). Version/slot atoms keep the rule above
    // (no tree candidate at all), so masked-package diagnostics still
    // render for those.
    // Portage: depgraph.py, _wrapped_select_pkg_highest_available_imp.
    if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && owner.is_some()
        && depth != 0
        && !ctx.empty
        && atom.slot.is_none()
        && atom.slot_operator.is_none()
        && atom.version.is_none()
        && cp_has_tree_or_binary_candidate(ctx.gate(), &ctx.local_binpkg, &key.0, &key.1)
        && !cp_has_visible_candidate(ctx.gate(), &ctx.local_binpkg, &key.0, &key.1, config)
        && let Some(version) = best_installed_matching(atom_root, current_atom, config)
            .filter(|v| !installed_version_masked(&bp.masked_negatives_for(key), &key.0, &key.1, v))
    {
        outcome = PretendOutcome::AlreadyInstalled { version };
    }
    // A top-level argument under `--update --selective` with no tree or
    // binary candidate at all but an installed satisfier is already
    // installed (`Total: 0`); without `--update` (or with `--selective=n`)
    // it stays a miss (Portage wants the reinstall and fails: the S0-backed
    // `360-gone-pv` capture).
    // Portage: depgraph.py, select_files argument handling.
    if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && owner.is_none()
        && depth == 0
        && !from_disjunction
        && ctx.update
        && ctx.selective
        && !ctx.empty
        && !cp_has_tree_or_binary_candidate(ctx.gate(), &ctx.local_binpkg, &key.0, &key.1)
        && let Some(version) = best_installed_matching(atom_root, current_atom, config)
            .filter(|v| !installed_version_masked(&bp.masked_negatives_for(key), &key.0, &key.1, v))
    {
        outcome = PretendOutcome::AlreadyInstalled { version };
    }
    // A running-root dep no ebuild satisfies but the running root has
    // installed settles `AlreadyInstalled` (Portage resolves those groups
    // against the running root's installed db). Loosens both gates above
    // (`buildtime_hard`, slot-operator-only); `||` and `--emptytree` stay out.
    if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && owner.is_some()
        && depth != 0
        && !from_disjunction
        && !ctx.empty
        && item_targets_running_root
        && let Some(version) = best_installed_matching(atom_root, current_atom, config)
            .filter(|v| !installed_version_masked(&bp.masked_negatives_for(key), &key.0, &key.1, v))
    {
        outcome = PretendOutcome::AlreadyInstalled { version };
    }

    // `--reinstall-atoms` forces a matching installed package to re-merge
    // (Portage drops it from every `inst_pkgs` list): the `--emptytree`
    // rewrite, scoped to the matched atom.
    // The same flip point carries the auto slot-operator `force_reinstall`:
    // keyed on `cat/pkg` so whichever visit comes first flips (patching the
    // entry afterwards would leave its deps unenqueued).
    // Portage: depgraph.py, _gen_reinstall_sets.
    // An installed child the update probe forced is never kept: the first visit
    // selects the probe's candidate (like `force_reinstall` excluding the
    // installed instance).
    if let PretendOutcome::AlreadyInstalled { version } = &outcome
        && let Some((_, _, target)) = bp
            .slot_operator_forced_upgrade
            .iter()
            .find(|(c, p, _)| *c == key.0 && *p == key.1)
        && target != version
    {
        let forced = resolve_queue_atom(ResolveQueueAtomArgs {
            ctx,
            config,
            bp,
            depth,
            union_constraints,
            key,
            atom_text: &format!("={}/{}-{target}", key.0, key.1),
            running: item_targets_running_root,
            buildtime_hard,
        })?;
        if !matches!(forced, PretendOutcome::NoVisibleCandidate) {
            outcome = forced;
        }
    }
    // A rebuilt package selected like an argument (#728) may come back as a plain
    // reinstall of the best version: it is still the slot-operator rebuild.
    if bp.slot_operator_replace_installed.contains(key)
        && let PretendOutcome::Reinstall {
            slot_operator_rebuild,
            ..
        } = &mut outcome
    {
        *slot_operator_rebuild = true;
    }
    if let PretendOutcome::AlreadyInstalled { version } = &outcome {
        let cpv = format!("{}/{}-{version}", key.0, key.1);
        if ctx
            .reinstall_atoms
            .iter()
            .any(|a| matches_cli_option_atom(a, &cpv))
        {
            outcome = PretendOutcome::Reinstall {
                version: version.clone(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            };
        } else if bp.slot_operator_replace_installed.contains(key) {
            outcome = PretendOutcome::Reinstall {
                version: version.clone(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: true,
            };
        } else if bp.rebuild_if_replace_installed.contains(key) {
            // The `--rebuild-if-*` restart seed: a reasonless reinstall, not
            // a slot-operator one (the red `r` comes from the restart
            // record, never `slot_operator_rebuild`).
            // Portage: depgraph.py, _gen_reinstall_sets (`__auto_rebuild__`).
            outcome = PretendOutcome::Reinstall {
                version: version.clone(),
                changed_flags: Vec::new(),
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
                slot_operator_rebuild: false,
            };
        }
    }
    Ok(outcome)
}
