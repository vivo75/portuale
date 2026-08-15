//! Parameters a backtracking pass runs with.

use crate::*;

/// The cross-pass accumulators, cloned per search node (hence `Clone`).
/// `slot_constraints` holds only the solvable-conflict atom sets; every
/// `!=cpv` negative lives in `runtime_pkg_mask` with its `MaskReason`;
/// readers see the union via `union_constraints`. `autounmask_use_config` ≈
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
    /// converges.
    pub(crate) reverse_dep_masked: HashSet<((String, String), String)>,
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
