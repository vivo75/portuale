//! Whether an atom is installed or already in the graph.

use crate::*;

/// Real `dep_zapdeps`'s `all_installed` predicate for one `||` alternative
/// atom (`dep_check.py:603-607`): `Atom(atom.cp)` -- **cp level**, with
/// version, slot and use-deps all stripped -- is matched by the vdb, OR
/// the atom targets a `virtual/` (real's explicit "new-style virtuals
/// have zero cost to install" exemption). A blocker atom is vacuously
/// satisfied (it's a conflict to report, not a dependency to resolve).
/// Used to bump a `||` alternative into `AltPreference::Installed`
/// (real's `preferred_installed` choice bin) so an already-installed
/// branch wins over a first-listed one that would need a new merge.
///
/// Deliberately does NOT re-check the atom's own use-deps against the
/// installed package's recorded `USE`: real's `all_installed` is
/// `vardb.match(Atom(atom.cp))`, strictly cp-level, by construction blind
/// to `[use]`. The USE side is a wholly separate fact
/// (`all_use_satisfied`) that [`disjunction_preference`] computes and
/// combines with this one itself -- not something this predicate's
/// caller pre-filters away.
pub(crate) fn atom_cp_installed(root: &Path, atom_str: &str) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None || atom.category == "virtual" {
        return true;
    }
    !installed_candidates(root, &atom.category, &atom.package).is_empty()
}

/// Real `vardb.match(atom)` for one `||` alternative atom, the FULL
/// match (`dep_check.py` soft 715-722, the `not all_available` "other_*"
/// classification -- slice 4 of backlog #22): unlike `atom_cp_installed`
/// (cp-level, deliberately blind to `[use]`), this checks the atom's
/// version/slot AND its `[use]` deps against the matching installed
/// version's own recorded vdb `USE` -- real's `built`-package semantics
/// (`_iuse_implicit_built`, bug #640318: the `is_valid_flag` domain is
/// the vdb `IUSE` unioned with the package's own recorded `USE`, so a
/// flag the ebuild has since dropped from `IUSE` but the installed copy
/// was built with still counts), same pattern as
/// `dependency_avoid_update_candidate`'s own USE-dep filter. No blocker/
/// virtual exemption here (unlike `atom_cp_installed`): real's own
/// `all_installed`/`some_installed` loop only ever calls this for
/// `if not atom.blocker`-filtered atoms (the caller's job), and this
/// exact code path has no virtual short-circuit in real either.
pub(crate) fn atom_matches_installed(
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
) -> bool {
    best_installed_matching(root, atom_str, config).is_some()
}

/// The highest installed version matching `atom_str` whose recorded vdb
/// `USE` satisfies its `[use]` deps -- the version [`atom_matches_installed`]
/// reports. Split out so `resolve_pretend` can return that exact version
/// (real's selection returns the installed `inst_pkg`, not merely "some
/// installed instance exists").
pub(crate) fn best_installed_matching(
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
) -> Option<String> {
    best_installed_match(root, atom_str, None, &|| config.iuse_effective.clone())
}

/// [`best_installed_matching`] with the two inputs a caller outside the
/// resolver supplies itself (feat#157 S6.2, the native `portageq`
/// `has_version` / `best_version`, real `vardb.match()`):
///
/// - `unevaluated`: when `atom_str` is a USE-conditional-evaluated copy of
///   an atom (real `Atom.evaluate_conditionals`), the original text.
///   Real `_match_use` checks the IUSE membership of the *unevaluated*
///   atom's `.required` flags, so a flag a dropped `[flag?]` names must
///   still be in the installed `IUSE`; the dropped conditionals are fed to
///   `use_deps_satisfied` as constraint-free entries for that check.
/// - `implicit_iuse`: the profile's `IUSE_EFFECTIVE` (what
///   `config.iuse_effective` is for the resolver). Called at most once and
///   only when the atom has `[use]` deps, so a caller can load the config
///   lazily.
///
/// Returns the highest matching installed version (the `PF` version part).
pub fn best_installed_match(
    root: &Path,
    atom_str: &str,
    unevaluated: Option<&str>,
    implicit_iuse: &dyn Fn() -> HashSet<String>,
) -> Option<String> {
    let atom = portage_dep::parse_atom(atom_str)?;
    let installed = installed_candidates(root, &atom.category, &atom.package);
    if installed.is_empty() {
        return None;
    }
    let cpv_strs: Vec<String> = installed
        .iter()
        .map(|(v, s, ss)| {
            format!(
                "{}/{}-{v}:{s}/{ss}::{}",
                atom.category,
                atom.package,
                installed_pkg_repo(root, &atom.category, &atom.package, v)
            )
        })
        .collect();
    let refs: Vec<&str> = cpv_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom_str, &refs)?;
    let mut all_deps: Vec<portage_dep::UseDep> = atom.use_deps.clone().unwrap_or_default();
    if let Some(orig) = unevaluated.and_then(portage_dep::parse_atom)
        && let Some(orig_deps) = orig.use_deps
    {
        all_deps.extend(orig_deps);
    }
    let use_deps = Some(all_deps).filter(|d| !d.is_empty());
    let by_str: HashMap<&str, &(String, String, String)> =
        refs.iter().copied().zip(installed.iter()).collect();
    let implicit: std::cell::OnceCell<HashSet<String>> = std::cell::OnceCell::new();
    matched
        .into_iter()
        .filter_map(|m| by_str.get(m).map(|(version, _slot, _sub)| version.clone()))
        .filter(|version| {
            let Some(use_deps) = &use_deps else {
                return true;
            };
            let vdb_iuse = read_vdb_flag_set(root, &atom.category, &atom.package, version, "IUSE");
            let vdb_use = read_vdb_flag_set(root, &atom.category, &atom.package, version, "USE");
            let mut valid = vdb_iuse;
            valid.extend(implicit.get_or_init(implicit_iuse).iter().cloned());
            valid.extend(vdb_use.iter().cloned());
            portage_dep::use_deps_satisfied(use_deps, &valid, &vdb_use)
        })
        .max_by(|a, b| vercmp_ordering(a, b))
}

/// Real `dep_zapdeps`'s `all_installed_slots` predicate (`dep_check.py`
/// soft 599-607, `slot_map`/`avail_slot`): whether the *slot* of the
/// best USE-ignoring candidate for `atom_str`
/// (`highest_available_candidate_ignoring_use`, real's `avail_pkg`) is
/// itself installed -- `slot_map[Atom(f"{atom.cp}:{avail_pkg.slot}")]`
/// matched against the vdb, or `virtual/` (zero-cost exemption, same as
/// [`atom_cp_installed`]). Distinct from `atom_cp_installed`: a package
/// can be installed in some *other* slot while the specific slot this
/// `||` alternative would need is not, which real still counts as
/// `unsat_use_non_installed`, not `unsat_use_installed`. A blocker is
/// vacuously satisfied. No candidate at all (`all_available` already
/// rejected the whole choice) counts as not-installed-in-slot.
pub(crate) fn atom_installed_in_slot_of(
    root: &Path,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    atom_str: &str,
    extra_constraints: &[String],
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None || atom.category == "virtual" {
        return true;
    }
    let Some((.., slot)) = highest_available_candidate_ignoring_use(
        repos,
        atom_str,
        &atom.category,
        &atom.package,
        config,
        extra_constraints,
    ) else {
        return false;
    };
    installed_candidates(root, &atom.category, &atom.package)
        .iter()
        .any(|(_, installed_slot, _)| *installed_slot == slot)
}

/// Real `dep_zapdeps`'s `all_in_graph` predicate for a `||` alternative
/// (`dep_check.py:636-649`): every non-blocker atom of the alternative is
/// satisfied by a package *already added to the graph this run*
/// (merge-bound), with the atom's own `[use]` deps checked against that
/// entry's resolved USE (`use_flags_display`). Real files such an
/// alternative in choice bin 0 -- `preferred_in_graph`, the same list
/// object `preferred_installed`/`preferred_any_slot` collapse to -- so it
/// ranks exactly like an installed alternative. This is what makes
/// `virtual/secret-service`'s `|| ( gnome-base/gnome-keyring
/// >=kde-frameworks/kwallet-runtime-6.18.0[keyring] app-admin/keepassxc )`
/// pick `kwallet-runtime` (pulled in anyway by the KDE stack) instead of
/// merging the first-listed `gnome-keyring` and its whole GTK/gcr subtree.
///
/// Returns `false` for an empty or all-blocker alternative (nothing to be
/// "in graph"); the caller's own zero-atom / circular-self handling
/// covers those cases first.
/// Real `dep_zapdeps`'s per-atom `all_in_graph` test (`dep_check.py:636-649`;
/// see `atoms_all_in_graph` for the whole-alternative predicate): the atom
/// is satisfied by a package already added to the graph this run
/// (merge-bound), with `[use]` deps checked against that entry's resolved
/// USE. Real matches against full package objects -- slot included
/// (`graph_db.match_pkgs(atom)` over the digraph) -- so the candidate
/// carries the entry's `:slot[/sub-slot]` (the same assembled shape as
/// [`circular_child_candidate`]; a slotless entry falls back to the bare
/// cpv). Matching slotless used to over-match `:slot` atoms (`matches_slot`
/// is vacuous without a candidate slot): `dev-lang/rust:1.95.0`
/// "matched" graphed `rust-1.96.1`, ranked `Installed`, and beat
/// `rust-bin:1.96.1` on the backtrack retry -- the g221c L0 regression
/// (real's slot check rejects it, `rust-bin` wins). Split out of
/// `atoms_all_in_graph` so the circular-self-dep check
/// in `disjunction_preference` can ask the same question for one atom: a
/// self-naming atom that matches an in-graph node is real's pass-1
/// `preferred_in_graph` pick (backlog #216), not a circle to refuse.
/// (The gate itself asks the coarser cp-presence question --
/// [`atom_cp_present_in_graph`] -- since a cross-slot same-cp atom must
/// still proceed to the bins; only the choice bins use this
/// slot-precise rule.)
pub(crate) fn atom_matches_graph(
    atom_str: &str,
    entries: &[GraphEntry],
    config: &portage_profile::Config,
    queued_cps: &HashSet<(String, String)>,
) -> bool {
    let Some(parsed) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if parsed.blocker != portage_dep::Blocker::None {
        return false;
    }
    if queued_cps.contains(&(parsed.category.clone(), parsed.package.clone())) {
        return true;
    };
    let use_deps = parsed.use_deps.unwrap_or_default();
    entries.iter().any(|e| {
        let Some(cpv) = merge_bound_cpv(e) else {
            return false;
        };
        if e.category != parsed.category || e.package != parsed.package {
            return false;
        }
        let mut candidate = cpv;
        if let Some(slot) = &e.slot {
            candidate.push(':');
            candidate.push_str(slot);
            if let Some(sub_slot) = &e.sub_slot {
                candidate.push('/');
                candidate.push_str(sub_slot);
            }
        }
        if portage_dep::match_from_list(atom_str, &[candidate.as_str()])
            .is_none_or(|m| m.is_empty())
        {
            return false;
        }
        if use_deps.is_empty() {
            return true;
        }
        let enabled: HashSet<String> = e
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let iuse: HashSet<String> = e.use_flags_display.iter().map(|(f, _)| f.clone()).collect();
        portage_dep::use_deps_satisfied(&use_deps, &valid_iuse(&iuse, config), &enabled)
    })
}

/// The circular-self-dep gate's cp-level presence check: does a merge-bound
/// entry (or a queued-but-unresolved atom, backlog #90 (S1)'s drain-state
/// approximation) share the atom's `cat/pkg`? Deliberately slot-blind,
/// unlike [`atom_matches_graph`]: the gate (portuale-only, real has no
/// self-exclusion) asks "is our cp in play at all", not "does this atom
/// resolve to a graphed instance". A same-cp cross-slot atom with a
/// different-slot instance graphed (e.g. `dev-lang/rust:1.95.0` while
/// `rust-1.96.1` is merge-bound) must still proceed to the choice bins --
/// real merges the second instance as an ordinary dependency -- so slot
/// precision here would wrongly refuse it as circular. Body is the
/// pre-g221c `atom_matches_graph` verbatim; only the choice bins moved to
/// the slot-precise rule.
pub(crate) fn atom_cp_present_in_graph(
    atom_str: &str,
    entries: &[GraphEntry],
    config: &portage_profile::Config,
    queued_cps: &HashSet<(String, String)>,
) -> bool {
    let Some(parsed) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if parsed.blocker != portage_dep::Blocker::None {
        return false;
    }
    if queued_cps.contains(&(parsed.category.clone(), parsed.package.clone())) {
        return true;
    };
    let use_deps = parsed.use_deps.unwrap_or_default();
    entries.iter().any(|e| {
        let Some(cpv) = merge_bound_cpv(e) else {
            return false;
        };
        if e.category != parsed.category || e.package != parsed.package {
            return false;
        }
        if portage_dep::match_from_list(atom_str, &[cpv.as_str()]).is_none_or(|m| m.is_empty()) {
            return false;
        }
        let enabled: HashSet<String> = e
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let iuse: HashSet<String> = e.use_flags_display.iter().map(|(f, _)| f.clone()).collect();
        portage_dep::use_deps_satisfied(&use_deps, &valid_iuse(&iuse, config), &enabled)
    })
}

pub(crate) fn atoms_all_in_graph(
    atoms: &[String],
    entries: &[GraphEntry],
    config: &portage_profile::Config,
    queued: &[QueueItem],
) -> bool {
    let non_blocker: Vec<&String> = atoms
        .iter()
        .filter(|a| {
            portage_dep::parse_atom(a).is_some_and(|p| p.blocker == portage_dep::Blocker::None)
        })
        .collect();
    if non_blocker.is_empty() {
        return false;
    }
    // Backlog #90 (S1): real resolves `||` groups off
    // `_dep_disjunctive_stack`, i.e. only after the main `dep_stack`
    // has drained -- so by resolution time every plain dep this run
    // will ever graph is already graphed. Portuale resolves `||`
    // eagerly at pop time; a queued-but-unresolved atom approximates
    // that drain state (cp-level, mirroring `atom_cp_installed`'s
    // own cp-level style beside it -- version/USE precision would
    // need the not-yet-selected instance). Residual risk: a queued
    // dep that later proves unresolvable mispredicts the branch;
    // the backtrack loop bounds that the way it bounds every
    // speculative selection.
    let queued_cps: HashSet<(String, String)> = queued
        .iter()
        .filter_map(|q| {
            portage_dep::parse_atom(&q.atom).map(|a| (a.category.clone(), a.package.clone()))
        })
        .collect();
    non_blocker
        .iter()
        .all(|a| atom_matches_graph(a, entries, config, &queued_cps))
}

/// The visible tree candidates matching `atom_str` (USE deps ignored) after
/// the `'backtrack` loop's `runtime_pkg_mask` constraints, as
/// `(version, slot)` -- the listing half of `atom_currently_satisfiable`,
/// returned rather than collapsed to a bool so the bug-531656 downgrade
/// guards can compare versions. Same small-duplication precedent as that
/// function's own doc comment.
pub(crate) fn visible_tree_matches(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    extra_constraints: &[String],
) -> Vec<(String, String)> {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return Vec::new();
    };
    let Ok(candidates) = list_candidates(repos, &atom.category, &atom.package) else {
        return Vec::new();
    };
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    let strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    let stripped = portage_dep::without_use(atom_str);
    let Some(matched) = portage_dep::match_from_list(stripped, &refs) else {
        return Vec::new();
    };
    let passes_constraints = |m: &str| {
        extra_constraints.iter().all(|c| match c.strip_prefix('!') {
            // #212/#276: a per-instance negative (`[binary]`
            // `slot_operator_mask_built`, `[installed]`
            // missing-dependency) hides no tree candidate (see
            // [`is_instance_only_mask`]).
            Some(_) if is_instance_only_mask(c) => true,
            Some(neg) => !portage_dep::match_from_list(neg, &[m]).is_some_and(|r| !r.is_empty()),
            None => portage_dep::match_from_list(c, &[m]).is_some_and(|r| !r.is_empty()),
        })
    };
    visible
        .iter()
        .zip(strs.iter())
        .filter(|(_, s)| matched.contains(&s.as_str()) && passes_constraints(s))
        .map(|(c, _)| (c.version.clone(), c.slot.clone()))
        .collect()
}

/// Real `depgraph._downgrade_probe` (`depgraph.py:2946`): a downgrade to
/// `version` is *desirable* only when no visible, not-installed candidate in
/// its slot is at the same or a higher version (the current one is masked or
/// gone). `slot_atom` is `cat/pkg:slot`; `extra_constraints` is the
/// `runtime_pkg_mask` real's `_iter_similar_available` skips.
pub(crate) fn downgrade_probe(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    slot_atom: &str,
    version: &str,
    extra_constraints: &[String],
) -> bool {
    let similar = visible_tree_matches(repos, slot_atom, config, extra_constraints);
    !similar.is_empty()
        && !similar
            .iter()
            .any(|(v, _)| vercmp_ordering(v, version) != Ordering::Less)
}
