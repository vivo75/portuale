//! The backtracking search tree.

use crate::*;

/// Phase C2 (023): one search node, mirroring real
/// `backtracking.py::_BacktrackNode` (`parameter` + `depth` +
/// `mask_steps` + `terminal`). `depth`/`mask_steps` live inside
/// `BacktrackParams` (C1) rather than alongside it; `params_equal`
/// excludes them (plus the derived `backtrack_config`), exactly like
/// real's parameter-only `__eq__`.
pub(crate) struct BacktrackNode {
    pub(crate) params: BacktrackParams,
    pub(crate) terminal: bool,
}

/// Phase C2 (023): the depth-first search over mask/config choices,
/// mirroring real `backtracking.py::Backtracker`. `nodes` holds every
/// node ever added; `unexplored` is the DFS stack of pending node
/// indices; `current` is the node whose pass just ran (feedback builds
/// from it). `max_depth` uses real's formula: `max(1, (retries + 1) // 2)`
/// from `--backtrack`. A total restart cap is also enforced, matching
/// real's `backtracked >= max_retries` check before feeding back.
pub(crate) struct Backtracker {
    pub(crate) nodes: Vec<BacktrackNode>,
    pub(crate) unexplored: Vec<usize>,
    pub(crate) current: Option<usize>,
    pub(crate) max_depth: u32,
}

/// Two `BacktrackParams` describe the same search state when every
/// accumulator agrees. Excluded, like real's parameter `__eq__` excludes
/// everything but the parameter content: `backtrack_config` (rebuilt
/// deterministically from `autounmask_use_config` whenever it grows),
/// `depth`/`mask_steps` (per-node search position, not state).
pub(crate) fn params_equal(a: &BacktrackParams, b: &BacktrackParams) -> bool {
    a.autounmask_suggest_keywords == b.autounmask_suggest_keywords
        && a.autounmask_suggest_use == b.autounmask_suggest_use
        && a.autounmask_suggest_license == b.autounmask_suggest_license
        && a.autounmask_suggest_masks == b.autounmask_suggest_masks
        && a.missing_dep_masked == b.missing_dep_masked
        && a.reverse_dep_masked == b.reverse_dep_masked
        && a.dropped_pins == b.dropped_pins
        && a.reverse_dep_pins == b.reverse_dep_pins
        && a.slot_constraints == b.slot_constraints
        && a.runtime_pkg_mask == b.runtime_pkg_mask
        && a.circular_dependency == b.circular_dependency
        && a.autounmask_use_config == b.autounmask_use_config
        && a.autounmask_use_change_records == b.autounmask_use_change_records
        && a.autounmask_use_broke == b.autounmask_use_broke
        && a.autounmask_disabled == b.autounmask_disabled
        && a.slot_operator_replace_installed == b.slot_operator_replace_installed
        && a.slot_operator_forced_upgrade == b.slot_operator_forced_upgrade
        && a.slot_operator_undone == b.slot_operator_undone
        && a.prune_rebuilds == b.prune_rebuilds
        && a.circular_dependency == b.circular_dependency
}

/// Real `Backtracker._check_runtime_pkg_mask` (bug 375573): a node is
/// discarded when a slot-conflict mask's every parent is itself masked
/// (the mask's reason got masked away -- exploring it is pointless).
/// Missing-dependency masks never disqualify. A mask with no parents is
/// always valid (real's 692746 arm: masking an all-matched package to
/// avoid a missed update).
/// Granularity note (carried into C3): real tests versioned-package
/// presence (`ppkg not in runtime_pkg_mask`); here a parent counts as
/// masked only on an exact `!=cpv` string hit, so masking a *different*
/// version of a cited parent explores where real discards (safe
/// direction: extra search, same reports). C3's real-shaped parent sets
/// are the place to tighten this.
pub(crate) fn check_runtime_pkg_mask(masks: &HashMap<(String, String), Vec<MaskEntry>>) -> bool {
    for entries in masks.values() {
        for entry in entries {
            let parents = match &entry.reason {
                MaskReason::SlotConflict { parents } => parents,
                MaskReason::MissingDependency { .. } => continue,
                // Real `Backtracker._check_runtime_pkg_mask`
                // (`backtracking.py:150-154`) skips
                // `slot_operator_mask_built` (and missing-dependency)
                // masks the same way: a built-package steer never
                // invalidates the node that carries it.
                MaskReason::SlotOperatorBuilt => continue,
            };
            if parents.is_empty() {
                continue;
            }
            let any_unmasked = parents.iter().any(|((pc, pp, pv), _atom)| {
                !masks.get(&(pc.clone(), pp.clone())).is_some_and(|bucket| {
                    bucket.iter().any(|m| m.neg == format!("!={pc}/{pp}-{pv}"))
                })
            });
            if !any_unmasked {
                return false;
            }
        }
    }
    true
}

impl Backtracker {
    pub(crate) fn new(max_depth: u32, initial: BacktrackParams) -> Self {
        let mut bt = Backtracker {
            nodes: Vec::new(),
            unexplored: Vec::new(),
            current: None,
            max_depth,
        };
        // Real `_add(self._root)`: the unmasked starting state, always
        // terminal (deepest-terminal wins in `get_best_run`).
        bt.add_node(initial, true, true);
        bt
    }

    /// Real `Backtracker._add`: keep the node unless a 375573 discard
    /// applies, it exceeds the mask budget, or an equal state exists.
    /// Returns whether the node will be explored.
    pub(crate) fn add_node(
        &mut self,
        params: BacktrackParams,
        terminal: bool,
        explore: bool,
    ) -> bool {
        if !check_runtime_pkg_mask(&params.runtime_pkg_mask) {
            return false;
        }
        if params.mask_steps > self.max_depth {
            return false;
        }
        if self.nodes.iter().any(|n| params_equal(&n.params, &params)) {
            return false;
        }
        self.nodes.push(BacktrackNode { params, terminal });
        if explore {
            self.unexplored.push(self.nodes.len() - 1);
        }
        true
    }

    /// C2: a dead end keeps its pass's merged accumulators in its node
    /// (see `PassDecision::DeadEnd`), so a later best-run re-pass
    /// reports what the abandoned pass saw instead of silently dropping
    /// its USE-change records and dropped pins.
    pub(crate) fn adopt_current(&mut self, params: BacktrackParams) {
        if let Some(idx) = self.current {
            self.nodes[idx].params = params;
        }
    }

    /// Real `Backtracker.get`: pop the DFS stack; `None` means the search
    /// is exhausted and the driver falls back to `get_best_run`.
    pub(crate) fn get(&mut self) -> Option<BacktrackParams> {
        let idx = self.unexplored.pop()?;
        self.current = Some(idx);
        Some(self.nodes[idx].params.clone())
    }

    /// Real `Backtracker.get_best_run`: the deepest terminal node's
    /// params -- the most config progress with no masks -- so users never
    /// see a "masked by backtracking" graph unless nothing better exists.
    /// The root is terminal, so this always yields at least the initial
    /// state.
    pub(crate) fn get_best_run(&self) -> BacktrackParams {
        let mut best = 0;
        for (i, node) in self.nodes.iter().enumerate() {
            if node.terminal && node.params.depth > self.nodes[best].params.depth {
                best = i;
            }
        }
        self.nodes[best].params.clone()
    }

    /// Real `Backtracker.feedback`: turn one pass's outcome into new
    /// nodes from the current one. Config growth keeps the step budget
    /// and the current terminal value; mask steps cost one budget unit
    /// and are never terminal.
    pub(crate) fn feedback(&mut self, kind: BacktrackFeedback) {
        let current_idx = self
            .current
            .expect("feedback() only after get() returned params");
        let current_terminal = self.nodes[current_idx].terminal;
        let (mut params, terminal, mask_cost) = match kind {
            BacktrackFeedback::Config { params }
            | BacktrackFeedback::RevDep { params }
            | BacktrackFeedback::Circular { params } => (*params, current_terminal, 0),
            BacktrackFeedback::SlotConflict { base, choices } => {
                // #57 S1: keep the pass's merged accumulators on the node
                // itself, exactly as `PassDecision::DeadEnd` does -- the
                // mask children below are non-terminal, so `get_best_run`
                // comes back to *this* node, and without this its
                // dropped reverse-dep pins and autounmask records would
                // be thrown away and the re-pass would report less than
                // the pass that found the conflict did.
                self.nodes[current_idx].params = (*base).clone();
                // C3: one node per ranked choice (real's
                // `_feedback_slot_conflict` loop), masking the target
                // plus its C4 similar group. A repeated mask replaces
                // its reason (real assigns
                // `runtime_pkg_mask[pkg]["slot conflict"]`, it never
                // accumulates).
                for choice in choices {
                    let mut params = (*base).clone();
                    let members = choice
                        .similar
                        .iter()
                        .map(|s| (&s.target, &s.parents))
                        .chain(std::iter::once((&choice.target, &choice.parents)))
                        .collect::<Vec<_>>();
                    for ((cat, pkg, ver), parents) in members {
                        let neg = format!("!={cat}/{pkg}-{ver}");
                        let bucket = params
                            .runtime_pkg_mask
                            .entry((cat.clone(), pkg.clone()))
                            .or_default();
                        match bucket.iter_mut().find(|m| m.neg == neg) {
                            Some(existing) => {
                                existing.reason = MaskReason::SlotConflict {
                                    parents: parents.clone(),
                                };
                            }
                            None => {
                                bucket.push(MaskEntry {
                                    neg: neg.clone(),
                                    reason: MaskReason::SlotConflict {
                                        parents: parents.clone(),
                                    },
                                });
                                // #129 (S1): first-seen order for the
                                // missed-update notice (see `mask_order`).
                                params.mask_order.push(((cat.clone(), pkg.clone()), neg));
                            }
                        }
                    }
                    params.mask_steps += 1;
                    params.depth += 1;
                    self.add_node(params, false, true);
                }
                return;
            }
            BacktrackFeedback::MissingDep { base, owner, entry } => {
                let mut params = *base;
                // The re-mask latch travels with the mask (real's
                // `dep.parent not in _runtime_pkg_mask` is the mask
                // itself; portuale's string latch additionally pins the
                // exact cpv so the same parent is never re-masked).
                params.missing_dep_masked.insert(entry.neg.clone());
                let order_key = (owner.clone(), entry.neg.clone());
                let bucket = params.runtime_pkg_mask.entry(owner).or_default();
                if !bucket.iter().any(|m| m.neg == entry.neg) {
                    bucket.push(entry);
                    // #129 (S1): first-seen order for the missed-update
                    // notice (see `mask_order`).
                    params.mask_order.push(order_key);
                }
                (params, false, 1)
            }
        };
        params.mask_steps += mask_cost;
        params.depth += 1;
        self.add_node(params, terminal, true);
    }
}
