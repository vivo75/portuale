//! Greedy installed-slot arguments for the default solver.
//!
//! Under `--update` Portage appends one `cp:slot` argument per installed slot
//! of the same cp whose own best visible match is lower than the overall best
//! (`select_files`), then repeats the selection with a blocker lookahead that
//! drops slots conflicting with the best (`_greedy_slots`). Only plain atom
//! arguments participate, never set members.

use crate::*;
use portage_dep::Atom;

/// The values [`greedy_slots_below_best`] reads from its caller.
pub(crate) struct GreedySlotsBelowBestArgs<'a, V> {
    pub(crate) top_slot: &'a str,
    pub(crate) top: &'a V,
    pub(crate) per_slot_best: &'a [(&'a str, &'a V)],
    pub(crate) installed_slots: &'a [String],
    pub(crate) update: bool,
}

/// Portage `_greedy_slots` slot filter shared by both solvers: the top slot,
/// then each installed slot (in the caller's order) whose own best is strictly
/// below the top. Pure over caller-supplied bests so the default solver
/// (string versions ordered by `vercmp_ordering`) and the pubgrub bridge
/// (`portage_solver::Version`) share it instead of copying the filter.
// Portage: _emerge/depgraph.py, _greedy_slots
pub(crate) fn greedy_slots_below_best<V>(
    args: GreedySlotsBelowBestArgs<'_, V>,
    is_below: impl Fn(&V, &V) -> bool,
) -> Vec<String> {
    let GreedySlotsBelowBestArgs {
        top_slot,
        top,
        per_slot_best,
        installed_slots,
        update,
    } = args;
    let mut slots = vec![top_slot.to_string()];
    if update {
        for slot in installed_slots {
            if slot != top_slot
                && !slots.contains(slot)
                && per_slot_best
                    .iter()
                    .find(|(s, _)| *s == slot)
                    .is_some_and(|(_, v)| is_below(v, top))
            {
                slots.push(slot.clone());
            }
        }
    }
    slots
}

/// The values [`greedy_slot_atoms`] reads from its caller.
pub(crate) struct GreedySlotsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Extra `cp:slot` seed atoms, one per installed slot below the best visible
/// match of each non-set top-level atom under `--update` (both Portage
/// passes: the plain expansion, then the blocker-lookahead revision).
/// Recomputed every pass so a backtrack mask moves the expansion with the
/// selection it mirrors.
// Portage: _emerge/depgraph.py, select_files; _greedy_slots
pub(crate) fn greedy_slot_atoms(args: GreedySlotsArgs<'_>) -> Vec<String> {
    let GreedySlotsArgs {
        ctx,
        config,
        union_constraints,
    } = args;
    if !ctx.update {
        return Vec::new();
    }
    let empty_constraints: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for atom_str in ctx.atoms {
        // Only `AtomArg` participates, never set members.
        // Portage: depgraph.py, select_files (`isinstance(arg, AtomArg)`)
        if ctx.set_args.contains(atom_str) || ctx.top_level.sets.contains_key(atom_str) {
            continue;
        }
        // An `AtomArg` always names a category (`"/" in x`); anything else
        // (and blockers, which are never valid targets) cannot expand.
        if !atom_str.contains('/') {
            continue;
        }
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        let key = (atom.category.clone(), atom.package.clone());
        let extra = union_constraints.get(&key).unwrap_or(&empty_constraints);
        let pool = greedy_pool(ctx, config, &atom, atom_str, extra);
        let Some(top) = greedy_best_for(&pool, atom_str) else {
            continue;
        };
        // Installed slots matching the original atom, same cp only: Portage
        // reads `vardb.match(atom)` and skips mixed virtuals via
        // `pkg.cp == highest_pkg.cp`.
        let installed = installed_candidates(ctx.root, &atom.category, &atom.package);
        let installed_strs: Vec<String> = installed
            .iter()
            .map(|(v, s, ss)| format!("{}/{}-{v}:{s}/{ss}", atom.category, atom.package))
            .collect();
        let installed_refs: Vec<&str> = installed_strs.iter().map(String::as_str).collect();
        let mut installed_slots: Vec<String> =
            portage_dep::match_from_list(atom_str, &installed_refs)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|m| {
                    installed_strs
                        .iter()
                        .position(|s| s == m)
                        .and_then(|i| installed.get(i))
                        .map(|(_, s, _)| s.clone())
                })
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
        // Sorted: Portage pops a hash set (pinned by `PYTHONHASHSEED` in the
        // captures); the suite needs a stable order, so slots seed low first.
        installed_slots.sort();
        if installed_slots.iter().all(|s| s == &top.candidate.slot) {
            continue;
        }
        // Each installed slot's own best visible match for the bare `cp:slot`
        // atom (no version range: Portage builds the slot atom from the cp
        // alone).
        let mut per_slot_best: Vec<(String, &Candidate)> = Vec::new();
        for slot in &installed_slots {
            if slot == &top.candidate.slot {
                continue;
            }
            let slot_atom = format!("{}/{}:{slot}", atom.category, atom.package);
            if let Some(best) = greedy_best_for(&pool, &slot_atom) {
                per_slot_best.push((slot.clone(), best.candidate));
            }
        }
        let pairs: Vec<(&str, &String)> = per_slot_best
            .iter()
            .map(|(s, c)| (s.as_str(), &c.version))
            .collect();
        let mut slots = greedy_slots_below_best(
            GreedySlotsBelowBestArgs {
                top_slot: &top.candidate.slot,
                top: &top.candidate.version,
                per_slot_best: &pairs,
                installed_slots: &installed_slots,
                update: true,
            },
            |a, b| vercmp_ordering(a, b) == std::cmp::Ordering::Less,
        );
        // The shared filter returns the top slot first; only the added slots
        // become arguments.
        slots.remove(0);
        // The blocker-lookahead revision: drop a slot whose best package and
        // the highest package block each other, then drop lower versions of
        // mutually conflicting pairs.
        // Portage: _emerge/depgraph.py, _greedy_slots (blocker_lookahead)
        for slot in greedy_lookahead(config, &key, top.candidate, slots, &per_slot_best) {
            out.push(format!("{}/{}:{slot}", atom.category, atom.package));
        }
    }
    out
}

/// One pool entry for the greedy choice: the candidate plus its match string.
struct GreedyEntry {
    candidate: Candidate,
    text: String,
}

/// The visible, constraint-satisfying pool for one cp: ebuilds (unless
/// `--usepkgonly`) plus usable binaries, so `--usepkg` runs agree with the
/// walk on the highest.
// Portage: depgraph.py dbs construction; list_binary_candidates
fn greedy_pool(
    ctx: &ResolveCtx<'_>,
    config: &portage_profile::Config,
    atom: &Atom,
    atom_str: &str,
    extra: &[String],
) -> Vec<GreedyEntry> {
    let ebuilds_shared: Option<std::sync::Arc<Vec<Candidate>>> =
        if ctx.usepkgonly || crate::globals::ebuild_db_dropped() {
            None
        } else {
            list_candidates(&ctx.repos, &atom.category, &atom.package).ok()
        };
    let mut candidates = ebuilds_shared.as_deref().cloned().unwrap_or_default();
    crate::resolve::pool_memo::collect_memoized(
        CollectBinaryCandidatesArgs {
            usepkg: ctx.usepkg,
            usepkgonly: ctx.usepkgonly,
            local_binpkg: &ctx.local_binpkg,
            atom,
            getbinpkg: ctx.getbinpkg,
            config,
            root: ctx.root,
            binpkg_respect_use: ctx.binpkg_respect_use,
            newuse: ctx.newuse,
            changed_use: ctx.changed_use,
            candidates: &mut candidates,
            repos: &ctx.repos,
            with_bdeps: ctx.with_bdeps,
            atom_str,
            extra_constraints: extra,
            usepkg_exclude: ctx.usepkg_exclude,
            usepkg_include: ctx.usepkg_include,
        },
        ebuilds_shared.as_ref(),
    );
    candidates
        .into_iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .map(|c| {
            let text = format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            );
            GreedyEntry { candidate: c, text }
        })
        .filter(|e| {
            // The walk narrows USE deps and autounmask flips later,
            // identically for the queued greedy seeds, so the expansion reads
            // the same shortlist the walk starts from.
            satisfies_extra_constraints(&e.candidate, atom, extra)
        })
        .collect()
}

/// The highest pool entry matching `atom_str`: best version, repo-priority
/// tiebreak, like the walk's own best choice.
// Portage: portdbapi.cp_list (pkg.version, repo.priority) sort
fn greedy_best_for<'a>(pool: &'a [GreedyEntry], atom_str: &str) -> Option<GreedyBest<'a>> {
    let refs: Vec<&str> = pool.iter().map(|e| e.text.as_str()).collect();
    let matched = portage_dep::match_from_list(atom_str, &refs)?;
    matched
        .into_iter()
        .filter_map(|m| pool.iter().find(|e| e.text == m))
        .max_by(|a, b| {
            vercmp_ordering(&a.candidate.version, &b.candidate.version)
                .then(a.candidate.repo_priority.cmp(&b.candidate.repo_priority))
        })
        .map(|e| GreedyBest {
            candidate: &e.candidate,
        })
}

/// The highest visible match: a borrow of the winning pool entry.
struct GreedyBest<'a> {
    candidate: &'a Candidate,
}

/// Blocker-lookahead revision over the expanded slots.
// Portage: _emerge/depgraph.py, _greedy_slots (blocker_lookahead=True)
pub(crate) fn greedy_lookahead(
    config: &portage_profile::Config,
    key: &(String, String),
    top: &Candidate,
    slots: Vec<String>,
    per_slot_best: &[(String, &Candidate)],
) -> Vec<String> {
    // Blocker atoms per surviving package; `None` marks an unreadable dep
    // string (Portage `InvalidDependString`).
    let Some(top_blockers) = blocker_atoms_for(config, key, top) else {
        return Vec::new();
    };
    let mut kept: Vec<(String, &Candidate, Vec<String>)> = Vec::new();
    for slot in &slots {
        let Some((_, cand)) = per_slot_best.iter().find(|(s, _)| s == slot) else {
            continue;
        };
        let Some(blockers) = blocker_atoms_for(config, key, cand) else {
            continue;
        };
        kept.push((slot.clone(), cand, blockers));
    }
    if kept.is_empty() {
        return Vec::new();
    }
    // Drop slots conflicting with the highest package, either direction.
    let top_str = greedy_candidate_str(key, top);
    let (top_use, top_iuse) = greedy_use_sets(config, key, top);
    kept.retain(|(_, cand, blockers)| {
        let other_str = greedy_candidate_str(key, cand);
        let (other_use, other_iuse) = greedy_use_sets(config, key, cand);
        !blocker_hits(&top_blockers, &other_str, &other_use, &other_iuse)
            && !blocker_hits(blockers, &top_str, &top_use, &top_iuse)
    });
    // Of mutually conflicting survivors, discard the lower version.
    kept.sort_by(|a, b| vercmp_ordering(&b.1.version, &a.1.version));
    let mut discard: Vec<usize> = Vec::new();
    for i in 0..kept.len() {
        if discard.contains(&i) {
            continue;
        }
        for j in (i + 1)..kept.len() {
            if discard.contains(&j) {
                continue;
            }
            let a_str = greedy_candidate_str(key, kept[i].1);
            let b_str = greedy_candidate_str(key, kept[j].1);
            let (a_use, a_iuse) = greedy_use_sets(config, key, kept[i].1);
            let (b_use, b_iuse) = greedy_use_sets(config, key, kept[j].1);
            if blocker_hits(&kept[i].2, &b_str, &b_use, &b_iuse)
                || blocker_hits(&kept[j].2, &a_str, &a_use, &a_iuse)
            {
                discard.push(j);
            }
        }
    }
    kept.into_iter()
        .enumerate()
        .filter(|(i, _)| !discard.contains(i))
        .map(|(_, (slot, _, _))| slot)
        .collect()
}

/// `cat/pkg-ver:slot/subslot` for blocker matching.
fn greedy_candidate_str(key: &(String, String), cand: &Candidate) -> String {
    format!(
        "{}/{}-{}:{}/{}",
        key.0, key.1, cand.version, cand.slot, cand.sub_slot
    )
}

/// Effective USE plus IUSE flag names for one tree candidate.
fn greedy_use_sets(
    config: &portage_profile::Config,
    key: &(String, String),
    cand: &Candidate,
) -> (HashSet<String>, HashSet<String>) {
    let pf = format!("{}-{}", key.1, cand.version);
    let meta = repo_aux_metadata(&cand.repo_location, &key.0, &pf).ok();
    let raw_iuse = meta
        .as_ref()
        .and_then(|m| m.get("IUSE"))
        .map(String::as_str)
        .unwrap_or("");
    let iuse: HashSet<String> = raw_iuse
        .split_whitespace()
        .map(|f| f.trim_start_matches(['+', '-']).to_string())
        .filter(|f| !f.is_empty())
        .collect();
    let candidate_str = format!(
        "{}/{}-{}:{}/{}::{}",
        key.0, key.1, cand.version, cand.slot, cand.sub_slot, cand.repo_name
    );
    let enabled = effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse: raw_iuse,
        keywords: &cand.keywords,
        candidate_str: &candidate_str,
        build_id: cand.build_id.as_deref(),
        category: &key.0,
        package: &key.1,
    })
    .as_ref()
    .clone();
    (enabled, iuse)
}

/// Blocker atoms from one tree candidate's dep keys, reduced against its
/// effective USE. `None` when the dep string does not parse or the metadata
/// is unreadable (Portage `InvalidDependString`: the package drops out of
/// the lookahead).
// Portage: _emerge/depgraph.py, _greedy_slots; Package._dep_keys
fn blocker_atoms_for(
    config: &portage_profile::Config,
    key: &(String, String),
    cand: &Candidate,
) -> Option<Vec<String>> {
    let pf = format!("{}-{}", key.1, cand.version);
    let meta = repo_aux_metadata(&cand.repo_location, &key.0, &pf).ok()?;
    let mut depstr = String::new();
    for k in ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"] {
        depstr.push_str(meta.get(k).map(String::as_str).unwrap_or(""));
        depstr.push(' ');
    }
    let candidate_str = format!(
        "{}/{}-{}:{}/{}::{}",
        key.0, key.1, cand.version, cand.slot, cand.sub_slot, cand.repo_name
    );
    let use_flags = effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse: meta.get("IUSE").map(String::as_str).unwrap_or(""),
        keywords: &cand.keywords,
        candidate_str: &candidate_str,
        build_id: cand.build_id.as_deref(),
        category: &key.0,
        package: &key.1,
    });
    let flat = flat_dep_atoms(&depstr, &use_flags)?;
    Some(
        flat.into_iter()
            .filter(|t| {
                // Blocker-ness comes from the raw token: the strip below (which
                // `match_from_list` applies too) parses the bare form.
                t.starts_with('!') && portage_dep::parse_atom(t.trim_start_matches('!')).is_some()
            })
            .collect(),
    )
}

/// Whether any blocker in `blockers` hits `other_str`, honouring the
/// blocker's own `[use]` deps against the other's USE.
fn blocker_hits(
    blockers: &[String],
    other_str: &str,
    other_use: &HashSet<String>,
    other_iuse: &HashSet<String>,
) -> bool {
    blockers.iter().any(|b| {
        // Blocker-ness comes from the raw token: the strip below (which
        // `match_from_list` applies too) parses the bare form.
        if !b.starts_with('!') {
            return false;
        }
        let Some(atom) = portage_dep::parse_atom(b.trim_start_matches('!')) else {
            return false;
        };
        // `match_from_list` strips the blocker itself and matches the bare
        // form (version, slot, repo).
        if portage_dep::match_from_list(b, &[other_str]).is_none_or(|m| m.is_empty()) {
            return false;
        }
        let use_deps = atom.use_deps.unwrap_or_default();
        use_deps.is_empty() || portage_dep::use_deps_satisfied(&use_deps, other_iuse, other_use)
    })
}
