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
