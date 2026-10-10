//! Parameters a backtracking pass runs with.

use crate::*;

/// Phase C3 (023): one ranked mask choice for a slot conflict: mask
/// `target` (a `(cat, pkg, ver)` triple), opposed by `parents` (the
/// pulling `((cat, pkg, ver), atom)` triples whose atom does NOT match
/// the masked version -- real's `conflict_atoms`). One choice becomes
/// one sibling node; DFS explores the last-added first.
/// C4: `similar` holds missed-update siblings masked in the SAME node
/// (real's `similar_pkgs` grouping): same `cat/pkg`, same slot, visible,
/// not excluded, not already masked, not already a conflict party, each
/// sharing at least one conflict atom with the target.
pub(crate) struct MaskChoice {
    pub(crate) target: (String, String, String),
    pub(crate) parents: Vec<((String, String, String), String)>,
    pub(crate) similar: Vec<SimilarMask>,
}

/// One missed-update sibling in a choice's mask group: mask its version
/// alongside the choice target, opposed by its own non-matching subset.
pub(crate) struct SimilarMask {
    pub(crate) target: (String, String, String),
    pub(crate) parents: Vec<((String, String, String), String)>,
}

/// Phase C3 (023): port of real
/// `lib/_emerge/depgraph.py::_slot_confict_backtrack` (note the upstream
/// one-`l` spelling), C4 extended with the `_iter_similar_available`
/// grouping: each choice masks its target plus missed-update siblings
/// in one node.
/// Ranked ascending: the in-graph resolved version is always a
/// candidate (real's bug-692746 append), candidates sort version-desc,
/// then stably by ascending `conflict_atoms` length; the driver adds the
/// nodes in that order so DFS pops the longest-conflict choice first,
/// exactly like real's sort-then-`pop()`. Candidate matching uses the
/// step-2 candidate-string form (`cat/pkg-ver:slot`, main slot only --
/// the same documented subslot approximation C1 used, kept so the two
/// call sites cannot disagree).
/// Two known ceilings, both documented (neither is exercised by any
/// fixture, and ranking-only effects cannot change the explored set,
/// only its order): candidates come from the recorded `instances`
/// (portuale keeps two per conflict -- existing + current -- where real
/// ranks every pulled version, so a 3+-version conflict drops its third
/// candidate), and `conflict_atoms` membership is version+slot only
/// (real joins atoms via `atom.match(pkg.with_use(...))`, so a parent
/// that version-matches but USE-mismatches counts as conflicting here
/// and merely inflates the choice's length).
/// Similar-grouping gates: same `cat/pkg` (the listing is scoped by cp),
/// same main slot, `is_visible` under the effective config (real passes
/// no `autounmask_level` at its own call site either), not `--exclude`d
/// (same `matches_config_entry` idiom as selection), not already masked
/// (exact-`!=cpv`-string presence -- same-version-different-repo is
/// treated as masked, absent from fixtures), and not already a conflict
/// party. The ebuild-only listing moots real's installed-skip and its
/// built/binpkg-visibility branches (no counterpart, documented cut).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_conflict_mask_choices(
    repos: &[RepoConfig],
    excluded: &[String],
    params: &BacktrackParams,
    slot_pullers: &SlotPullers,
    sc: &SlotConflict,
    config: &portage_profile::Config,
) -> Vec<MaskChoice> {
    // Every pulled version is a candidate, highest first (real's
    // `conflict_pkgs.sort(reverse=True)`).
    let mut versions: Vec<String> = sc.instances.iter().map(|i| i.version.clone()).collect();
    versions.sort_by(|a, b| vercmp_ordering(a, b));
    versions.reverse();
    versions.dedup();
    // Real's bug-692746 append: the resolved version is a candidate even
    // when every parent atom matches it (masking it avoids a missed
    // update). Portuale's `instances[0]` is always the resolved version,
    // so this only fires when the instance list is somehow empty.
    if !versions.contains(&sc.resolved_version) {
        versions.push(sc.resolved_version.clone());
    }
    // All pulling parents (real's `all_parents`), minus the
    // top-level-argument pseudo-pullers (empty `cat`), which match every
    // version and hence never discriminate -- real keeps `@selected` in
    // the set for the same null effect.
    let mut all_parents: Vec<((String, String, String), String)> = Vec::new();
    if let Some(pullers) = slot_pullers.get(&(sc.category.clone(), sc.package.clone())) {
        for (pc, pp, pv, atom) in pullers {
            if pc.is_empty() {
                continue;
            }
            let parent = ((pc.clone(), pp.clone(), pv.clone()), atom.clone());
            if !all_parents.contains(&parent) {
                all_parents.push(parent);
            }
        }
    }
    let mut choices: Vec<MaskChoice> = versions
        .into_iter()
        .map(|ver| {
            let candidate = format!("{}/{}-{}:{}", sc.category, sc.package, ver, sc.slot);
            let parents: Vec<((String, String, String), String)> = all_parents
                .iter()
                .filter(|(_, atom)| {
                    !portage_dep::match_from_list(atom, &[candidate.as_str()])
                        .is_some_and(|m| !m.is_empty())
                })
                .cloned()
                .collect();
            // C4: missed-update siblings join the choice's mask group
            // (real's `similar_pkgs`): same cp, same slot, visible under
            // the effective config, not `--exclude`d, not already masked
            // in the search state, not already a conflict party -- each
            // sharing at least one conflict atom with the target (else
            // masking it is pointless). Skipped entirely when the
            // target's own conflict set is empty (real's `if
            // conflict_atoms` gate). Highest version first, for a
            // deterministic group order.
            let mut similar: Vec<SimilarMask> = Vec::new();
            if !parents.is_empty() {
                let party_versions: Vec<&str> =
                    sc.instances.iter().map(|i| i.version.as_str()).collect();
                if let Ok(cands) = list_candidates(repos, &sc.category, &sc.package) {
                    let mut sibs: Vec<&Candidate> = Vec::new();
                    for c in cands.iter() {
                        if c.slot != sc.slot || party_versions.contains(&c.version.as_str()) {
                            continue;
                        }
                        if params
                            .runtime_pkg_mask
                            .get(&(sc.category.clone(), sc.package.clone()))
                            .is_some_and(|bucket| {
                                bucket.iter().any(|m| {
                                    m.neg
                                        == format!("!={}/{}-{}", sc.category, sc.package, c.version)
                                })
                            })
                        {
                            continue;
                        }
                        if !is_visible(c, &sc.category, &sc.package, config) {
                            continue;
                        }
                        let cand_str = format!(
                            "{}/{}-{}:{}/{}::{}",
                            sc.category, sc.package, c.version, c.slot, c.sub_slot, c.repo_name
                        );
                        if excluded.iter().any(|ex| {
                            matches_config_entry(ex, &cand_str, &sc.category, &sc.package)
                        }) {
                            continue;
                        }
                        sibs.push(c);
                    }
                    sibs.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));
                    for c in sibs {
                        let sim_cpv =
                            format!("{}/{}-{}:{}", sc.category, sc.package, c.version, sc.slot);
                        let subset: Vec<((String, String, String), String)> = parents
                            .iter()
                            .filter(|(_, atom)| {
                                !portage_dep::match_from_list(atom, &[sim_cpv.as_str()])
                                    .is_some_and(|m| !m.is_empty())
                            })
                            .cloned()
                            .collect();
                        if subset.is_empty() {
                            continue;
                        }
                        similar.push(SimilarMask {
                            target: (sc.category.clone(), sc.package.clone(), c.version.clone()),
                            parents: subset,
                        });
                    }
                }
            }
            MaskChoice {
                target: (sc.category.clone(), sc.package.clone(), ver),
                parents,
                similar,
            }
        })
        .collect();
    // Stable ascending length sort (real's `backtrack_data.sort`), so
    // equal-length choices keep version-desc order.
    choices.sort_by_key(|c| c.parents.len());
    choices
}

/// Phase C2 (023): one pass's outcome in real `_backtrack_depgraph`
/// terms. Every variant carries the overlay-merged working copy (`base`
/// for the mask kinds, `params` for `Config`): a pass that folded flips
/// or dropped pins AND hit a conflict/mask reports or retries with the
/// union, exactly as the old live-`bp` code did. `feedback` advances the
/// counters; mask kinds are non-terminal, `Config` preserves the current
/// terminal value. First hit wins, like the old step order (a pass with
/// both growth and a conflict takes the growth -- real would park the
/// config node and explore the conflict first; that combined ordering is
/// a deliberate C2 cut).
pub(crate) enum BacktrackFeedback {
    Config {
        params: Box<BacktrackParams>,
    },
    /// Backlog #209: the reverse-dependency feed (`_complete_graph`'s
    /// installed-consumer pins, `collect_feedback`'s scan). Real
    /// `_resolve_conflicts` (`_emerge/depgraph.py:9444`) calls
    /// `_complete_graph()` (`:8562`) with no `_allow_backtracking` gate,
    /// so these pins enforce in-pass even under `--backtrack=0`; the
    /// driver re-runs the pass with them instead of settling. Carries
    /// the same working copy as `Config` and behaves identically inside
    /// the search.
    RevDep {
        params: Box<BacktrackParams>,
    },
    SlotConflict {
        base: Box<BacktrackParams>,
        choices: Vec<MaskChoice>,
    },
    /// Backlog #216: the serialize dead-end retry (real
    /// `_serialize_tasks`' `not selected_nodes` branch setting
    /// `_need_restart` with a grown `circular_dependency` map). Carries
    /// the grown working copy like `Config`/`RevDep` and behaves
    /// identically inside the search (budget-free re-pass, not a mask
    /// step): the driver explores it when backtracking and settles the
    /// pass as-is under `--backtrack=0`, where it renders the circular
    /// abort like real's own pass-0 display.
    Circular {
        params: Box<BacktrackParams>,
    },
    MissingDep {
        base: Box<BacktrackParams>,
        owner: (String, String),
        entry: MaskEntry,
    },
}

/// Phase C2 (023): settle (report this pass with its merged working
/// copy), feed one outcome back into the search, or abandon this node.
/// `DeadEnd` is a pass real would not retry from: it carries an
/// unsatisfiable dependency no feedback covers (top-level or latched
/// parent, USE-only mismatch), i.e. real's `_create_graph` returning 0.
/// The node is consumed but keeps the merged copy (see
/// `Backtracker::adopt_current`); the search continues elsewhere and
/// `get_best_run` picks the report. Without this, such a pass would
/// settle straight into the abort path and hide the unmasked best run.
pub(crate) enum PassDecision {
    Settle { params: Box<BacktrackParams> },
    Feedback(BacktrackFeedback),
    DeadEnd { params: Box<BacktrackParams> },
}

/// Phase A1 (023): the cross-pass half of `backtracking_resolve`'s old
/// `let mut` block. Cloned per search node once C2 lands (hence `Clone`).
///
/// ## Real counterpart
///
/// Real `lib/_emerge/resolver/backtracking.py::BacktrackParameter` carries
/// `runtime_pkg_mask` (a `cpv -> mask-reason` dict fed by every feedback
/// path), `needed_use_config_changes` (the autounmask accumulator) and the
/// search bookkeeping (`depth`, `mask_steps`). Mapped here:
/// `slot_constraints` holds only the solvable-conflict atom sets
/// (portuale-specific positives); every `!=cpv` negative lives in
/// `runtime_pkg_mask` with its `MaskReason` (C1 split); readers see the
/// union via `union_constraints`.
/// `autounmask_use_config` ≈ `needed_use_config_changes`. The C1
/// trial quartet (`mask_phase`/`mask_trial_spent`/`mask_negatives`/
/// `pre_trial_nvc`) and the `Reverting` pass had no real counterpart --
/// portuale-only, removed in C2 when `get_best_run` replaced them.
/// `mask_steps`/`depth` are the per-node search position (real's node
/// attributes); `params_equal` excludes them, like real's parameter-only
/// `__eq__`.
#[derive(Debug, Clone, Default)]
pub(crate) struct BacktrackParams {
    /// `mut` so the `_autounmask_breakage` fallback (real
    /// depgraph.py:12262) can switch all four off for one final clean
    /// pass once an accumulated autounmask change turns out to break
    /// another dependency.
    pub(crate) autounmask_suggest_keywords: bool,
    pub(crate) autounmask_suggest_use: bool,
    pub(crate) autounmask_suggest_license: bool,
    pub(crate) autounmask_suggest_masks: bool,
    /// Real `backtracking.py::_feedback_missing_dep`: a dependency atom
    /// with no matching package makes `_backtrack_depgraph` mask that
    /// atom's *parent* and retry, so `dep_zapdeps` re-picks a `||` group
    /// whose chosen alternative had an unsatisfiable subtree. The latch
    /// (real's `dep.parent not in _runtime_pkg_mask` check) holds every
    /// `!=parent-cpv` already tried, guaranteeing termination.
    pub(crate) missing_dep_masked: HashSet<String>,
    /// Real `_complete_graph`'s own parent atoms, discovered by vdb
    /// reverse scan -- see `reverse_dependency_constraints`. Latches every
    /// `(cat/pkg, atom)` already folded into `slot_constraints` so a
    /// constraint is never re-added and the `'backtrack` loop converges.
    pub(crate) reverse_dep_masked: HashSet<((String, String), String)>,
    /// Dropped reverse-dep pins (see `RevDepPin`), accumulated across
    /// passes, for the residual installed-instance conflict report once
    /// the graph settles (see `build_residual_slot_conflicts`). Deduped
    /// by `(cp, atom, consumer)`; stale entries (a later pass picked a
    /// compatible version after all) simply yield no record.
    pub(crate) dropped_pins: Vec<RevDepPin>,
    /// Backlog #90 (S2) + #92: enforced holdable pins, accumulated
    /// across passes like `dropped_pins`, so `assemble_result` can
    /// report the upgrades they withhold (no conflict ever forms on
    /// that path) through the same skip-conflict `WARNING`.
    pub(crate) reverse_dep_pins: Vec<RevDepPin>,
    /// Real `_emerge/resolver/backtracking.py`: the resolver runs the whole
    /// graph walk, and if the first pass hits a *solvable* slot conflict
    /// (real `_process_slot_conflicts` -- a single version can satisfy
    /// every parent atom that landed on the slot), it records the extra
    /// constraints and re-runs the entire walk from scratch. `entries` and
    /// every other per-pass accumulator is rebuilt each iteration; only
    /// this map (keyed by `cat/pkg`, the union of every atom text that
    /// targeted a conflicted package) and the other cross-pass
    /// accumulators survive across attempts. Since C1 this map
    /// holds only the positive atom sets; negatives live in
    /// `runtime_pkg_mask` (readers use `union_constraints`).
    pub(crate) slot_constraints: HashMap<(String, String), Vec<String>>,
    /// C1 split of the old combined map: every `!=cpv` negative with its
    /// `MaskReason`, keyed by the masked version's `cat/pkg` (real keys by
    /// package; the per-`cat/pkg` bucket keeps every reader's lookup shape).
    pub(crate) runtime_pkg_mask: HashMap<(String, String), Vec<MaskEntry>>,
    /// Backlog #129 (S1): first-insertion order of the `runtime_pkg_mask`
    /// negatives above, as `((cat, pkg), neg)` pairs across buckets. Real's
    /// `_get_missed_updates` iterates the mask dict in insertion order and
    /// the notice renders in that order, while portuale's buckets hash
    /// theirs away; this recovers it. Same-state dedup ignores it (same
    /// masks are the same search state whatever order they arrived in --
    /// see `params_equal`, which deliberately does not compare it, like
    /// real's order-insensitive parameter `__eq__`). Appended exactly
    /// where a negative is first pushed (a reason *replace* keeps its
    /// first-seen position, like real's dict assignment).
    pub(crate) mask_order: Vec<((String, String), String)>,
    /// Mask-producing retries (slot-conflict choices, missing-dep
    /// parents). Real's `--backtrack=N` bounds this per node
    /// (`Backtracker._add`), not the pass count; advanced by
    /// `Backtracker::feedback`, never in the walk.
    pub(crate) mask_steps: u32,
    /// Every retry including config-growth passes. Real's node depth;
    /// advanced by `Backtracker::feedback`, never in the walk.
    pub(crate) depth: u32,
    /// Backtracking slice: `--autounmask-use` changes fed back into the
    /// loop (real `_dynamic_config._needed_use_config_changes`, applied by
    /// `backtracking.py::_feedback_config` on the next attempt). A
    /// dependency atom `X[flag]` that lands on an *already-resolved* slot
    /// whose package can't satisfy `[flag]` -- but a `package.use` flip
    /// could -- folds the flip in here (`(cat, pkg) -> {flag -> desired}`)
    /// and re-runs the whole walk, so the freshly-walked package's
    /// `flag?`-gated deps appear. Survives across passes like
    /// `slot_constraints`; converges when a pass adds nothing new.
    /// Written in-walk (see the A3 overlay); read back within the same pass.
    pub(crate) autounmask_use_config: HashMap<(String, String), HashMap<String, bool>>,
    /// The `The following USE changes are necessary to proceed:` block
    /// entries recorded at the moment each flip is folded in above --
    /// recorded here (not the per-pass `autounmask_use_changes`) because a
    /// `continue 'backtrack` discards the pass, and after the re-resolve
    /// the atom's `[flag]` is *satisfied* (by the accumulator) so nothing
    /// re-records it. Merged into `autounmask_use_changes` once the graph
    /// settles. Deduped by `(atom, token)`.
    /// Written in-walk (see the A3 overlay); read back within the same pass.
    pub(crate) autounmask_use_change_records: Vec<AutounmaskChange>,
    /// `req.config` with `autounmask_use` filled from the accumulator --
    /// rebuilt whenever the accumulator grows, `None` until then (so the
    /// common no-autounmask path never clones `Config`).
    pub(crate) backtrack_config: Option<portage_profile::Config>,
    /// Real `_autounmask_breakage` (depgraph.py:12262): once an accumulated
    /// autounmask USE change turns out to make some *other* use-dep
    /// unsatisfiable and cannot be reconciled -- the same flag ends up
    /// wanted both on and off -- portage abandons autounmask entirely
    /// (`myparams["autounmask"] = False`) and re-resolves one final clean
    /// pass. This latch holds the contradiction; the driver acts on it
    /// once and sets `autounmask_disabled` so it can't re-fire.
    /// Written in-walk (see the A3 overlay); read back within the same pass.
    pub(crate) autounmask_use_broke: bool,
    pub(crate) autounmask_disabled: bool,
    /// Backlog #24 S3: real `_dynamic_config._slot_operator_replace_installed`
    /// (`depgraph.py:2400`/`2361`/`2881` write it, `resolver/backtracking.py::
    /// _feedback_config` 237-257 consumes it, `_gen_reinstall_sets` 5457-5480
    /// turns it into the pseudo `SetArg` `__auto_slot_operator_replace_installed__`
    /// with `force_reinstall=True`, `reset_depth=False`). An installed consumer
    /// whose built `cat/pkg:S/SS=` dep no longer matches how this run leaves
    /// the provider is added here by `collect_feedback`'s scan; the next pass
    /// seeds the walk with its `cat/pkg` atom and flips its
    /// `AlreadyInstalled` outcome to `Reinstall { slot_operator_rebuild: true }`,
    /// so the consumer is a **walked graph node** -- its tree `RDEPEND` is
    /// re-evaluated, its `:=` re-bound against the graph, and its merge-order
    /// edges real.
    ///
    /// Narrowing: real keys `(root, slot_atom)` (`_replace_installed_atom`
    /// 3059-3087 returns the *available* ebuild's `slot_atom`). Portuale
    /// resolves one instance per `cat/pkg` (`resolved_slots` is keyed by
    /// `(cat, pkg, slot)` but a dependency walk never pulls two slots of one
    /// cp), so `(cat, pkg)` carries the same information here. `BTreeSet` for
    /// deterministic seed order.
    pub(crate) slot_operator_replace_installed: BTreeSet<(String, String)>,
    /// #236 A: the child half of real `_slot_operator_update_backtrack`
    /// (`_emerge/depgraph.py:2400-2452`) -- an installed child whose
    /// update the probe found joins the replace set there, and
    /// `@__auto_slot_operator_replace_installed__`'s `force_reinstall`
    /// (installed instances excluded from selection) makes the restart
    /// pick the probe's candidate. Portuale's replace-set flip reinstalls
    /// the same version, so the child keeps its own record here:
    /// `(category, package, version)` of the candidate the restart must
    /// select. Filled by [`slot_operator_update_force_scan`].
    pub(crate) slot_operator_forced_upgrade: BTreeSet<(String, String, String)>,
    /// Backlog #24 S4: the undo latch. Real `_eliminate_rebuilds`
    /// (3859-4000) re-adds the installed instance in the rebuilt pkg's
    /// place and never sets `_need_restart`, so a within-node redo cannot
    /// re-schedule it; portuale's S3 scan re-runs every pass on a fresh
    /// graph, so a demoted cp must be remembered here or the scan would
    /// immediately re-add it (its vdb `:S/SS=` binding is still stale --
    /// that is *why* rule 8 demoted it). Monotone and bounded by the
    /// installed cp count, so the search terminates. `BTreeSet` for
    /// deterministic order; included in `params_equal`, unlike the
    /// caller-side `already` set (real has no analogue of this latch to
    /// compare -- its undo mutates the graph in place).
    pub(crate) slot_operator_undone: BTreeSet<(String, String)>,
    /// Backlog #213 (v2 `#24d`): the prune latch. Real restarts with
    /// `config["prune_rebuilds"]` when `_slot_operator_replace_installed`
    /// meets missed updates (`_resolve`, `depgraph.py:5763-5780`;
    /// `_ENABLE_PRUNE_REBUILDS` `:628`, always true upstream; the
    /// `prune_rebuilds` backtrack parameter `:710`, consumed by
    /// `resolver/backtracking.py::_feedback_config:247-257`). Included
    /// in `params_equal` like real's parameter `__eq__`
    /// (`backtracking.py:76`), which compares `prune_rebuilds`.
    pub(crate) prune_rebuilds: bool,
    /// Backlog #221: real `backtrack_infos["config"]["circular_dependency"]`
    /// (`_emerge/depgraph.py:10270-10287`, written by `_serialize_tasks`
    /// when no leaf node selects and consumed by `resolver/backtracking.py::
    /// _feedback_config` on the restart). When the merge-order walk strands
    /// on a dependency cycle real does not fail at once: it records, for
    /// every cycle node, the in-cycle package it depends on
    /// (`circular_dependency.setdefault(node, set()).add(circular_child)`)
    /// and re-runs the whole walk, on which `dep_zapdeps`
    /// (`lib/portage/dep/dep_check.py:673-691`) demotes any `||`
    /// alternative with an atom matching such a child (`circular_atom`)
    /// to the `other` bin, so the walk takes the branch that breaks the
    /// cycle (bugs 703440, 705986). Keyed by the cycle node's `cat/pkg`
    /// (real keys exact package nodes; portuale resolves one instance per
    /// `cat/pkg`, so the cp carries the same information -- see
    /// `slot_operator_replace_installed`'s own narrowing note). A retry
    /// whose cycle re-uses an already-recorded node is unsolved (real's
    /// `unsolved_cycle`) and settles into the circular report instead of
    /// retrying, so the map strictly grows per retry and the search
    /// terminates. Included in `params_equal`: a grown map is a distinct
    /// search state, or the retry node would dedup against its parent.
    /// Backlog #216's fresh-self-loop records share this map (same
    /// rotation, `cycle_restartable`'s scope gate in `collect_feedback`):
    /// one field, structured children, both triggers.
    pub(crate) circular_dependency: HashMap<RootCp, Vec<CircularDepChild>>,
}

/// Backlog #221: one in-cycle dependency of a `circular_dependency` parent
/// above -- real's `circular_child` package object, reduced to the fields
/// `atom.match(child)` needs: the child's own `cat/pkg-version` plus the
/// slot/sub-slot an atom's `:slot` pin would match against
/// (`match_from_list` on the assembled candidate string, the same shape
/// `best_installed_matching` matches installed candidates with).
/// `pub` (fields private) so the CLI crate can carry the settling pass's
/// map into its own `kept_alt_branches` / `resolved_dep_targets` display
/// derivations, which must demote exactly like the walk did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CircularDepChild {
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) slot: Option<String>,
    pub(crate) sub_slot: Option<String>,
}

/// The `cat/pkg-version[:slot[/sub]]` candidate string real's
/// `atom.match(circular_child)` sees, shared by the walk's demotion and
/// the merge-order suppression's (see `suppressed_alt_edges`): the two
/// must agree on what matches, or the re-derivation keeps the branch the
/// walk dropped and a phantom edge re-closes the cycle.
pub(crate) fn circular_child_candidate(child: &CircularDepChild) -> String {
    let mut candidate = format!("{}/{}-{}", child.category, child.package, child.version);
    if let Some(slot) = &child.slot {
        candidate.push(':');
        candidate.push_str(slot);
        if let Some(sub) = &child.sub_slot {
            candidate.push('/');
            candidate.push_str(sub);
        }
    }
    candidate
}

impl BacktrackParams {
    /// The root node's params: the caller's suggestion flags, everything
    /// else default (no constraints, no masks, step counters at zero).
    pub(crate) fn initial(req: &ResolveRequest) -> Self {
        BacktrackParams {
            autounmask_suggest_keywords: req.autounmask_suggest_keywords,
            autounmask_suggest_use: req.autounmask_suggest_use,
            autounmask_suggest_license: req.autounmask_suggest_license,
            autounmask_suggest_masks: req.autounmask_suggest_masks,
            ..Default::default()
        }
    }

    /// Effective per-`cat/pkg` constraint bucket for readers: the
    /// solvable-conflict positives plus the rendered `runtime_pkg_mask`
    /// negatives. Order within a bucket is positives-then-negatives; every
    /// consumer applies the entries as an order-independent conjunction
    /// (see the `!`-prefix handling in `atom_currently_satisfiable` and
    /// `resolve_pretend`), so this matches the old combined map exactly.
    /// Built once per pass in `run_pass` -- `bp` is never mutated
    /// mid-walk, so the union is pass-constant.
    pub(crate) fn union_constraints(&self) -> HashMap<(String, String), Vec<String>> {
        let mut out = self.slot_constraints.clone();
        for (cp, masks) in &self.runtime_pkg_mask {
            out.entry(cp.clone())
                .or_default()
                .extend(masks.iter().map(|m| m.neg.clone()));
        }
        out
    }

    /// Just the rendered `runtime_pkg_mask` negatives for one `cat/pkg`
    /// (no positives): what real's dep-level `runtime_pkg_mask` probe
    /// sees. Used by the missing-dep trigger to tell mask-induced
    /// unsatisfiability (retry with the parent masked) from genuine
    /// absence and USE-only failure.
    pub(crate) fn masked_negatives_for(&self, cp: &(String, String)) -> Vec<String> {
        self.runtime_pkg_mask
            .get(cp)
            .map(|bucket| bucket.iter().map(|m| m.neg.clone()).collect())
            .unwrap_or_default()
    }
}
