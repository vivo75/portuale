//! The backtracking search tree.

use crate::*;

/// One search node: `parameter` + `depth` + `mask_steps` + `terminal`.
/// `depth`/`mask_steps` live inside `BacktrackParams`; `params_equal`
/// excludes them (plus the derived `backtrack_config`), like Portage's
/// parameter-only `__eq__`.
/// Portage: backtracking.py, _BacktrackNode.
pub(crate) struct BacktrackNode {
    pub(crate) params: BacktrackParams,
    pub(crate) terminal: bool,
}

/// The depth-first search over mask/config choices. `nodes` holds every node
/// ever added; `unexplored` is the DFS stack; `current` is the node whose
/// pass just ran. `max_depth` uses Portage's formula (`max(1, (retries + 1)
/// // 2)` from `--backtrack`); a total restart cap matches Portage's
/// `backtracked >= max_retries` check.
/// Portage: backtracking.py, Backtracker.
pub(crate) struct Backtracker {
    pub(crate) nodes: Vec<BacktrackNode>,
    pub(crate) unexplored: Vec<usize>,
    pub(crate) current: Option<usize>,
    pub(crate) max_depth: u32,
}

/// Two `BacktrackParams` describe the same search state when every
/// accumulator agrees. Excluded, like Portage's parameter `__eq__`: the
/// derived `backtrack_config` and the per-node `depth`/`mask_steps`.
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

/// A node is discarded when a slot-conflict mask's every parent is itself
/// masked (the mask's reason got masked away -- exploring it is pointless).
/// Missing-dependency masks never disqualify; a mask with no parents is
/// always valid (masking an all-matched package avoids a missed update).
/// Portage: Backtracker._check_runtime_pkg_mask (bug 375573).
///
/// Granularity note: Portage tests versioned-package presence; here a parent
/// counts as masked only on an exact `!=cpv` string hit, so masking a
/// *different* version of a cited parent explores where Portage discards
/// (safe direction: extra search, same reports).
pub(crate) fn check_runtime_pkg_mask(masks: &HashMap<(String, String), Vec<MaskEntry>>) -> bool {
    for entries in masks.values() {
        for entry in entries {
            let parents = match &entry.reason {
                MaskReason::SlotConflict { parents } => parents,
                MaskReason::MissingDependency { .. } => continue,
                // Built-package steers never invalidate the node that carries
                // them (like missing-dependency masks).
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
        // The unmasked starting state, always terminal (deepest-terminal
        // wins in `get_best_run`).
        bt.add_node(initial, true, true);
        bt
    }

    /// Keep the node unless a discard applies, it exceeds the mask budget,
    /// or an equal state exists. Returns whether the node will be explored.
    /// Portage: Backtracker._add.
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

    /// A dead end keeps its pass's merged accumulators in its node (see
    /// `PassDecision::DeadEnd`), so a later best-run re-pass reports what
    /// the abandoned pass saw.
    pub(crate) fn adopt_current(&mut self, params: BacktrackParams) {
        if let Some(idx) = self.current {
            self.nodes[idx].params = params;
        }
    }

    /// Pop the DFS stack; `None` means the search is exhausted and the
    /// driver falls back to `get_best_run`. Portage: Backtracker.get.
    pub(crate) fn get(&mut self) -> Option<BacktrackParams> {
        let idx = self.unexplored.pop()?;
        self.current = Some(idx);
        Some(self.nodes[idx].params.clone())
    }

    /// The deepest terminal node's params -- the most config progress with no
    /// masks -- so users never see a "masked by backtracking" graph unless
    /// nothing better exists. The root is terminal, so this always yields at
    /// least the initial state. Portage: Backtracker.get_best_run.
    pub(crate) fn get_best_run(&self) -> BacktrackParams {
        let mut best = 0;
        for (i, node) in self.nodes.iter().enumerate() {
            if node.terminal && node.params.depth > self.nodes[best].params.depth {
                best = i;
            }
        }
        self.nodes[best].params.clone()
    }

    /// Turn one pass's outcome into new nodes from the current one. Config
    /// growth keeps the step budget and the current terminal value; mask
    /// steps cost one budget unit and are never terminal.
    /// Portage: Backtracker.feedback.
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
                // Keep the pass's merged accumulators on the node itself: the
                // mask children below are non-terminal, so `get_best_run`
                // comes back to *this* node.
                self.nodes[current_idx].params = (*base).clone();
                // One node per ranked choice, masking the target plus its
                // similar group. A repeated mask replaces its reason (never
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
                                // First-seen order for the missed-update notice.
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
                // The re-mask latch travels with the mask (additionally pins the
                // exact cpv so the same parent is never re-masked).
                params.missing_dep_masked.insert(entry.neg.clone());
                let order_key = (owner.clone(), entry.neg.clone());
                let bucket = params.runtime_pkg_mask.entry(owner).or_default();
                if !bucket.iter().any(|m| m.neg == entry.neg) {
                    bucket.push(entry);
                    // First-seen order for the missed-update notice.
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
