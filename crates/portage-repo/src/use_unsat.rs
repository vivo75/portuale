//! Reports for dependencies whose USE requirements no candidate meets.

use crate::*;

/// Real `_show_unsatisfied_dep`'s parent-conditional row (`depgraph.py:
/// 6768-6858`): when a `[use]`-dep is `opt?`-conditional on the requirer
/// and the child's violation is entirely conditional, real appends the
/// *requirer* to `missing_use_reasons` with its own `Change USE:` flip,
/// and the display shows it after the latest child row. The involved
/// flags are real `violated_conditionals`' parent-side partition (the
/// same narrowing [`suggested_parent_use_candidate`] uses, via
/// [`viable_parent_flip_targets`]: only parent-active violated
/// conditionals of a version-matching visible child that passes the
/// untouchable-child `continue`), gated on the flip being settable and
/// not newly violating the parent's own REQUIRED_USE (real's warning
/// text is reproduced when it would). `None` when no child yields a
/// viable flip, the parent's state can't be read, or a flip is pinned
/// by `use.mask`/`use.force`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn use_unsat_parent_row(
    repos: &[RepoConfig],
    root: &Path,
    entries: &[GraphEntry],
    display_atom: &str,
    owner: Option<&(String, String)>,
    config: &portage_profile::Config,
) -> Option<(String, Vec<String>)> {
    let owner = owner?;
    let unevaluated = portage_dep::parse_atom(display_atom)?;
    // Merge-bound parents resolve through `parent_use_state`; installed
    // ones (the live `qtdeclarative` row, the r135 fixture) through the
    // vdb-backed twin -- the abort block is display-only, so the
    // autounmask repair path keeps the merge-bound shape.
    let (parent_candidate, parent_iuse, parent_use, parent_required_use) =
        parent_use_state(repos, entries, owner, config)
            .or_else(|| installed_parent_use_state(repos, root, entries, owner, config))?;
    let evaluated_atom = portage_dep::evaluate_atom_conditionals(display_atom, &parent_use)?;
    let evaluated = portage_dep::parse_atom(&evaluated_atom)?;
    let (_child, involved) = viable_parent_flip_targets(
        repos,
        &unevaluated,
        &evaluated,
        display_atom,
        &parent_use,
        config,
    )
    .into_iter()
    .next()?;
    if involved.iter().any(|f| !parent_iuse.contains(f)) {
        return None;
    }
    let mut hypothetical = parent_use.as_ref().clone();
    let mut changes: Vec<String> = Vec::new();
    for flag in &involved {
        let desired = !parent_use.contains(flag);
        if !flag_is_settable(&parent_candidate, &owner.0, &owner.1, flag, desired, config) {
            return None;
        }
        if desired {
            hypothetical.insert(flag.clone());
        } else {
            hypothetical.remove(flag);
        }
        changes.push(if desired {
            format!("+{flag}")
        } else {
            format!("-{flag}")
        });
    }
    let mut reason = format!("Change USE: {}", changes.join(" "));
    if let Some(required_use) = &parent_required_use
        && !required_use.trim().is_empty()
    {
        let old_sat =
            portage_required_use::check_required_use(required_use, &parent_use, &parent_iuse)
                .unwrap_or(false);
        let new_sat =
            portage_required_use::check_required_use(required_use, &hypothetical, &parent_iuse)
                .unwrap_or(false);
        if old_sat && !new_sat {
            reason.push_str(&format!(
                ", this change violates use flag constraints defined by {}/{}-{}: '{}'",
                owner.0, owner.1, parent_candidate.version, required_use
            ));
        }
    }
    Some((
        format!(
            "{}/{}-{}::{}",
            owner.0, owner.1, parent_candidate.version, parent_candidate.repo_name
        ),
        vec![reason],
    ))
}

/// Real `_show_unsatisfied_dep`'s "no ebuilds built with USE flags to
/// satisfy" candidate scan (`depgraph.py:6619` collects `missing_use`,
/// `:6714-6873` turns it into `unmasked_use_reasons` /
/// `unmasked_iuse_reasons`): the version/slot-matching, *visible* ebuilds
/// whose own IUSE is missing a required no-default flag (unconditional
/// *or* conditional -- real `use.required`, #265(a)), or whose
/// effective USE cannot satisfy the atom's unconditional use-deps.
/// `None` when the atom carries no use-deps or no visible candidate
/// misses on USE -- real's plain masked / "no ebuilds to satisfy" paths
/// own those.
///
/// Display order mirrors real exactly: when any candidate can be fixed by
/// a `Change USE:`, that path owns the block and only the **latest** such
/// candidate is listed ("Only show the latest version", `:6876-6890`),
/// followed by the requirer's own row when the dep is
/// `opt?`-conditional ([`use_unsat_parent_row`]); only when there is none
/// do all `Missing IUSE:` candidates print (`:6896-6907`), in
/// descending-version order.
///
/// Documented narrowings (same family as `masked_candidates_for_atom`'s):
/// the parent row is the all-conditional approximation
/// [`use_unsat_parent_row`] documents; real's `Missing IUSE:` fallback can
/// be suppressed by a *masked* candidate that has the flag
/// (`:6897-6907`), which this visibility-filtered scan cannot express.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn use_unsat_candidates_for_atom(
    repos: &[RepoConfig],
    root: &Path,
    entries: &[GraphEntry],
    display_atom: &str,
    evaluated_atom: &str,
    owner: Option<&(String, String)>,
    config: &portage_profile::Config,
) -> Option<Vec<(String, Vec<String>)>> {
    let display = portage_dep::parse_atom(display_atom)?;
    display.use_deps.as_deref().filter(|d| !d.is_empty())?;
    let evaluated = portage_dep::parse_atom(evaluated_atom)?;
    let candidates = list_candidates(repos, &display.category, &display.package).ok()?;

    let strs: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                display.category, display.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    // Real `db.match(atom.without_use)`: version/slot filter, USE and
    // visibility ignored.
    let matched: std::collections::HashSet<&str> =
        portage_dep::match_from_list(portage_dep::without_use(display_atom), &refs)?
            .into_iter()
            .collect();
    let mut cands: Vec<&Candidate> = candidates
        .iter()
        .zip(strs.iter())
        .filter(|(_, s)| matched.contains(s.as_str()))
        .map(|(c, _)| c)
        .collect();
    // Real `cpv_list.reverse()` -> descending version.
    cands.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));

    let flags_of = |atom: &portage_dep::Atom, op: portage_dep::UseDepOp| -> Vec<String> {
        atom.use_deps
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|d| d.op == op)
            .map(|d| d.flag.clone())
            .collect()
    };
    // Real `atom.unevaluated_atom.use.required` -- `no_default`, i.e.
    // every token *without* a `(+)`/`(-)` default marker, conditionals
    // (`flag?`, `flag=`, `!flag?`, …) included, in declaration order
    // here (`lib/portage/dep/__init__.py:1363` `self.required =
    // frozenset(no_default)`; backlog #265(a)). A default-marked token
    // is not a `required` flag in real's parser (it has a fallback when
    // missing from IUSE).
    let required_of = |atom: &portage_dep::Atom| -> Vec<String> {
        atom.use_deps
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|d| d.default.is_none())
            .map(|d| d.flag.clone())
            .collect()
    };
    let required = required_of(&display);
    // Real `atom.use.enabled` / `.disabled` -- the *evaluated* atom's own
    // unconditional deps.
    let enabled = flags_of(&evaluated, portage_dep::UseDepOp::Enabled);
    let disabled = flags_of(&evaluated, portage_dep::UseDepOp::Disabled);

    let mut change_rows: Vec<(String, Vec<String>)> = Vec::new();
    let mut iuse_rows: Vec<(String, Vec<String>)> = Vec::new();
    for c in cands {
        // Real `unmasked_use_reasons` / `unmasked_iuse_reasons`: masked
        // instances never reach the displayed rows.
        if !is_visible(c, &display.category, &display.package, config) {
            continue;
        }
        let Some((declared, use_flags)) =
            candidate_iuse_and_use(c, &display.category, &display.package, config)
        else {
            continue;
        };
        let valid = valid_iuse(&declared, config);
        // Real `pkg.iuse.get_missing_iuse(required_flags)`.
        let missing: Vec<String> = required
            .iter()
            .filter(|f| !valid.contains(*f))
            .cloned()
            .collect();
        let output_cpv = format!(
            "{}/{}-{}::{}",
            display.category, display.package, c.version, c.repo_name
        );
        if !missing.is_empty() {
            iuse_rows.push((
                output_cpv,
                vec![format!("Missing IUSE: {}", missing.join(" "))],
            ));
            continue;
        }
        // Real `need_enable`/`need_disable` (`:6729-6730`), sorted and
        // restricted to flags this candidate's own IUSE declares.
        let mut need_enable: Vec<String> = enabled
            .iter()
            .filter(|f| !use_flags.contains(*f) && valid.contains(*f))
            .cloned()
            .collect();
        let mut need_disable: Vec<String> = disabled
            .iter()
            .filter(|f| use_flags.contains(*f) && valid.contains(*f))
            .cloned()
            .collect();
        need_enable.sort();
        need_enable.dedup();
        need_disable.sort();
        need_disable.dedup();
        if need_enable.is_empty() && need_disable.is_empty() {
            continue;
        }
        // Real `:6732-6736`: a flag this candidate's own `use.mask`/
        // `use.force` pins is not adjustable -- no row at all, and the
        // candidate also stays out of `missing_use_adjustable`.
        let candidate_str = format!(
            "{}/{}-{}:{}/{}::{}",
            display.category, display.package, c.version, c.slot, c.sub_slot, c.repo_name
        );
        let stable = is_stable(
            &c.keywords,
            &candidate_str,
            &display.category,
            &display.package,
            &config.accept_keywords,
            &config.package_accept_keywords,
        );
        // Owned union of the two shared sets (display path; the union
        // is small).
        let mut untouchable = resolved_use_mask_or_force(
            MaskOrForce::Mask,
            config,
            &candidate_str,
            &display.category,
            &display.package,
            stable,
        )
        .as_ref()
        .clone();
        untouchable.extend(
            resolved_use_mask_or_force(
                MaskOrForce::Force,
                config,
                &candidate_str,
                &display.category,
                &display.package,
                stable,
            )
            .iter()
            .cloned(),
        );
        if need_enable
            .iter()
            .chain(need_disable.iter())
            .any(|f| untouchable.contains(f))
        {
            continue;
        }
        let mut changes: Vec<String> = need_enable.iter().map(|f| format!("+{f}")).collect();
        changes.extend(need_disable.iter().map(|f| format!("-{f}")));
        change_rows.push((
            output_cpv,
            vec![format!("Change USE: {}", changes.join(" "))],
        ));
    }
    if !change_rows.is_empty() {
        // Real `:6877-6887`: "Only show the latest version" -- the first
        // (highest-version) unmasked `Change USE:` candidate, then the
        // requirer's own row when the dep is conditional.
        let mut rows = vec![change_rows.remove(0)];
        if let Some(parent_row) =
            use_unsat_parent_row(repos, root, entries, display_atom, owner, config)
        {
            rows.push(parent_row);
        }
        return Some(rows);
    }
    if iuse_rows.is_empty() {
        return None;
    }
    Some(iuse_rows)
}
