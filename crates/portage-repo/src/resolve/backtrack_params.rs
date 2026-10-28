//! Parameters a backtracking pass runs with.

use crate::*;

/// One ranked mask choice for a slot conflict: mask `target`, opposed by
/// `parents` (pullers whose atom does NOT match the masked version). One
/// choice becomes one sibling node; DFS explores the last-added first.
/// `similar` holds missed-update siblings masked in the SAME node.
/// Portage: depgraph.py, _slot_confict_backtrack + _iter_similar_available.
pub(crate) struct MaskChoice {
    pub(crate) target: (String, String, String),
    pub(crate) parents: Vec<((String, String, String), String)>,
    pub(crate) similar: Vec<SimilarMask>,
    /// The target is an installed instance of the conflict: its mask also hides the
    /// installed package itself, as Portage's group includes the installed `Package`.
    pub(crate) installed: bool,
}

/// One missed-update sibling in a choice's mask group: mask its version
/// alongside the choice target, opposed by its own non-matching subset.
pub(crate) struct SimilarMask {
    pub(crate) target: (String, String, String),
    pub(crate) parents: Vec<((String, String, String), String)>,
}

/// The `(category, package, version)` of a record parent's `cat/pkg-ver:slot/sub::repo`
/// text; all empty for a top-level argument.
pub(crate) fn conflict_parent_key(parent_cpv: &str) -> (String, String, String) {
    let head = parent_cpv.split(':').next().unwrap_or_default();
    split_cpv(head).unwrap_or_default()
}

/// Arguments of [`slot_conflict_mask_choices`].
pub(crate) struct SlotConflictMaskChoicesArgs<'a> {
    pub(crate) excluded: &'a [String],
    pub(crate) params: &'a BacktrackParams,
    pub(crate) slot_pullers: &'a SlotPullers,
    pub(crate) sc: &'a SlotConflict,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
}

/// Ranked mask choices for a slot conflict: each choice masks its target plus
/// missed-update siblings in one node. The in-graph resolved version is always
/// a candidate; candidates sort version-desc, then stably by ascending
/// conflict-atom count, so DFS pops the longest-conflict choice first.
/// Portage: depgraph.py, _slot_confict_backtrack (note the upstream one-`l`
/// spelling) + _iter_similar_available.
///
/// Candidate matching uses `cat/pkg-ver:slot` (main slot only). Two known
/// ceilings (ranking-only, neither exercised by any fixture): candidates come
/// from the recorded `instances`, and conflict-atom membership is version+slot
/// only.
pub(crate) fn slot_conflict_mask_choices(args: SlotConflictMaskChoicesArgs<'_>) -> Vec<MaskChoice> {
    let SlotConflictMaskChoicesArgs {
        excluded,
        params,
        slot_pullers,
        sc,
        config,
        gate,
    } = args;
    // Every pulled version is a candidate, highest first.
    let mut versions: Vec<String> = sc.instances.iter().map(|i| i.version.clone()).collect();
    versions.sort_by(|a, b| vercmp_ordering(a, b));
    versions.reverse();
    versions.dedup();
    // The resolved version is a candidate even when every parent atom
    // matches it (masking it avoids a missed update).
    if !versions.contains(&sc.resolved_version) {
        versions.push(sc.resolved_version.clone());
    }
    // Every parent of every slot node: the hard pullers, then the parents the
    // record itself carries (an installed consumer's atoms). Argument parents
    // count: a versioned argument atom opposes the versions it misses.
    let mut all_parents: Vec<((String, String, String), String)> = Vec::new();
    let record_parents = sc
        .instances
        .iter()
        .flat_map(|i| i.parents.iter())
        .map(|p| (conflict_parent_key(&p.parent_cpv), p.atom.clone()));
    let puller_parents = slot_pullers
        .get(&(sc.category.clone(), sc.package.clone()))
        .into_iter()
        .flatten()
        .map(|(pc, pp, pv, atom)| ((pc.clone(), pp.clone(), pv.clone()), atom.clone()));
    for parent in puller_parents.chain(record_parents) {
        if !all_parents.contains(&parent) {
            all_parents.push(parent);
        }
    }
    // The candidate string of one version: slot and sub-slot, so a built
    // `:S/SS=` atom that misses the sub-slot counts against it.
    let candidate_of = |ver: &str| -> String {
        let sub = sc
            .instances
            .iter()
            .find(|i| i.version == ver)
            .map(|i| i.sub_slot.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                Some(slot_conflict_meta(gate, &sc.category, &sc.package, ver).0)
                    .filter(|s| !s.is_empty())
            });
        match sub {
            // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
            Some(sub) => format!("{}/{}-{}:{}/{}", sc.category, sc.package, ver, sc.slot, sub),
            // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
            None => format!("{}/{}-{}:{}", sc.category, sc.package, ver, sc.slot),
        }
    };
    let mut choices: Vec<MaskChoice> = versions
        .into_iter()
        .map(|ver| {
            let candidate = candidate_of(&ver);
            let parents: Vec<((String, String, String), String)> = all_parents
                .iter()
                .filter(|(_, atom)| {
                    !portage_dep::match_from_list(atom, &[candidate.as_str()])
                        .is_some_and(|m| !m.is_empty())
                })
                .cloned()
                .collect();
            // The skip block prints a recorded built `:S/SS=` atom ahead of the
            // plain ones.
            let (built, plain): (Vec<_>, Vec<_>) = parents.into_iter().partition(|(_, atom)| {
                parse_dep_atom(atom, "mask_choice_parents").is_some_and(|a| is_built_slot_op(&a))
            });
            let parents: Vec<((String, String, String), String)> =
                built.into_iter().chain(plain).collect();
            // Missed-update siblings join the choice's mask group, each
            // sharing at least one conflict atom with the target (else
            // masking it is pointless). Skipped entirely when the target's
            // own conflict set is empty. Highest version first.
            let mut similar: Vec<SimilarMask> = Vec::new();
            if !parents.is_empty() {
                // Portage yields only ebuilds, so an installed instance's own
                // ebuild is a sibling; the target's version is masked by its own
                // entry already.
                let party_versions: Vec<&str> = sc
                    .instances
                    .iter()
                    .filter(|i| !i.installed)
                    .map(|i| i.version.as_str())
                    .chain(std::iter::once(ver.as_str()))
                    .collect();
                if let Ok(cands) = gate.repo_candidates(&sc.category, &sc.package) {
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
                        let cand_str = c.match_str(&sc.category, &sc.package);
                        if excluded
                            .iter()
                            .any(|ex| matches_cli_option_atom(ex, &cand_str))
                        {
                            continue;
                        }
                        sibs.push(c);
                    }
                    sibs.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));
                    for c in sibs {
                        let sim_cpv = format!(
                            // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                            "{}/{}-{}:{}/{}",
                            sc.category, sc.package, c.version, sc.slot, c.sub_slot
                        );
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
            let installed = sc.instances.iter().any(|i| i.installed && i.version == ver);
            MaskChoice {
                target: (sc.category.clone(), sc.package.clone(), ver),
                parents,
                similar,
                installed,
            }
        })
        .collect();
    // Stable ascending length sort, so equal-length choices keep
    // version-desc order.
    choices.sort_by_key(|c| c.parents.len());
    choices
}

/// One pass's outcome in `_backtrack_depgraph` terms. Every variant carries
/// the overlay-merged working copy (`base` for the mask kinds, `params` for
/// `Config`): a pass that folded flips or dropped pins AND hit a
/// conflict/mask reports or retries with the union. `feedback` advances the
/// counters; mask kinds are non-terminal, `Config` preserves the current
/// terminal value. First hit wins.
/// Portage: _backtrack_depgraph.
pub(crate) enum BacktrackFeedback {
    Config {
        params: Box<BacktrackParams>,
    },
    /// The reverse-dependency feed (installed-consumer pins): these enforce
    /// in-pass even under `--backtrack=0`; the driver re-runs the pass with
    /// them instead of settling. Carries the same working copy as `Config`.
    /// Portage: _complete_graph (no `_allow_backtracking` gate).
    RevDep {
        params: Box<BacktrackParams>,
    },
    SlotConflict {
        base: Box<BacktrackParams>,
        choices: Vec<MaskChoice>,
    },
    /// The serialize dead-end retry: carries the grown working copy like
    /// `Config`/`RevDep` (budget-free re-pass, not a mask step). The driver
    /// explores it when backtracking and settles the pass as-is under
    /// `--backtrack=0`.
    /// Portage: _serialize_tasks' `not selected_nodes` branch.
    Circular {
        params: Box<BacktrackParams>,
    },
    MissingDep {
        base: Box<BacktrackParams>,
        owner: (String, String),
        entry: MaskEntry,
    },
}

/// Settle (report this pass with its merged working copy), feed one outcome
/// back into the search, or abandon this node. `DeadEnd` is a pass Portage
/// would not retry from (an unsatisfiable dependency no feedback covers);
/// the node is consumed but keeps the merged copy, the search continues
/// elsewhere and `get_best_run` picks the report.
/// Portage: _create_graph returning 0.
pub(crate) enum PassDecision {
    Settle { params: Box<BacktrackParams> },
    Feedback(BacktrackFeedback),
    DeadEnd { params: Box<BacktrackParams> },
}

/// The cross-pass accumulators, cloned per search node (hence `Clone`).
/// `slot_constraints` holds only the solvable-conflict atom sets; every
/// `!=cpv` negative lives in `runtime_pkg_mask` with its `MaskReason`;
/// readers see the union via `union_constraints`. `autounmask_use_config` ≈
/// `(cat/pkg, atom, consumer)` -- one consumer's parent rows of one masked atom.
pub(crate) type MaskParentKey = ((String, String), String, (String, String, String));

/// `needed_use_config_changes`. `params_equal` excludes the per-node
/// `mask_steps`/`depth`, like Portage's parameter-only `__eq__`.
/// Portage: resolver/backtracking.py, BacktrackParameter.
#[derive(Debug, Clone, Default)]
pub(crate) struct BacktrackParams {
    /// `mut` so the `_autounmask_breakage` fallback can switch all four off
    /// for one final clean pass once an accumulated autounmask change breaks
    /// another dependency. Portage: depgraph.py.
    pub(crate) autounmask_suggest_keywords: bool,
    pub(crate) autounmask_suggest_use: bool,
    pub(crate) autounmask_suggest_license: bool,
    pub(crate) autounmask_suggest_masks: bool,
    /// A dependency atom with no matching package makes the search mask that
    /// atom's *parent* and retry, so a `||` group re-picks an alternative
    /// whose subtree was unsatisfiable. The latch holds every `!=parent-cpv`
    /// already tried, guaranteeing termination.
    /// Portage: backtracking.py, _feedback_missing_dep.
    pub(crate) missing_dep_masked: HashSet<String>,
    /// Portage `_complete_graph`'s parent atoms, discovered by vdb reverse
    /// scan. Latches every `(cat/pkg, atom)` already folded into
    /// `slot_constraints` so a constraint is never re-added and the loop
    /// converges. The mask's parent rows are bookkept separately: several
    /// installed consumers record the same atom, and each one owns its rows.
    pub(crate) reverse_dep_masked: HashSet<((String, String), String)>,
    /// `(cat/pkg, atom, consumer)` whose `(parent, atom)` pairs are already in
    /// that atom's `MaskReason::SlotConflict`. The latch above stops the second
    /// consumer from reaching the mask at all; this set lets its rows append
    /// once, without re-appending them on every pass.
    pub(crate) reverse_dep_conflict_parents: HashSet<MaskParentKey>,
    /// Dropped reverse-dep pins, accumulated across passes, for the residual
    /// installed-instance conflict report once the graph settles. Deduped by
    /// `(cp, atom, consumer)`; stale entries simply yield no record.
    pub(crate) dropped_pins: Vec<RevDepPin>,
    /// Enforced holdable pins, accumulated across passes like `dropped_pins`,
    /// so `assemble_result` can report the upgrades they withhold through the
    /// same skip-conflict `WARNING` (no conflict ever forms on that path).
    pub(crate) reverse_dep_pins: Vec<RevDepPin>,
    /// The cross-pass constraint map (keyed by `cat/pkg`): the union of every
    /// atom text that targeted a conflicted package. Only this map and the
    /// other accumulators survive across attempts; `entries` and every other
    /// per-pass accumulator is rebuilt each iteration.
    /// Portage: resolver/backtracking.py (re-runs the whole walk from scratch).
    pub(crate) slot_constraints: HashMap<(String, String), Vec<String>>,
    /// Every `!=cpv` negative with its `MaskReason`, keyed by the masked
    /// version's `cat/pkg`.
    pub(crate) runtime_pkg_mask: HashMap<(String, String), Vec<MaskEntry>>,
    /// First-insertion order of the `runtime_pkg_mask` negatives above, as
    /// `((cat, pkg), neg)` pairs: the missed-update notice renders in that
    /// order. Same-state dedup ignores it (`params_equal` deliberately does
    /// not compare it, like Portage's order-insensitive parameter `__eq__`).
    /// Portage: _get_missed_updates iterates the mask dict in insertion order.
    pub(crate) mask_order: Vec<((String, String), String)>,
    /// Mask-producing retries (slot-conflict choices, missing-dep parents).
    /// Portage's `--backtrack=N` bounds this per node, not the pass count;
    /// advanced by `Backtracker::feedback`, never in the walk.
    pub(crate) mask_steps: u32,
    /// Every retry including config-growth passes (Portage's node depth);
    /// advanced by `Backtracker::feedback`, never in the walk.
    pub(crate) depth: u32,
    /// `--autounmask-use` changes fed back into the loop: a dependency atom
    /// `X[flag]` landing on an *already-resolved* slot whose package can't
    /// satisfy `[flag]` -- but a `package.use` flip could -- folds the flip
    /// in here and re-runs the whole walk. Survives across passes;
    /// converges when a pass adds nothing new.
    /// Portage: _dynamic_config._needed_use_config_changes.
    pub(crate) autounmask_use_config: HashMap<(String, String), HashMap<String, bool>>,
    /// The `The following USE changes are necessary to proceed:` block entries,
    /// recorded when each flip is folded in (a `continue 'backtrack` discards
    /// the pass, so they can't live on the per-pass list). Merged into
    /// `autounmask_use_changes` once the graph settles. Deduped by
    /// `(atom, token)`.
    pub(crate) autounmask_use_change_records: Vec<AutounmaskChange>,
    /// `req.config` with `autounmask_use` filled from the accumulator --
    /// rebuilt whenever the accumulator grows, `None` until then (so the
    /// common no-autounmask path never clones `Config`).
    pub(crate) backtrack_config: Option<portage_profile::Config>,
    /// Once an accumulated autounmask USE change makes some *other* use-dep
    /// unsatisfiable and cannot be reconciled, portage abandons autounmask
    /// entirely and re-resolves one final clean pass. This latch holds the
    /// contradiction; the driver acts on it once and sets
    /// `autounmask_disabled` so it can't re-fire.
    /// Portage: _autounmask_breakage.
    pub(crate) autounmask_use_broke: bool,
    pub(crate) autounmask_disabled: bool,
    /// Installed consumers whose built `cat/pkg:S/SS=` dep no longer matches how
    /// this run leaves the provider: the next pass seeds the walk with their
    /// `cat/pkg` atom and flips them to `Reinstall`, so each consumer is a
    /// **walked graph node** (tree `RDEPEND` re-evaluated, `:=` re-bound).
    /// Portage keys `(root, slot_atom)`; portuale resolves one instance per
    /// `cat/pkg`, so `(cat, pkg)` carries the same information here.
    /// Portage: _dynamic_config._slot_operator_replace_installed.
    pub(crate) slot_operator_replace_installed: BTreeSet<(String, String)>,
    /// The members of the set above that the built-parent update probe filed
    /// (Portage's `_slot_operator_update_backtrack` results). Only these become the `__auto_slot_operator_replace_installed__`
    /// force-reinstall argument that renders the red `r`; members the rebuild
    /// scan scheduled do not, and an undone scan never reaches the display.
    /// Not compared by `params_equal`.
    pub(crate) probe_replaced: BTreeSet<(String, String)>,
    /// The `--rebuild-if-unbuilt` / `--rebuild-if-new-rev` /
    /// `--rebuild-if-new-ver` twin of the set above: installed parents the
    /// post-walk scan (`rebuild_if_restart_parents`) found behind a dep this
    /// run merges. The next pass seeds the walk with their `cat/pkg` atom
    /// and flips them to a reasonless `Reinstall`, so each parent is a
    /// **walked graph node** (its own deps re-resolved). Same `(cat, pkg)`
    /// keying; monotone, so the search terminates.
    /// Portage: _rebuild_config.rebuild_list + `__auto_rebuild__` (`depgraph.py:5460-5480`).
    pub(crate) rebuild_if_replace_installed: BTreeSet<(String, String)>,
    /// The child half of Portage's `_slot_operator_update_backtrack`: an
    /// installed child whose update the probe found keeps its own record here
    /// -- `(category, package, version)` of the candidate the restart must
    /// select.
    pub(crate) slot_operator_forced_upgrade: BTreeSet<(String, String, String)>,
    /// The undo latch: a demoted cp must be remembered here or the scan would
    /// immediately re-add it (its vdb `:S/SS=` binding is still stale).
    /// Monotone and bounded by the installed cp count, so the search
    /// terminates. `BTreeSet` for deterministic order; included in
    /// `params_equal`.
    /// Portage: _eliminate_rebuilds (which re-adds the installed instance
    /// in place and never restarts, so it needs no such latch).
    pub(crate) slot_operator_undone: BTreeSet<(String, String)>,
    /// The prune latch: restart with `prune_rebuilds` when the replace set
    /// meets missed updates. Included in `params_equal` like Portage's
    /// parameter `__eq__` compares `prune_rebuilds`.
    /// Portage: _resolve + resolver/backtracking.py, _feedback_config.
    pub(crate) prune_rebuilds: bool,
    /// When the merge-order walk strands on a dependency cycle, records for every
    /// cycle node the in-cycle package it depends on, and re-runs the walk --
    /// `dep_zapdeps` then demotes any `||` alternative matching such a child,
    /// so the walk takes the branch that breaks the cycle. Keyed by `cat/pkg`
    /// (Portage keys exact package nodes). An already-recorded cycle re-use is
    /// unsolved and settles into the circular report; the map strictly grows per
    /// retry, so the search terminates. Included in `params_equal`.
    /// Portage: backtrack_infos["config"]["circular_dependency"].
    pub(crate) circular_dependency: HashMap<RootCp, Vec<CircularDepChild>>,
}

/// One in-cycle dependency of a `circular_dependency` parent: the child's
/// own `cat/pkg-version` plus the slot/sub-slot an atom's `:slot` pin would
/// match against. `pub` (fields private) so the CLI crate can carry the
/// settling pass's map into its own display derivations, which must demote
/// exactly like the walk did.
/// Portage: `circular_child` package object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CircularDepChild {
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) slot: Option<String>,
    pub(crate) sub_slot: Option<String>,
}

/// The `cat/pkg-version[:slot[/sub]]` candidate string for one child, shared
/// by the walk's demotion and the merge-order suppression's: the two must
/// agree on what matches, or the re-derivation keeps the branch the walk
/// dropped and a phantom edge re-closes the cycle.
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
    /// else default.
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
    /// negatives (applied as an order-independent conjunction). Built once
    /// per pass in `run_pass` -- `bp` is never mutated mid-walk, so the
    /// union is pass-constant.
    pub(crate) fn union_constraints(&self) -> HashMap<(String, String), Vec<String>> {
        let mut out = self.slot_constraints.clone();
        for (cp, masks) in &self.runtime_pkg_mask {
            out.entry(cp.clone())
                .or_default()
                .extend(masks.iter().map(|m| m.neg.clone()));
        }
        out
    }

    /// Just the `runtime_pkg_mask` negatives for one `cat/pkg` (no
    /// positives): tells mask-induced unsatisfiability (retry with the
    /// parent masked) from genuine absence and USE-only failure.
    /// Portage: the dep-level `runtime_pkg_mask` probe.
    pub(crate) fn masked_negatives_for(&self, cp: &(String, String)) -> Vec<String> {
        self.runtime_pkg_mask
            .get(cp)
            .map(|bucket| bucket.iter().map(|m| m.neg.clone()).collect())
            .unwrap_or_default()
    }
}
