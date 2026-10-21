//! Small helpers of the resolver pass.

use crate::*;

/// The effective value of one autounmask flag for the current pass: this pass's
/// overlay first, then the cross-pass accumulator. The walk's two flip sites must
/// see both (a flip folded on an *earlier* pass lives in `committed`; one folded
/// *earlier this pass* lives in `overlay`).
pub(crate) fn overlay_use_want(
    overlay: &HashMap<(String, String), HashMap<String, bool>>,
    committed: &HashMap<(String, String), HashMap<String, bool>>,
    key: &(String, String),
    flag: &str,
) -> Option<bool> {
    overlay
        .get(key)
        .and_then(|b| b.get(flag))
        .copied()
        .or_else(|| committed.get(key).and_then(|b| b.get(flag)).copied())
}

/// Arguments of [`onlydeps_walk_dep_key`].
pub(crate) struct OnlydepsWalkDepKeyArgs<'a> {
    pub(crate) key: &'a str,
    pub(crate) onlydeps_blanks_runtime: bool,
    pub(crate) onlydeps_blanks_idepend: bool,
}

/// Whether one dep key is walked for this package. `IDEPEND` reads
/// presence-based: Portage gates it only at generation time (`bin/ebuild.sh`
/// unsets it below EAPI 8, so a consistent cache never carries a stale one)
/// and walks whatever string the cache holds at resolve time without an EAPI
/// check (`edepend[k] = metadata[k]`). An `--onlydeps` root's `RDEPEND`/
/// `PDEPEND` (plus `IDEPEND` unless kept) blank the same way. Extracted so the
/// adversarial-cache path stays pinned.
// Portage: _emerge/depgraph.py, _add_pkg_deps
pub(crate) fn onlydeps_walk_dep_key(args: OnlydepsWalkDepKeyArgs<'_>) -> bool {
    let OnlydepsWalkDepKeyArgs {
        key,
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
    } = args;
    if key == "IDEPEND" && onlydeps_blanks_idepend {
        return false;
    }
    if onlydeps_blanks_runtime && (key == "RDEPEND" || key == "PDEPEND") {
        return false;
    }
    true
}

/// Arguments of [`onlydeps_runtime_keys`].
pub(crate) struct OnlydepsRuntimeKeysArgs {
    pub(crate) onlydeps_blanks_runtime: bool,
    pub(crate) onlydeps_blanks_idepend: bool,
}

/// Which run-time keys classify atoms as runtime edges for this package (an atom
/// in a build-time key but no walked run-time key is an unbreakable merge-order
/// edge). Mirrors `onlydeps_walk_dep_key` above.
// Portage: DepPriority buildtime_hard
pub(crate) fn onlydeps_runtime_keys(args: OnlydepsRuntimeKeysArgs) -> &'static [&'static str] {
    let OnlydepsRuntimeKeysArgs {
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
    } = args;
    if onlydeps_blanks_runtime {
        if onlydeps_blanks_idepend {
            &[]
        } else {
            &["IDEPEND"]
        }
    } else if onlydeps_blanks_idepend {
        &["RDEPEND", "PDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    }
}

/// Portage's `_minimize_children` over one parent's atoms on one cp, each already
/// paired with the outcome it would resolve to on its own. Returns every atom with
/// the outcome it keeps: when two or more distinct packages were selected, the
/// elimination runs (installed first, then the rest, each ascending by version) and
/// each atom binds to the highest surviving package it matches.
///
/// A package is `(version, installed)`: the installed instance and a same-version
/// ebuild are distinct nodes. Matching is `findAtomForPackage` on the package's
/// `cat/pkg-ver:slot/sub::repo` string; use-deps check against the installed vdb
/// `USE`/`IUSE` for a built package and against the ebuild candidate's effective
/// USE for a not-yet-built one. Atoms without a package (`NoVisibleCandidate`, a
/// removal) pass through untouched.
// Portage: _emerge/depgraph.py, _minimize_children; _pkg_use_enabled
pub(crate) fn minimize_children(
    root: &Path,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    key: &(String, String),
    picks: Vec<(String, PretendOutcome)>,
) -> Vec<(String, PretendOutcome)> {
    fn pkg_of(o: &PretendOutcome) -> Option<(String, bool)> {
        match o {
            PretendOutcome::AlreadyInstalled { version } => Some((version.clone(), true)),
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some((version.clone(), false))
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some((to.clone(), false))
            }
            PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
        }
    }
    let mut pkgs: Vec<(String, bool)> = Vec::new();
    let mut pkg_outcome: HashMap<(String, bool), PretendOutcome> = HashMap::new();
    for (_, o) in &picks {
        if let Some(p) = pkg_of(o)
            && !pkgs.contains(&p)
        {
            pkg_outcome.insert(p.clone(), o.clone());
            pkgs.push(p);
        }
    }
    if pkgs.len() < 2 {
        return picks;
    }
    let pkg_str = |(version, installed): &(String, bool)| -> String {
        let (slot, sub, repo) = if *installed {
            let (slot, sub) = read_vdb_slot(root, &key.0, &key.1, version);
            (slot, sub, installed_pkg_repo(root, &key.0, &key.1, version))
        } else {
            let (sub, repo, slot) = slot_conflict_meta(repos, &key.0, &key.1, version);
            (slot, sub, repo)
        };
        let sub = if sub.is_empty() { slot.clone() } else { sub };
        if repo.is_empty() {
            format!("{}/{}-{version}:{slot}/{sub}", key.0, key.1)
        } else {
            format!("{}/{}-{version}:{slot}/{sub}::{repo}", key.0, key.1)
        }
    };
    let strs: HashMap<(String, bool), String> =
        pkgs.iter().map(|p| (p.clone(), pkg_str(p))).collect();
    // The ebuild candidate behind each not-yet-built version, for the use-dep
    // check below. A same-version tie goes to the highest `repo_priority`; versions
    // with no ebuild candidate stay absent (binary-only picks keep the old
    // own-selection-only match).
    let ebuilds = list_candidates(repos, &key.0, &key.1).unwrap_or_default();
    let mut ebuild_by_version: HashMap<&str, &Candidate> = HashMap::new();
    for c in ebuilds.iter() {
        match ebuild_by_version.get(c.version.as_str()) {
            Some(prev) if prev.repo_priority >= c.repo_priority => {}
            _ => {
                ebuild_by_version.insert(c.version.as_str(), c);
            }
        }
    }
    // atom -> packages it matches.
    let mut matches: Vec<(String, Vec<(String, bool)>)> = Vec::new();
    for (a, o) in &picks {
        let Some(own) = pkg_of(o) else { continue };
        let parsed = portage_dep::parse_atom(a);
        let use_deps = parsed
            .as_ref()
            .and_then(|x| x.use_deps.as_ref())
            .filter(|d| !d.is_empty());
        let mut ms: Vec<(String, bool)> = Vec::new();
        for p in &pkgs {
            let matches_str = || {
                portage_dep::match_from_list(portage_dep::without_use(a), &[strs[p].as_str()])
                    .is_some_and(|m| !m.is_empty())
            };
            let use_ok = || match use_deps {
                None => true,
                // A built package: its vdb record.
                Some(deps) if p.1 => {
                    let vdb_iuse = read_vdb_flag_set(root, &key.0, &key.1, &p.0, "IUSE");
                    let vdb_use = read_vdb_flag_set(root, &key.0, &key.1, &p.0, "USE");
                    portage_dep::use_deps_satisfied_union(
                        deps,
                        &vdb_iuse,
                        &[&config.iuse_effective, &vdb_use],
                        &vdb_use,
                    )
                }
                // A not-yet-built package: the ebuild candidate's own effective USE
                // (a version with no ebuild candidate keeps the own-selection-only match).
                Some(deps) => ebuild_by_version
                    .get(p.0.as_str())
                    .and_then(|c| candidate_iuse_and_use(c, &key.0, &key.1, config))
                    .is_some_and(|(iuse, use_flags)| {
                        portage_dep::use_deps_satisfied_union(
                            deps,
                            &iuse,
                            &[&config.iuse_effective],
                            &use_flags,
                        )
                    }),
            };
            let hit = *p == own || (matches_str() && use_ok());
            if hit {
                ms.push(p.clone());
            }
        }
        matches.push((a.clone(), ms));
    }
    let mut order: Vec<(String, bool)> = pkgs.clone();
    order.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| vercmp_ordering(&x.0, &y.0)));
    let mut alive: HashSet<(String, bool)> = pkgs.iter().cloned().collect();
    for p in &order {
        let eliminate = matches.iter().all(|(_, ms)| {
            !ms.contains(p) || ms.iter().filter(|q| alive.contains(*q)).count() >= 2
        });
        if eliminate {
            alive.remove(p);
        }
    }
    picks
        .into_iter()
        .map(|(a, o)| {
            let Some((_, ms)) = matches.iter().find(|(x, _)| *x == a) else {
                return (a, o);
            };
            let best = ms
                .iter()
                .filter(|q| alive.contains(*q))
                .max_by(|x, y| vercmp_ordering(&x.0, &y.0).then_with(|| x.1.cmp(&y.1).reverse()));
            match best {
                Some(b) => {
                    let bo = pkg_outcome[b].clone();
                    (a, bo)
                }
                None => (a, o),
            }
        })
        .collect()
}

/// Arguments of [`arg_pinned_installed_reuse`].
pub(crate) struct ArgPinnedInstalledReuseArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) cache: &'a mut HashMap<(String, String), Vec<(String, String)>>,
    pub(crate) atom_root: &'a Path,
    pub(crate) key: &'a (String, String),
    pub(crate) current_atom: &'a str,
    pub(crate) atom: &'a portage_dep::Atom,
    pub(crate) fresh_version: &'a str,
}

/// Portage's `find_existing_node` pass as a pop-site reuse check: Portage consults
/// the packages already in the graph before picking the highest available one and
/// reuses the node when the dep atom matches it, unless a higher version is
/// available in a *different* slot. The winner returns directly.
///
/// Portuale resolves the consumer's dep *before* the pinning argument, so no
/// pass-state node exists yet at selection time: the pinned set is re-derived from
/// the top-level atoms instead (same-cp atoms, blockers skipped, speculatively
/// re-resolved as the walk would; memoized per pass in
/// [`PassState::top_pinned_installed`], which never outlives one pass).
///
/// The caller applies the reuse only for a fresh new merge (`New`/`Upgrade`/
/// `Downgrade`) in main slot `S`, additionally requiring a pinned instance in that
/// same slot, the dep atom matching the pinned `cat/pkg-ver:slot/sub` string, and
/// (for `[use]` deps) the pinned instance's recorded vdb `USE` satisfying them.
/// Highest matching pin wins; a `Reinstall` verdict is never overridden.
/// `||`-chosen atoms keep their own preference; `@world`/`@selected` members pin
/// nothing here; cross-root atoms resolve against the dep's own root.
// Portage: lib/_emerge/depgraph.py, _wrapped_select_pkg_highest_available_imp;
// _package_tracker.match; atom.match; _check_slot_conflict
pub(crate) fn arg_pinned_installed_reuse(args: ArgPinnedInstalledReuseArgs<'_>) -> Option<String> {
    let ArgPinnedInstalledReuseArgs {
        ctx,
        config,
        bp,
        union_constraints,
        cache,
        atom_root,
        key,
        current_atom,
        atom,
        fresh_version,
    } = args;
    // The fresh pick's own main slot: the reuse target must sit in it. Same winner
    // rule as the pop site's own `resolved` pick.
    let fresh_slot = list_candidates(&ctx.repos, &key.0, &key.1)
        .ok()?
        .iter()
        .filter(|c| c.version == fresh_version)
        .max_by(|a, b| {
            let src_rank = |c: &Candidate| match c.source {
                CandidateSource::Binary => 1u8,
                CandidateSource::Ebuild => 0,
            };
            src_rank(a)
                .cmp(&src_rank(b))
                .then(a.repo_priority.cmp(&b.repo_priority))
        })?
        .slot
        .clone();
    if !cache.contains_key(key) {
        let empty_constraints: Vec<String> = Vec::new();
        let extra_constraints = union_constraints.get(key).unwrap_or(&empty_constraints);
        let mut kept: Vec<(String, String)> = Vec::new();
        for t in ctx.atoms {
            let Some(arg) = portage_dep::parse_atom(t) else {
                continue;
            };
            if arg.blocker != portage_dep::Blocker::None {
                continue;
            }
            if arg.category != key.0 || arg.package != key.1 {
                continue;
            }
            let Ok(outcome) = resolve_pretend(
                &SelectSource {
                    repos: &ctx.repos,
                    root: atom_root,
                    config,
                    local_binpkg: &ctx.local_binpkg,
                },
                t,
                &SelectOptions {
                    is_top_level: true,
                    extra_constraints,
                    ..ctx.select_options(bp)
                },
            ) else {
                continue;
            };
            if let PretendOutcome::AlreadyInstalled { version } = outcome
                && let Some((_, slot, _)) = installed_candidates(atom_root, &key.0, &key.1)
                    .into_iter()
                    .find(|(v, _, _)| v == &version)
            {
                kept.push((version, slot));
            }
        }
        kept.sort();
        kept.dedup();
        cache.insert(key.clone(), kept);
    }
    let kept = cache.get(key)?;
    let installed = installed_candidates(atom_root, &key.0, &key.1);
    let mut best: Option<(String, String, String)> = None;
    for (version, slot) in kept {
        if *slot != fresh_slot {
            continue;
        }
        let sub = installed
            .iter()
            .find(|(v, _, _)| v == version)
            .map(|(_, _, sub)| sub.clone())
            .unwrap_or_default();
        let pinned_str = if sub.is_empty() {
            format!("{}/{}-{version}:{slot}", key.0, key.1)
        } else {
            format!("{}/{}-{version}:{slot}/{sub}", key.0, key.1)
        };
        if portage_dep::match_from_list(current_atom, &[pinned_str.as_str()])
            .is_none_or(|m| m.is_empty())
        {
            continue;
        }
        if let Some(use_deps) = atom.use_deps.as_deref()
            && !use_deps.is_empty()
        {
            let vdb_iuse = read_vdb_flag_set(atom_root, &key.0, &key.1, version, "IUSE");
            let vdb_use = read_vdb_flag_set(atom_root, &key.0, &key.1, version, "USE");
            if !portage_dep::use_deps_satisfied_union(
                use_deps,
                &vdb_iuse,
                &[&config.iuse_effective, &vdb_use],
                &vdb_use,
            ) {
                continue;
            }
        }
        let take = match &best {
            None => true,
            Some((bv, _, _)) => vercmp_ordering(version, bv) == Ordering::Greater,
        };
        if take {
            best = Some((version.clone(), slot.clone(), sub));
        }
    }
    best.map(|(version, _, _)| version)
}
