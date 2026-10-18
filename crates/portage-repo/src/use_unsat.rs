//! Reports for dependencies whose USE requirements no candidate meets.

use crate::*;

pub(crate) struct UseUnsatParentRowArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) display_atom: &'a str,
    pub(crate) owner: Option<&'a (String, String)>,
    pub(crate) config: &'a portage_profile::Config,
}

/// The parent-conditional row: when a `[use]`-dep is `opt?`-conditional
/// on the requirer and the child's violation is entirely conditional,
/// the requirer is appended with its own `Change USE:` flip, shown after
/// the latest child row. `None` when no child yields a viable flip, the
/// parent state can't be read, or a flip is pinned by mask/force.
/// Portage: lib/_emerge/depgraph.py, _show_unsatisfied_dep
pub(crate) fn use_unsat_parent_row(
    args: UseUnsatParentRowArgs<'_>,
) -> Option<(String, Vec<String>)> {
    let UseUnsatParentRowArgs {
        repos,
        root,
        entries,
        display_atom,
        owner,
        config,
    } = args;
    let owner = owner?;
    let unevaluated = portage_dep::parse_atom(display_atom)?;
    // Merge-bound parents resolve through `parent_use_state`; installed
    // ones through the vdb-backed twin (display-only; the repair path
    // keeps the merge-bound shape).
    let (parent_candidate, parent_iuse, parent_use, parent_required_use) =
        parent_use_state(repos, entries, owner, config)
            .or_else(|| installed_parent_use_state(repos, root, entries, owner, config))?;
    let evaluated_atom = portage_dep::evaluate_atom_conditionals(display_atom, &parent_use)?;
    let evaluated = portage_dep::parse_atom(&evaluated_atom)?;
    let (_child, involved) = viable_parent_flip_targets(ViableParentFlipTargetsArgs {
        repos,
        unevaluated: &unevaluated,
        evaluated: &evaluated,
        unevaluated_atom: display_atom,
        parent_use: &parent_use,
        config,
    })
    .into_iter()
    .next()?;
    if involved.iter().any(|f| !parent_iuse.contains(f)) {
        return None;
    }
    let mut hypothetical = parent_use.as_ref().clone();
    let mut changes: Vec<String> = Vec::new();
    for flag in &involved {
        let desired = !parent_use.contains(flag);
        if !flag_is_settable(FlagIsSettableArgs {
            candidate: &parent_candidate,
            category: &owner.0,
            package: &owner.1,
            flag,
            desired,
            config,
        }) {
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

pub(crate) struct UseUnsatCandidatesForAtomArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) display_atom: &'a str,
    pub(crate) evaluated_atom: &'a str,
    pub(crate) owner: Option<&'a (String, String)>,
    pub(crate) config: &'a portage_profile::Config,
}

/// The "no ebuilds built with USE flags to satisfy" candidate scan:
/// the version/slot-matching visible ebuilds whose IUSE lacks a
/// required no-default flag (unconditional or conditional), or whose
/// effective USE cannot satisfy the unconditional use-deps. `None` when
/// the atom carries no use-deps or no visible candidate misses on USE.
///
/// Display order: when any candidate can be fixed by a `Change USE:`,
/// only the latest such candidate is listed, followed by the requirer's
/// own row when the dep is conditional; otherwise all `Missing IUSE:`
/// candidates print, in descending-version order.
///
/// Narrowing: a `Missing IUSE:` fallback that Portage suppresses via a
/// masked flag-carrying candidate can't be expressed by this scan.
/// Portage: lib/_emerge/depgraph.py, _show_unsatisfied_dep
pub(crate) fn use_unsat_candidates_for_atom(
    args: UseUnsatCandidatesForAtomArgs<'_>,
) -> Option<Vec<(String, Vec<String>)>> {
    let UseUnsatCandidatesForAtomArgs {
        repos,
        root,
        entries,
        display_atom,
        evaluated_atom,
        owner,
        config,
    } = args;
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
    // Version/slot filter, USE and visibility ignored (like Portage's
    // `db.match(atom.without_use)`).
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
    // Descending version (Portage's `cpv_list.reverse()`).
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
    // Every no-default token (conditionals included), in declaration
    // order; a default-marked token is not required (it has a fallback
    // when missing from IUSE).
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
    // The evaluated atom's own unconditional deps.
    let enabled = flags_of(&evaluated, portage_dep::UseDepOp::Enabled);
    let disabled = flags_of(&evaluated, portage_dep::UseDepOp::Disabled);

    let mut change_rows: Vec<(String, Vec<String>)> = Vec::new();
    let mut iuse_rows: Vec<(String, Vec<String>)> = Vec::new();
    for c in cands {
        // Masked instances never reach the displayed rows.
        if !is_visible(c, &display.category, &display.package, config) {
            continue;
        }
        let Some((declared, use_flags)) =
            candidate_iuse_and_use(c, &display.category, &display.package, config)
        else {
            continue;
        };
        let valid = valid_iuse(&declared, config);
        // Portage's `pkg.iuse.get_missing_iuse(required_flags)`.
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
        // `need_enable`/`need_disable`, sorted and restricted to flags
        // this candidate's own IUSE declares.
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
        // A flag pinned by this candidate's own `use.mask`/`use.force`
        // is not adjustable: no row, and the candidate stays out.
        let candidate_str = format!(
            "{}/{}-{}:{}/{}::{}",
            display.category, display.package, c.version, c.slot, c.sub_slot, c.repo_name
        );
        let eff_keywords = effective_keywords(
            config,
            &candidate_str,
            &display.category,
            &display.package,
            &c.keywords,
        );
        let stable = is_stable(IsStableArgs {
            keywords: &eff_keywords,
            candidate_str: &candidate_str,
            build_id: c.build_id.as_deref(),
            category: &display.category,
            package: &display.package,
            accept_keywords: &config.accept_keywords,
            package_accept_keywords: &config.package_accept_keywords,
        });
        // Owned union of the two shared sets (small on this path).
        let mut untouchable = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
            which: MaskOrForce::Mask,
            config,
            candidate_str: &candidate_str,
            build_id: c.build_id.as_deref(),
            category: &display.category,
            package: &display.package,
            stable,
        })
        .as_ref()
        .clone();
        untouchable.extend(
            resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
                which: MaskOrForce::Force,
                config,
                candidate_str: &candidate_str,
                build_id: c.build_id.as_deref(),
                category: &display.category,
                package: &display.package,
                stable,
            })
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
        // "Only show the latest version": the first (highest-version)
        // `Change USE:` candidate, then the requirer's row when conditional.
        let mut rows = vec![change_rows.remove(0)];
        if let Some(parent_row) = use_unsat_parent_row(UseUnsatParentRowArgs {
            repos,
            root,
            entries,
            display_atom,
            owner,
            config,
        }) {
            rows.push(parent_row);
        }
        return Some(rows);
    }
    if iuse_rows.is_empty() {
        return None;
    }
    Some(iuse_rows)
}
