//! USE-flip suggestions for autounmask.

use crate::*;

/// Whether `flag` can actually be forced to `desired` via a
/// `package.use` entry for `candidate` -- real `pkg.use.mask`/`pkg.use.
/// force` (global `use.mask`/`use.force` folded in) always override
/// `package.use` regardless of what it says (real `_wrapped_select_pkg_
/// highest_available_imp`'s own `can_adjust_use` hard block), so a
/// masked/forced flag can never really be "fixed" this way. Rather than
/// re-deriving `use.mask`/`.force`/`package.use.mask`/`.force` matching
/// logic separately (a second copy of what `effective_use_flags`
/// already does), this recomputes `effective_use_flags` with a
/// synthetic, maximally-specific `package.use` entry for this exact
/// candidate appended and checks whether the result actually reflects
/// `desired` -- if mask/force override it, the synthetic entry's own
/// effect is silently discarded the same way a real one would be.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn flag_is_settable(
    candidate: &Candidate,
    category: &str,
    package: &str,
    flag: &str,
    desired: bool,
    config: &portage_profile::Config,
) -> bool {
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
    // A real, plain `=category/package-version` atom -- not
    // `candidate_str` itself, which carries a `:slot/subslot::repo`
    // suffix `match_from_list` can't parse back as an atom pattern (it
    // expects an atom on the left, a candidate string on the right, not
    // a fully-qualified candidate string used as both).
    let synthetic_atom = format!("={category}/{package}-{}", candidate.version);
    // The synthetic entry stands in for the hypothetical autounmask
    // change itself, so it joins the `autounmask_use` tier (top) -- if
    // `use.mask`/`.force` still override it, the synthetic entry's effect
    // is silently discarded the same way a real autounmask change would
    // be, which is exactly what makes the flag "not settable".
    let mut probe = config.clone();
    probe
        .autounmask_use
        .push((synthetic_atom, vec![synthetic_token]));
    let use_flags = effective_use_flags(
        &probe,
        iuse,
        &candidate.keywords,
        &candidate_str,
        category,
        package,
    );
    use_flags.contains(flag) == desired
}

/// The best `--autounmask-use` flag-flip suggestion for `candidate`
/// against `use_deps` (an atom's own use-dep spec, already conditional-
/// evaluated -- only plain `flag`/`-flag` (`UseDepOp::Enabled`/
/// `Disabled`) forms are ever consulted, matching what
/// `use_deps_satisfied` itself checks). `None` when nothing needs to
/// change, when a needed flag isn't even in the candidate's own IUSE at
/// all (real "flag not in IUSE" unfixability -- no `package.use` entry
/// could address it), or when any needed change is blocked by
/// `flag_is_settable` -- real portage's own "only suggest a change that
/// would actually fix it" spirit again: a partially-fixable atom (some
/// flags adjustable, one masked/forced) suggests nothing at all rather
/// than a change that wouldn't actually resolve the mismatch, mirroring
/// real `can_adjust_use`'s own whole-atom rejection.
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
            // Conditional forms (flag?/!flag?/flag=/!flag=) are already
            // evaluated away into concrete Enabled/Disabled (or dropped
            // entirely) before an atom is ever queued -- see
            // `enqueue_flat_deps`'s own doc comment. Real portage's own
            // separate opt?-driven suggestion mechanism (a *parent's*
            // own flag, not the candidate's) is a different code path
            // entirely -- see this module's own doc comment.
            _ => continue,
        };
        if !iuse.contains(&ud.flag) {
            // The flag isn't declared by this package, so no `package.use`
            // entry could ever set it -- but a `(+)`/`(-)` default stands
            // in for its state (real `_use_dep`'s "missing" handling, same
            // as `use_deps_satisfied`): `[flag(+)]` is already satisfied
            // when the requirement is "enabled", `[flag(-)]` when it's
            // "disabled". Only a genuinely unsatisfiable missing flag (no
            // default, or a default that contradicts the requirement) makes
            // the whole atom unfixable.
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
        !flag_is_settable(candidate, category, package, flag, *desired, config)
    }) {
        return None;
    }
    changes.sort();
    Some(changes)
}

/// The best `--autounmask-use` suggestion for `category/package` against
/// `use_deps`, if any: among every candidate masked by a plain USE-dep
/// mismatch alone (`use_masked_only`), the highest-versioned one (repo
/// priority breaking a tie), paired with its own `suggested_use_flip`.
/// `None` when `use_deps` is empty/absent (nothing to suggest fixing),
/// `category/package` isn't listable at all, or no candidate qualifies.
/// Mirrors `suggested_keyword_candidate`'s own "best near-miss" shape
/// exactly, shared by the same two call sites (a top-level atom's own
/// fatal `NoVisibleCandidate` and a dependency's own, see
/// `GraphEntry::use_suggestion`'s own doc comment).
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

/// Real `config.py`'s own `_get_implicit_iuse()`: a package's own
/// declared `IUSE` (default markers stripped) folded together with
/// `PORTAGE_ARCHLIST` (`profiles/arch.list`), `use.mask ∪ use.force`,
/// and the literal `build`/`bootstrap` flags -- real `pkg.iuse.
/// is_valid_flag`'s own full domain, not a package's own literal `IUSE`
/// alone. Without this, a `REQUIRED_USE` (or, here, a conditional
/// use-dep) referencing an implicit flag never mentioned in a package's
/// own `IUSE` (e.g. real `media-libs/mesa`'s own `REQUIRED_USE`
/// referencing `"x86"`) spuriously reports "not in IUSE" -- confirmed
/// live against the real, installed system. See `portage_profile::
/// Config::archlist`'s own doc comment for the full grounding.
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
    // Real EAPI 5+ `check_required_use` is also called with
    // `pkg.iuse.is_valid_flag`, which for EAPI 5+ is `explicit ∪
    // IUSE_EFFECTIVE` -- so a `REQUIRED_USE` that references an
    // `elibc_*`/`kernel_*`/... implicit flag (via `USE_EXPAND_IMPLICIT`,
    // `Config::iuse_effective`) is recognized as valid the same way one
    // referencing `x86` (via `archlist`) already is. `valid_iuse` (used
    // for `use_deps_satisfied`) is the narrower `declared ∪
    // iuse_effective` subset of this; this pragmatic superset stays for
    // the `REQUIRED_USE`/parent-USE-state path.
    iuse_set.extend(config.iuse_effective.iter().cloned());
    iuse_set
}

/// The requesting parent's own current resolved `Candidate`, IUSE
/// (implicit-folded, see `implicit_iuse_set`), and effective USE --
/// looked up via its own already-resolved entry in `entries`. The parent
/// is always already present there by the time any of its own
/// dependencies are dequeued (BFS processes a package's own entry before
/// ever enqueueing its dependencies). `None` when the parent isn't found,
/// has no version to look up by (`AlreadyInstalled`/`NoVisibleCandidate`
/// -- moot anyway, since `enqueue_dependencies`'s own `AlreadyInstalled`
/// recursion path never conditional-evaluates deps at all, so `--
/// autounmask-use`'s own `opt?` mechanism never triggers for it), or its
/// own metadata can't be read.
/// `(candidate, implicit_iuse, effective_use, REQUIRED_USE)`.
pub(crate) type ParentUseState = (
    Candidate,
    HashSet<String>,
    Rc<HashSet<String>>,
    Option<String>,
);

pub(crate) fn parent_use_state(
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    owner: &(String, String),
    config: &portage_profile::Config,
) -> Option<ParentUseState> {
    let parent_entry = entries
        .iter()
        .find(|e| e.category == owner.0 && e.package == owner.1)?;
    let version: String = match &parent_entry.outcome {
        PretendOutcome::New { version } => version,
        PretendOutcome::Upgrade { to, .. } => to,
        PretendOutcome::Downgrade { to, .. } => to,
        PretendOutcome::Reinstall { version, .. } => version,
        _ => return None,
    }
    .clone();
    let candidates = list_candidates(repos, &owner.0, &owner.1).ok()?;
    let resolved = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)?
        .clone();
    let (_iuse, use_flags) = candidate_iuse_and_use(&resolved, &owner.0, &owner.1, config)?;
    let pf = format!("{}-{version}", owner.1);
    let metadata = repo_aux_metadata(&resolved.repo_location, &owner.0, &pf).ok()?;
    let full_iuse = implicit_iuse_set(
        metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
        config,
    );
    Some((
        resolved,
        full_iuse,
        use_flags,
        metadata.get("REQUIRED_USE").cloned(),
    ))
}

/// Real `_show_unsatisfied_dep`'s parent-conditional row for an
/// *installed* requesting parent (`depgraph.py:6768-6858` reads
/// `self._pkg_use_enabled(myparent)` and `myparent.use.mask/.force`
/// with no installed/merge-bound distinction -- the live `qtdeclarative`
/// row is `[installed]`). `parent_use_state` covers merge-bound parents
/// only (its version lookup has no `AlreadyInstalled` arm); this is the
/// installed twin, used as a fallback by [`use_unsat_parent_row`] alone.
/// The abort block is the only consumer: the `--autounmask-use` repair
/// path (`suggested_parent_use_candidate` and its walk-time caller) keeps
/// `parent_use_state`'s merge-bound-only shape, so installed parents gain
/// the display row without gaining a repair attempt.
///
/// State sources mirror real's installed instance: the flip base is the
/// vdb-recorded `USE` (real `_pkg_use_enabled` for a built package), the
/// IUSE fold is the vdb `IUSE`, and the repo candidate at the same
/// version supplies the `Candidate` `flag_is_settable` needs plus the row
/// cpv/repo and `REQUIRED_USE` (the fixture vdb carries no
/// `REQUIRED_USE` file; same-version ebuild metadata is what a real vdb
/// entry built from that ebuild records). `None` when the entry isn't
/// installed, the vdb record is absent, or no same-version repo
/// candidate exists (without it neither mask/force nor the row cpv can
/// be grounded -- real would still print the row from the instance, a
/// documented narrowing).
pub(crate) fn installed_parent_use_state(
    repos: &[RepoConfig],
    root: &Path,
    entries: &[GraphEntry],
    owner: &(String, String),
    config: &portage_profile::Config,
) -> Option<ParentUseState> {
    let parent_entry = entries
        .iter()
        .find(|e| e.category == owner.0 && e.package == owner.1)?;
    let PretendOutcome::AlreadyInstalled { version } = &parent_entry.outcome else {
        return None;
    };
    if !vdb_entry_is_dir(root, &owner.0, &owner.1, version) {
        return None;
    }
    let candidates = list_candidates(repos, &owner.0, &owner.1).ok()?;
    let resolved = candidates
        .iter()
        .filter(|c| c.version == *version)
        .max_by_key(|c| c.repo_priority)?
        .clone();
    let vdb_iuse = read_vdb_flag_set(root, &owner.0, &owner.1, version, "IUSE");
    let mut iuse_line: Vec<String> = vdb_iuse.into_iter().collect();
    iuse_line.sort();
    let full_iuse = implicit_iuse_set(&iuse_line.join(" "), config);
    let use_flags: Rc<HashSet<String>> =
        Rc::new(read_vdb_flag_set(root, &owner.0, &owner.1, version, "USE"));
    let pf = format!("{}-{version}", owner.1);
    let metadata = repo_aux_metadata(&resolved.repo_location, &owner.0, &pf).ok()?;
    Some((
        resolved,
        full_iuse,
        use_flags,
        metadata.get("REQUIRED_USE").cloned(),
    ))
}

/// Real `Atom.violated_conditionals(child_use, child_is_valid_flag,
/// parent_use)` (`lib/portage/dep/__init__.py:1465`), narrowed to the
/// parent-conditional arm's question (`lib/_emerge/depgraph.py:
/// 6779-6798`): which of the unevaluated atom's conditional use-deps are
/// violated *by the parent's current USE* — i.e. flipping the parent's
/// flag changes the evaluation — given the child's IUSE-filtered
/// effective USE. Real `Package.py:728-731` filters `pkg.use.enabled`
/// to valid IUSE, so a profile flag the child never declares (the
/// fixture profile's `foo` on IUSE-less `aup0d`) never counts as set:
/// pass `child_use` already intersected with `child_valid`.
///
/// Per operator, for parent-has `P` and child-has `C`, child-IUSE-valid
/// `V`, `(+)`-defaulted `E` and `(-)`-defaulted `D` (oracle:
/// `LLM/evidence/2026-09-28-g195/g195d-oracle-output.txt`, from
/// `g195d-oracle-probe.py` over real 3.0.82.2; pinned cell-by-cell by
/// `tests_195d`):
/// * `flag?` (`IfParentEnabled`, `:1525-1531`): violated iff the parent
///   has the flag, the child lacks it, and the flag is valid or
///   `(-)`-assumed — an invalid `(+)` flag is satisfied there (its
///   default already assumes it enabled, so the parent having it on
///   changes nothing).
/// * `flag=` (`EqualParent`, `:1532-1549`): parent-has-child-lacks
///   violates for a valid or `(-)` flag; parent-lacks-child-lacks
///   violates only for an invalid `(+)` flag (the `missing_enabled`
///   sub-branch — a flip off is already in effect); parent-lacks-
///   child-has always violates.
/// * `flag!=` (`OppositeParent`, `:1550-1567`): the mirror image —
///   parent-lacks-child-lacks violates for a valid or `(-)` flag;
///   parent-has-child-lacks violates only for an invalid `(+)` flag;
///   parent-has-child-has always violates.
/// * `!flag?` (`IfParentDisabled`, `:1568-1576`): parent-has is always
///   satisfied; otherwise child-has always violates, and
///   parent-lacks-child-lacks violates only for an invalid `(+)` flag.
///
/// For valid flags these reduce to the uniform `P`/`C` math (`P && !C`,
/// `P != C`, `P == C`, `!P && C`).
///
/// Empty means "no parent flip helps this candidate" — either no
/// conditional is parent-active, or the concrete gate fails (real's
/// `not (violated_atom.use.enabled or violated_atom.use.disabled)`;
/// here the unevaluated atom's own unconditional `flag`/`-flag` deps
/// must already hold for this candidate -- note the gate is on the
/// *unevaluated* form: a parent-lacks-child-has `flag=` (or either
/// `flag!=` match direction) lands in real's `conditional` partition,
/// not in `enabled`/`disabled`, so it never fails the gate even though
/// the *evaluated* form shows a concrete mismatch for it).
///
/// Documented narrowings vs real: a conditional token whose flag is
/// neither valid child IUSE nor `(+)/(-)`-defaulted poisons the whole
/// candidate. End-to-end this matches real exactly: such a flag sits in
/// the unevaluated atom's `required` set, so real's `missing_iuse`
/// branch (`:6721-6727`) skips the parent arm before
/// `violated_conditionals` is ever called — no flip either way. (In
/// isolation real's function would report the token in `conditional`;
/// the `tests_195d` oracle table's `arm` column shows the composed
/// outcome.) The unconditional-`required` subset must sit in child IUSE
/// for the same reason (real's `missing_iuse` branch — `:6725-6727`).
pub(crate) fn violated_parent_flags(
    unevaluated: &portage_dep::Atom,
    parent_use: &HashSet<String>,
    child_use: &HashSet<String>,
    child_valid: &HashSet<String>,
) -> Vec<String> {
    use portage_dep::{UseDepDefault, UseDepOp};
    let is_conditional = |op: &UseDepOp| {
        matches!(
            op,
            UseDepOp::IfParentEnabled
                | UseDepOp::IfParentDisabled
                | UseDepOp::EqualParent
                | UseDepOp::OppositeParent
        )
    };
    let use_deps = unevaluated.use_deps.as_deref().unwrap_or_default();
    let conds: Vec<&portage_dep::UseDep> = use_deps
        .iter()
        .filter(|ud| is_conditional(&ud.op))
        .collect();
    if conds.is_empty() {
        return Vec::new();
    }
    // Real `validate_flag`: valid child IUSE, or a `(+)/(-)` default
    // standing in for it. Anything else is a concrete violation there;
    // here it poisons the candidate (same arm outcome -- see the doc
    // comment's `missing_iuse` note).
    if conds
        .iter()
        .any(|ud| !child_valid.contains(&ud.flag) && ud.default.is_none())
    {
        return Vec::new();
    }
    // Real `required` (`no_default`): unconditional `flag`/`-flag`
    // forms without a `(+)`/`(-)` marker. Outside child IUSE they take
    // the `missing_iuse` branch, which never reaches the parent arm.
    let required_missing = use_deps.iter().any(|ud| {
        ud.default.is_none()
            && matches!(ud.op, UseDepOp::Enabled | UseDepOp::Disabled)
            && !child_valid.contains(&ud.flag)
    });
    if required_missing {
        return Vec::new();
    }
    // The concrete gate, on the unevaluated unconditional deps: real's
    // violated atom carries no `enabled`/`disabled` here (a
    // parent-lacks-child-has `flag=`, or either `flag!=` match
    // direction, lands in `conditional`, never in the concrete sets).
    // Real `:1511-1524`: `flag` violates iff the child lacks it and the
    // flag is valid or `(-)`-assumed; `-flag` violates iff the child
    // has it, or (lacking it) the flag is invalid-`(+)`-assumed. The
    // defaultless-invalid shapes never reach this gate: `required`
    // poisons above, as real's `missing_iuse` branch does.
    let concrete_violated = use_deps.iter().any(|ud| match ud.op {
        UseDepOp::Enabled => {
            !child_use.contains(&ud.flag)
                && (child_valid.contains(&ud.flag) || ud.default == Some(UseDepDefault::Disabled))
        }
        UseDepOp::Disabled => {
            child_use.contains(&ud.flag)
                || (!child_valid.contains(&ud.flag) && ud.default == Some(UseDepDefault::Enabled))
        }
        _ => false,
    });
    if concrete_violated {
        return Vec::new();
    }
    let mut involved: Vec<String> = conds
        .iter()
        .filter(|ud| {
            let p = parent_use.contains(&ud.flag);
            let c = child_use.contains(&ud.flag);
            let valid = child_valid.contains(&ud.flag);
            let missing_en = ud.default == Some(UseDepDefault::Enabled);
            let missing_dis = ud.default == Some(UseDepDefault::Disabled);
            match ud.op {
                // Real `:1525-1531`.
                UseDepOp::IfParentEnabled => p && !c && (valid || missing_dis),
                // Real `:1532-1549`.
                UseDepOp::EqualParent => {
                    if p && !c {
                        valid || missing_dis
                    } else if !p && !c {
                        !valid && missing_en
                    } else {
                        !p && c
                    }
                }
                // Real `:1550-1567`.
                UseDepOp::OppositeParent => {
                    if !p && !c {
                        valid || missing_dis
                    } else if p && !c {
                        !valid && missing_en
                    } else {
                        p && c
                    }
                }
                // Real `:1568-1576`.
                UseDepOp::IfParentDisabled => {
                    if !p && !c {
                        !valid && missing_en
                    } else {
                        !p && c
                    }
                }
                _ => false,
            }
        })
        .map(|ud| ud.flag.clone())
        .collect();
    involved.sort();
    involved.dedup();
    involved
}

/// Real `_show_unsatisfied_dep`'s per-`missing_use`-package loop
/// (`lib/_emerge/depgraph.py:6716-6858`), descending-cpv order: the
/// version/slot-matching *visible* ebuilds behind the unevaluated atom
/// (real `db.match(atom.without_use)` plus the `masked_pkg_instances`
/// skip — `:6775`), each paired with the parent-active violated flags
/// [`violated_parent_flags`] finds for it. A candidate is skipped when
/// a flag its own `use.mask`/`use.force` pins is needed to satisfy the
/// evaluated atom (real's untouchable-child `continue`, `:6732-6736`:
/// no child flip — and no parent flip either — can fix it) or when no
/// conditional is parent-active for it. The repair probe
/// ([`suggested_parent_use_candidate`]) and the display twin
/// ([`use_unsat_parent_row`]) both walk this list and take the first
/// viable candidate, mirroring real's return-True-on-first-collect.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn viable_parent_flip_targets(
    repos: &[RepoConfig],
    unevaluated: &portage_dep::Atom,
    evaluated: &portage_dep::Atom,
    unevaluated_atom: &str,
    parent_use: &HashSet<String>,
    config: &portage_profile::Config,
) -> Vec<(Candidate, Vec<String>)> {
    let category = unevaluated.category.clone();
    let package = unevaluated.package.clone();
    let Ok(candidates) = list_candidates(repos, &category, &package) else {
        return Vec::new();
    };
    let strs: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                category, package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    // Real `db.match(atom.without_use)`: version/slot filter, USE and
    // visibility ignored.
    let Some(matched) =
        portage_dep::match_from_list(portage_dep::without_use(unevaluated_atom), &refs)
    else {
        return Vec::new();
    };
    let matched: std::collections::HashSet<&str> = matched.into_iter().collect();
    let mut cands: Vec<&Candidate> = candidates
        .iter()
        .zip(strs.iter())
        .filter(|(_, s)| matched.contains(s.as_str()))
        .map(|(c, _)| c)
        .collect();
    // Real `cpv_list.reverse()` -> descending version.
    cands.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));

    let eval_deps = evaluated.use_deps.as_deref().unwrap_or_default();
    let mut out = Vec::new();
    for c in cands {
        // Real `masked_pkg_instances`: masked instances never reach the
        // parent arm (`:6775`).
        if !is_visible(c, &category, &package, config) {
            continue;
        }
        let Some((declared, use_flags)) = candidate_iuse_and_use(c, &category, &package, config)
        else {
            continue;
        };
        let valid = valid_iuse(&declared, config);
        // Real `pkg.use.enabled` (`Package.py:728-731` filters to valid
        // IUSE).
        let child_use: HashSet<String> = use_flags
            .iter()
            .filter(|f| valid.contains(*f))
            .cloned()
            .collect();
        // Real `need_enable`/`need_disable` (`:6729-6730`), for the
        // untouchable-child `continue` below.
        let need_enable: Vec<&String> = eval_deps
            .iter()
            .filter(|ud| {
                ud.op == portage_dep::UseDepOp::Enabled
                    && !child_use.contains(&ud.flag)
                    && valid.contains(&ud.flag)
            })
            .map(|ud| &ud.flag)
            .collect();
        let need_disable: Vec<&String> = eval_deps
            .iter()
            .filter(|ud| {
                ud.op == portage_dep::UseDepOp::Disabled
                    && child_use.contains(&ud.flag)
                    && valid.contains(&ud.flag)
            })
            .map(|ud| &ud.flag)
            .collect();
        // Real `:6732-6736`: a flag this candidate's own `use.mask`/
        // `use.force` pins is not adjustable — no parent flip either.
        let candidate_str = format!(
            "{}/{}-{}:{}/{}::{}",
            category, package, c.version, c.slot, c.sub_slot, c.repo_name
        );
        let stable = is_stable(
            &c.keywords,
            &candidate_str,
            &category,
            &package,
            &config.accept_keywords,
            &config.package_accept_keywords,
        );
        let mut untouchable = resolved_use_mask_or_force(
            MaskOrForce::Mask,
            config,
            &candidate_str,
            &category,
            &package,
            stable,
        )
        .as_ref()
        .clone();
        untouchable.extend(
            resolved_use_mask_or_force(
                MaskOrForce::Force,
                config,
                &candidate_str,
                &category,
                &package,
                stable,
            )
            .iter()
            .cloned(),
        );
        if need_enable
            .iter()
            .chain(need_disable.iter())
            .any(|f| untouchable.contains(*f))
        {
            continue;
        }
        let involved = violated_parent_flags(unevaluated, parent_use, &child_use, &valid);
        if involved.is_empty() {
            continue;
        }
        out.push((c.clone(), involved));
    }
    out
}

/// Real `--autounmask-use`'s own second, architecturally distinct
/// mechanism (real `_show_unsatisfied_dep`, `lib/_emerge/depgraph.py:
/// 6756-6846): unlike `suggested_use_candidate` (which flips the
/// *candidate's* own flag), this one flips the *requesting parent's* own
/// flag, for the case where a dependency atom's use-dep was originally
/// conditional (`opt?`/`!opt?`/`opt=`/`!opt=`) on the parent's own USE
/// state -- `enqueue_flat_deps` already evaluated it away into a
/// concrete form (or dropped it) before this atom was ever queued, using
/// the parent's own *current* USE; this asks "if the parent's own
/// involved flag(s) were toggled together instead, would the
/// re-evaluated atom now actually resolve?"
///
/// The involved flags are real `Atom.violated_conditionals`' parent-side
/// partition ([`violated_parent_flags`]): only conditionals the parent's
/// current USE actually activates (e.g. `foo(-)?` with parent `foo` on;
/// a `bar(-)?` the parent already has off is satisfied, not violated),
/// never every conditional flag the atom references. And each child
/// candidate is subject to real's untouchable-child `continue`
/// (`:6732-6736`): when the flag the child would need is `use.mask`'d
/// or `use.force`'d on the child, no parent flip is attempted for it
/// (a normal miss is displayed instead).
///
/// Gated on: every involved flag must be real, valid IUSE on the parent
/// (`implicit_iuse_set`); none may be `package.use.mask`/`.force`'d on
/// the parent (`flag_is_settable`, reused as-is -- its own logic doesn't
/// assume anything child-specific); the re-evaluated atom must actually
/// become satisfiable (`atom_currently_satisfiable`) against the
/// hypothetical flip; and the flip must not newly violate the parent's
/// own `REQUIRED_USE` (mirrors real `_show_unsatisfied_dep`'s own
/// `collect_use_changes and not required_use_warning` gate -- a flip
/// that already-violated `REQUIRED_USE` before the change is not
/// disqualified by it, only one that goes from satisfied to violated
/// is). Candidates are tried in descending version order and the first
/// viable one wins (real returns True on the first successful collect).
/// Returns `(parent_category, parent_package, parent_version,
/// [(flag, desired_state)])`, attached to the *dependency's* own
/// `GraphEntry` (`parent_use_suggestion`) rather than the parent's own
/// entry, unlike real portage's own `missing_use_reasons.append
/// ((myparent, ...))` -- a pragmatic simplification: portuale's
/// `GraphEntry` model has no per-parent "reasons" list to attach it to
/// instead, and the dependency's own entry is where the "no visible
/// ebuild for dependency" note already lives.
/// `(parent_category, parent_package, parent_version, [(flag, desired_state)])`.
pub type ParentUseSuggestion = (String, String, String, Vec<(String, bool)>);

pub(crate) fn suggested_parent_use_candidate(
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    unevaluated_atom: &str,
    owner: &(String, String),
    config: &portage_profile::Config,
) -> Option<ParentUseSuggestion> {
    let unevaluated = portage_dep::parse_atom(unevaluated_atom)?;
    let (parent_candidate, parent_iuse, parent_use, parent_required_use) =
        parent_use_state(repos, entries, owner, config)?;
    let evaluated_atom = portage_dep::evaluate_atom_conditionals(unevaluated_atom, &parent_use)?;
    let evaluated = portage_dep::parse_atom(&evaluated_atom)?;
    for (_child, involved) in viable_parent_flip_targets(
        repos,
        &unevaluated,
        &evaluated,
        unevaluated_atom,
        &parent_use,
        config,
    ) {
        if involved.iter().any(|f| !parent_iuse.contains(f)) {
            continue;
        }

        let target_use: Vec<(String, bool)> = involved
            .iter()
            .map(|f| (f.clone(), !parent_use.contains(f)))
            .collect();
        if target_use.iter().any(|(flag, desired)| {
            !flag_is_settable(
                &parent_candidate,
                &owner.0,
                &owner.1,
                flag,
                *desired,
                config,
            )
        }) {
            continue;
        }

        let mut hypothetical_use = parent_use.as_ref().clone();
        for (flag, desired) in &target_use {
            if *desired {
                hypothetical_use.insert(flag.clone());
            } else {
                hypothetical_use.remove(flag);
            }
        }

        let re_evaluated =
            portage_dep::evaluate_atom_conditionals(unevaluated_atom, &hypothetical_use)?;
        if !atom_currently_satisfiable(repos, &re_evaluated, config, &[]) {
            continue;
        }

        if let Some(required_use) = &parent_required_use
            && !required_use.trim().is_empty()
        {
            let old_sat =
                portage_required_use::check_required_use(required_use, &parent_use, &parent_iuse)
                    .unwrap_or(false);
            let new_sat = portage_required_use::check_required_use(
                required_use,
                &hypothetical_use,
                &parent_iuse,
            )
            .unwrap_or(false);
            if old_sat && !new_sat {
                continue;
            }
        }

        return Some((
            owner.0.clone(),
            owner.1.clone(),
            parent_candidate.version.clone(),
            target_use,
        ));
    }
    None
}
