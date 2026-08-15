//! Whether an atom is installed or already in the graph.

use crate::*;

/// One `||` alternative atom matched at cp level (version, slot and use-deps all
/// stripped), or a `virtual/`, or a blocker (vacuously satisfied). Used to bump a
/// `||` alternative into the installed rank so an already-installed branch wins over
/// a first-listed one that would need a new merge.
/// Portage: dep_check.py, dep_zapdeps (all_installed).
///
/// Deliberately does NOT re-check the atom's own use-deps against the installed
/// package's recorded `USE`: the cp-level predicate is blind to `[use]` by
/// construction. The USE side is a separate fact (`all_use_satisfied`) that
/// `disjunction_preference` computes and combines with this one itself.
pub(crate) fn atom_cp_installed(root: &Path, atom_str: &str) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None || atom.category == "virtual" {
        return true;
    }
    !installed_candidates(root, &atom.category, &atom.package).is_empty()
}

/// One `||` alternative atom matched fully -- version/slot AND its `[use]` deps
/// against the matching installed version's own recorded vdb `USE` (a flag the ebuild
/// has since dropped from `IUSE` but the installed copy was built with still counts).
/// No blocker/virtual exemption here, unlike `atom_cp_installed`.
/// Portage: dep_check.py, dep_zapdeps (the `not all_available` classification).
pub(crate) fn atom_matches_installed(
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
) -> bool {
    best_installed_matching(root, atom_str, config).is_some()
}

/// The highest installed version matching `atom_str` whose recorded vdb `USE`
/// satisfies its `[use]` deps -- the version `atom_matches_installed` reports. Split
/// out so `resolve_pretend` can return that exact version, not merely existence.
pub(crate) fn best_installed_matching(
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
) -> Option<String> {
    best_installed_match(root, atom_str, None, &|| config.iuse_effective.clone())
}

/// `best_installed_matching` with the two inputs a caller outside the resolver
/// supplies itself (the native `portageq` `has_version` / `best_version`):
///
/// - `unevaluated`: when `atom_str` is a USE-conditional-evaluated copy of an atom,
///   the original text. The IUSE membership check reads the *unevaluated* atom's
///   required flags, so a flag a dropped `[flag?]` names must still be installed
///   `IUSE`; the dropped conditionals are fed as constraint-free entries for that.
/// - `implicit_iuse`: the profile's `IUSE_EFFECTIVE`. Called at most once and only
///   when the atom has `[use]` deps, so a caller can load the config lazily.
///
/// Returns the highest matching installed version (the `PF` version part).
/// Portage: vartree.py, vardb.match().
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

/// Whether one atom is satisfied by a package already added to the graph this run
/// (merge-bound), with `[use]` deps checked against that entry's resolved USE.
/// Portage matches against full package objects -- slot included -- so the candidate
/// carries the entry's `:slot[/sub-slot]`; a slotless entry falls back to the bare
/// cpv (matching slotless used to over-match `:slot` atoms). Split out so the
/// circular-self-dep check can ask the same question for one atom.
/// Portage: dep_check.py, dep_zapdeps (all_in_graph).
///
/// Returns `false` for an empty or all-blocker alternative (nothing to be "in
/// graph"); the caller's own zero-atom / circular-self handling covers those first.
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

/// The visible tree candidates matching `atom_str` (USE deps ignored) after the
/// backtrack loop's `runtime_pkg_mask` constraints, as `(version, slot)` -- the listing
/// half of `atom_currently_satisfiable`, returned rather than collapsed to a bool so
/// the downgrade guards can compare versions.
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
            // A per-instance negative hides no tree candidate (see
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
