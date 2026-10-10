//! Small helpers of the resolver pass.

use crate::*;

/// Phase A3 (023): the effective value of one autounmask flag for the
/// current pass -- this pass's overlay first, then the cross-pass
/// accumulator. The walk's two flip sites must see both (a flip folded on
/// an *earlier* pass lives in `committed`; one folded *earlier this pass*
/// lives in `overlay`), exactly as the old direct-write-then-read did.
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
    pub(crate) eapi_has_idepend: bool,
    pub(crate) onlydeps_blanks_runtime: bool,
    pub(crate) onlydeps_blanks_idepend: bool,
}

/// Backlog #194: whether one dep key is walked for this package.
/// Real `bin/ebuild.sh`'s own `depend` phase unsets `IDEPEND` below
/// EAPI 8 (`portage/eapi.py:135`), so a stale or hand-written cache
/// entry can still *carry* `IDEPEND` under EAPI 7 (real `egencache`
/// never emits one) -- the read-time gate here drops it anyway. Real
/// `depgraph.py:4186-4193` additionally blanks an `--onlydeps` root's
/// `RDEPEND`/`PDEPEND` under `--onlydeps-with-rdeps=n`, and its
/// `IDEPEND` too unless `--onlydeps-with-ideps` is `y`/`True`.
/// Extracted (rather than left as `run_pass`'s inline closure) so the
/// adversarial-cache path stays pinned after the fixtures stopped
/// carrying one.
pub(crate) fn onlydeps_walk_dep_key(args: OnlydepsWalkDepKeyArgs<'_>) -> bool {
    let OnlydepsWalkDepKeyArgs {
        key,
        eapi_has_idepend,
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
    } = args;
    if key == "IDEPEND" && (!eapi_has_idepend || onlydeps_blanks_idepend) {
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
    pub(crate) eapi_has_idepend: bool,
}

/// Backlog #194: which run-time keys classify atoms as runtime edges
/// for this package (real `DepPriority`: an atom in a build-time key
/// but no walked run-time key is `buildtime_hard`, an unbreakable
/// merge-order edge). Mirrors `onlydeps_walk_dep_key` above: under
/// `--onlydeps --onlydeps-with-rdeps=n --onlydeps-with-ideps=y` on
/// EAPI 8, `IDEPEND` is still walked, so it still classifies.
pub(crate) fn onlydeps_runtime_keys(args: OnlydepsRuntimeKeysArgs) -> &'static [&'static str] {
    let OnlydepsRuntimeKeysArgs {
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    } = args;
    if onlydeps_blanks_runtime {
        if onlydeps_blanks_idepend || !eapi_has_idepend {
            &[]
        } else {
            &["IDEPEND"]
        }
    } else if onlydeps_blanks_idepend || !eapi_has_idepend {
        &["RDEPEND", "PDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    }
}

/// #236 B: real `_minimize_children` (`_emerge/depgraph.py:4751-4854`)
/// over one parent's atoms on one cp, each already paired with the
/// outcome it would resolve to on its own. Returns every atom with the
/// outcome it keeps: when two or more distinct packages were selected,
/// real's elimination runs -- installed packages first, then the rest,
/// each group ascending by version (bug 631894), and a package is dropped
/// when every atom that matches it also matches another surviving
/// package -- and each atom is then bound to the highest surviving
/// package it matches (`child_pkgs.sort(); child_pkgs[-1]`).
///
/// A package is `(version, installed)`: the installed instance and a
/// same-version ebuild are distinct nodes in real. Matching is real's
/// `findAtomForPackage(pkg, modified_use=self._pkg_use_enabled(pkg))` on
/// the package's `cat/pkg-ver:slot/sub::repo` string. For an installed
/// package the use-deps are checked against its recorded vdb `USE`/`IUSE`
/// (real's `_pkg_use_enabled` for a built package, the same check
/// [`best_installed_matching`] makes) -- #107: installed `mesa`'s live
/// `libdisplay-info:=[abi_x86_32(-),abi_x86_64(-)]` and built
/// `libdisplay-info:0/3=[...]` both bind the installed `0.3.0`. For a
/// not-yet-built package the use-deps are checked against that version's
/// ebuild candidate -- its md5-cache IUSE plus its profile-computed
/// effective USE (real's `_pkg_use_enabled` for an unbuilt package, the
/// same pair the walk's own selection applies) -- #266: `>=foo-1[bar]`
/// alongside `<foo-3` collapses onto the lower pick when that pick builds
/// `bar`, and stays split when it does not. A version with no ebuild
/// candidate (a binary-only pick) still only ever counts as matching its
/// own selection.
/// Atoms without a package (`NoVisibleCandidate`, a removal) are passed
/// through untouched, like real's `(atom, None)` yield.
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
    // #266: the ebuild candidate behind each not-yet-built version, for
    // the use-dep check below. A same-version tie goes to the highest
    // `repo_priority`, the walk's own selection tie-break; versions with
    // no ebuild candidate stay absent (binary-only picks keep the old
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
    // atom -> packages it matches (real's `atom_pkg_graph.child_nodes`).
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
                // Real `_pkg_use_enabled` of a built package: its vdb
                // record (see the doc comment).
                Some(deps) if p.1 => {
                    let vdb_iuse = read_vdb_flag_set(root, &key.0, &key.1, &p.0, "IUSE");
                    let vdb_use = read_vdb_flag_set(root, &key.0, &key.1, &p.0, "USE");
                    let mut valid = valid_iuse(&vdb_iuse, config);
                    valid.extend(vdb_use.iter().cloned());
                    portage_dep::use_deps_satisfied(deps, &valid, &vdb_use)
                }
                // #266: real `_pkg_use_enabled` of a not-yet-built
                // package -- the ebuild candidate's own effective USE,
                // the same pair the walk's own selection applies. A
                // version with no ebuild candidate (a binary-only pick)
                // keeps the old own-selection-only match.
                Some(deps) => ebuild_by_version
                    .get(p.0.as_str())
                    .and_then(|c| candidate_iuse_and_use(c, &key.0, &key.1, config))
                    .is_some_and(|(iuse, use_flags)| {
                        portage_dep::use_deps_satisfied(
                            deps,
                            &valid_iuse(&iuse, config),
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

/// Backlog #285: real `_wrapped_select_pkg_highest_available_imp`'s
/// `find_existing_node` pass (`lib/_emerge/depgraph.py:7854`, the `for
/// find_existing_node in True, False` loop) as a pop-site reuse check.
///
/// Real consults the packages already added to the graph before picking
/// the highest available one: for the current db's first match it looks
/// up `_package_tracker.match(root, pkg.slot_atom, installed=False)`
/// (`:8180-8215`) -- the in-graph nodes in that (main-)`slot_atom`
/// (`slot_atom` is `cp:slot`, `_emerge/Package.py:139`, so `1/1` and
/// `1/2` share it) -- and reuses the node when the dep atom matches it
/// (`atom.match(e_pkg)`), unless a higher version is available in a
/// *different* slot (then the node is "irrelevant" and the highest
/// wins). The winner returns directly (`:8370-8371`, mirrored at add
/// time by `_check_slot_conflict`'s reuse, `:3532-3547`, in `_add_pkg`,
/// `:3661-3670`).
///
/// The slice shape (`=app-misc/mmprov-2 app-misc/mmcons` under
/// `--update --deep`, real 3.0.82.2 probed 2026-10-02): the `=mmprov-2`
/// argument puts installed `mmprov-2:1/1` in the graph, and after the
/// missed-slot-abi backtrack (`_slot_operator_update_backtrack`,
/// `:2400-2452`) the rebuilt mmcons' dep `app-misc/mmprov:=` reuses
/// that node (soft/`runtime_slot_op` edge) instead of re-selecting the
/// highest slot-1 version (mmprov-3). Real only keeps it when it
/// satisfies the dep atom.
///
/// Portuale resolves the consumer's dep *before* the pinning argument
/// (reversed queue order, #90 S1), so no pass-state node exists yet at
/// selection time and `resolve_pretend` itself is stateless: the pinned
/// set is re-derived from the top-level atoms instead. For every
/// same-cp top-level atom (blockers skipped) the helper speculatively
/// re-resolves it exactly as the walk would (`is_top_level = true`,
/// same flags and constraints) and keeps its `AlreadyInstalled`
/// verdicts as `(version, main slot)` pairs -- the instances the
/// arguments pin in the graph, whatever the walk order. Memoized per
/// pass in [`PassState::top_pinned_installed`] (a missing key means
/// uncomputed; the replace set and every other `BacktrackParams` input
/// can change the verdict per pass, so the cache never outlives one).
///
/// The caller applies the reuse only when its fresh verdict is a new
/// merge (`New`/`Upgrade`/`Downgrade`) in main slot `S`, and this
/// helper additionally requires: a pinned instance in that same slot
/// (real's same-`slot_atom` requirement -- a pin in another slot never
/// diverts an upgrade, e.g. `=mmprov-1` still upgrades slot 1), the dep
/// atom matching the pinned `cat/pkg-ver:slot/sub` string (real's
/// `atom.match`, the "only keeps it when it satisfies the dep atom"
/// half), and -- for `[use]` deps -- the pinned instance's own recorded
/// vdb `USE` satisfying them (the same check
/// [`dependency_avoid_update_candidate`] runs). Highest matching pin
/// wins (real iterates the tracker descending). A `Reinstall` verdict
/// is never overridden by the caller (something already decided that
/// version rebuilds); `resolve_pretend` writes no global state, so the
/// speculative resolutions are side-effect free.
///
/// Cuts (listed, not grown, per the slice): `||`-chosen atoms keep
/// their own in-graph preference; `@world`/`@selected` members are not
/// `ctx.atoms` and pin nothing here; cross-root atoms resolve against
/// the dep's own root.
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
    // The fresh pick's own main slot -- the reuse target must sit in it.
    // Same winner rule as the pop site's own `resolved` pick ("ebuild
    // type is the last resort", then repo priority).
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
            let mut valid = valid_iuse(&vdb_iuse, config);
            valid.extend(vdb_use.iter().cloned());
            if !portage_dep::use_deps_satisfied(use_deps, &valid, &vdb_use) {
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
