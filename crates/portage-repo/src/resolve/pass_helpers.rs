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
