//! USE-flip suggestions for autounmask.

use crate::*;

pub(crate) struct FlagIsSettableArgs<'a> {
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) flag: &'a str,
    pub(crate) desired: bool,
    pub(crate) config: &'a portage_profile::Config,
}

/// Whether `flag` can actually be forced to `desired` via a
/// `package.use` entry: mask/force always override `package.use`, so a
/// masked/forced flag is never settable. Probed by recomputing
/// `effective_use_flags` with a synthetic maximally-specific entry and
/// checking the result reflects `desired`.
/// Portage: _wrapped_select_pkg_highest_available_imp, can_adjust_use
pub(crate) fn flag_is_settable(args: FlagIsSettableArgs<'_>) -> bool {
    let FlagIsSettableArgs {
        candidate,
        category,
        package,
        flag,
        desired,
        config,
    } = args;
    let pf = format!("{package}-{}", candidate.version);
    let Ok(metadata) = repo_aux_metadata(&candidate.repo_location, category, &pf) else {
        return false;
    };
    let iuse = metadata.get("IUSE").map(String::as_str).unwrap_or_default();
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let synthetic_token = if desired {
        flag.to_string()
    } else {
        format!("-{flag}")
    };
    // A plain `=category/package-version` atom: `candidate_str` itself
    // carries a `:slot/subslot::repo` suffix no atom pattern parses.
    let synthetic_atom = format!("={category}/{package}-{}", candidate.version);
    // The synthetic entry joins the `autounmask_use` tier (top): a
    // mask/force override discards it, which is what makes the flag
    // "not settable".
    let mut probe = config.clone();
    probe
        .autounmask_use
        .push((synthetic_atom, vec![synthetic_token]));
    let use_flags = effective_use_flags(EffectiveUseFlagsArgs {
        config: &probe,
        iuse,
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
    });
    use_flags.contains(flag) == desired
}

/// The best `--autounmask-use` flag-flip suggestion for `candidate`
/// against `use_deps` (already conditional-evaluated: only plain
/// `flag`/`-flag` forms are consulted). `None` when nothing needs to
/// change, a needed flag isn't in the candidate's IUSE at all, or any
/// needed change is blocked by `flag_is_settable` (a partially-fixable
/// atom suggests nothing, rather than a change that wouldn't resolve).
/// Portage: can_adjust_use (whole-atom rejection)
pub(crate) fn suggested_use_flip(
    candidate: &Candidate,
    category: &str,
    package: &str,
    use_deps: &[portage_dep::UseDep],
    config: &portage_profile::Config,
) -> Option<Vec<(String, bool)>> {
    let (iuse, use_flags) = candidate_iuse_and_use(candidate, category, package, config)?;
    let mut changes = Vec::new();
    for ud in use_deps {
        let desired = match ud.op {
            portage_dep::UseDepOp::Enabled => true,
            portage_dep::UseDepOp::Disabled => false,
            // Conditional forms were evaluated away before the atom was
            // ever queued (see `enqueue_flat_deps`); the parent-side
            // mechanism below is a separate path.
            _ => continue,
        };
        if !iuse.contains(&ud.flag) {
            // No `package.use` entry could ever set an undeclared flag --
            // but a matching `(+)`/`(-)` default already satisfies the
            // requirement; only a genuinely unsatisfiable missing flag
            // makes the whole atom unfixable.
            match (&ud.op, &ud.default) {
                (portage_dep::UseDepOp::Enabled, Some(portage_dep::UseDepDefault::Enabled))
                | (portage_dep::UseDepOp::Disabled, Some(portage_dep::UseDepDefault::Disabled)) => {
                    continue;
                }
                _ => return None,
            }
        }
        let currently_enabled = use_flags.contains(&ud.flag);
        if currently_enabled != desired {
            changes.push((ud.flag.clone(), desired));
        }
    }
    if changes.is_empty() {
        return None;
    }
    if changes.iter().any(|(flag, desired)| {
        !flag_is_settable(FlagIsSettableArgs {
            candidate,
            category,
            package,
            flag,
            desired: *desired,
            config,
        })
    }) {
        return None;
    }
    changes.sort();
    Some(changes)
}

/// The best `--autounmask-use` suggestion for `category/package`
/// against `use_deps`, if any: among the candidates mismatched on plain
/// USE-deps alone, the highest-versioned one with its `suggested_use_flip`.
/// Mirrors `suggested_keyword_candidate`'s "best near-miss" shape.
pub(crate) fn suggested_use_candidate(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    use_deps: Option<&[portage_dep::UseDep]>,
    config: &portage_profile::Config,
) -> Option<(String, Vec<(String, bool)>)> {
    let use_deps = use_deps.filter(|ud| !ud.is_empty())?;
    let candidates = list_candidates(repos, category, package).ok()?;
    candidates
        .iter()
        .filter(|c| use_masked_only(c, category, package, use_deps, config))
        .filter_map(|c| suggested_use_flip(c, category, package, use_deps, config).map(|f| (c, f)))
        .max_by(|(a, _), (b, _)| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
        .map(|(c, flip)| (c.version.clone(), flip))
}

/// A package's declared `IUSE` (markers stripped) folded together with
/// the arch list, `use.mask ∪ use.force`, the literal
/// `build`/`bootstrap` flags and `iuse_effective`: the full domain for
/// contextual flag validity (wider than the `use_deps_satisfied` subset).
/// Portage: config.py, _get_implicit_iuse
pub(crate) fn implicit_iuse_set(iuse: &str, config: &portage_profile::Config) -> HashSet<String> {
    let mut iuse_set: HashSet<String> = iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    iuse_set.extend(config.archlist.iter().cloned());
    iuse_set.extend(config.use_mask.iter().cloned());
    iuse_set.extend(config.use_force.iter().cloned());
    iuse_set.insert("build".to_string());
    iuse_set.insert("bootstrap".to_string());
    // EAPI 5+ `check_required_use` also accepts `USE_EXPAND_IMPLICIT`
    // flags (`Config::iuse_effective`) the same way.
    iuse_set.extend(config.iuse_effective.iter().cloned());
    iuse_set
}

/// The parent-side `--autounmask-use` mechanism: unlike
/// `suggested_use_candidate` (which flips the candidate's flag), this
/// flips the requesting parent's flag for atoms whose use-dep was
/// originally conditional on the parent's USE (already evaluated away by
/// `enqueue_flat_deps`): would the re-evaluated atom resolve under the
/// flip?
///
/// Only parent-active violated conditionals count ([`violated_parent_flags`]).
/// Gated on: valid parent IUSE, no mask/force pin, satisfiable under the
/// flip, and no new REQUIRED_USE violation (an old one doesn't disqualify).
/// First viable candidate in descending version order wins.
/// Portage: lib/_emerge/depgraph.py, _show_unsatisfied_dep
///
/// Attached to the dependency's own `GraphEntry` (`parent_use_suggestion`)
/// rather than the parent's entry (no per-parent reasons list exists).
/// `(parent_category, parent_package, parent_version, [(flag, desired)])`.
pub type ParentUseSuggestion = (String, String, String, Vec<(String, bool)>);
