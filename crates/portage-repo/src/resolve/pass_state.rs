//! The state and result of one resolver pass.

use crate::*;

/// The walk's per-pass `let mut`s, moved out of the loop body so the `'queue` walk
/// can live in `run_pass`. The walk writes the cross-pass autounmask accumulators
/// through the overlay trio and reads them back overlay-then-`bp` within the same
/// pass (see `overlay_use_want`); `collect_feedback` + driver folds them into `bp`.
#[derive(Default)]
pub(crate) struct PassState {
    /// Skip-conflict rows from the direct-solve hook at this pass's tail. The
    /// constraint-withhold rows join in `collect_feedback`; both ride out through
    /// `PassResult::skipped_updates`.
    pub(crate) skipped_updates: Vec<SkippedUpdate>,
    /// Guards against infinite requeuing (e.g. a dependency cycle): the exact same
    /// atom *text* is only ever resolved once. Deliberately coarser than the
    /// (category, package, slot) dedup below: two different atom texts can both need
    /// resolving even with a shared visited key.
    pub(crate) visited_atoms: HashSet<String>,
    /// `_minimize_children` verdicts for an owner's same-cp sibling atoms, keyed
    /// `(owner cp, atom text)`: filled when the first sibling pops, consumed as each
    /// one pops (see `run_pass`).
    pub(crate) minimize_pins: HashMap<((String, String), String), PretendOutcome>,
    /// (category, package, slot) -> index into `entries`, for New/Upgrade outcomes
    /// only. The first atom to resolve a slot wins; later atoms landing on the same
    /// slot are checked against that version (see `SlotConflict`) instead of
    /// re-resolving. Keyed by resolving root too, so one `cat/pkg:slot` needed in
    /// both roots is two nodes.
    // Portage: Package.__hash__ graph-node keys
    pub(crate) resolved_slots: HashMap<(String, String, String, bool), usize>,
    /// (category, package, slot) -> the queue depth the first-resolving atom carried
    /// when this slot's entry was pushed. An in-pass `--autounmask-use` flip on an
    /// already-resolved slot re-expands that package's newly-gated deps at this depth.
    // Portage: depgraph.py pkg.depth, set once in _add_pkg
    pub(crate) resolved_depths: HashMap<(String, String, String), u32>,
    /// (category, package, slot) -> the version of the `AlreadyInstalled` node this
    /// pass put in that slot (slot read from the vdb `InstalledRef`). A second map
    /// rather than a new `resolved_slots` arm, so installed-vs-merge slot collisions
    /// surface as ordinary collisions. Keyed by resolving root, like `resolved_slots`.
    // Portage: depgraph.py _add_pkg adds installed nomerge nodes to _package_tracker
    pub(crate) installed_slots: HashMap<(String, String, String, bool), String>,
    /// `(category, package, slot)` -> already added an `AlreadyInstalled` entry for
    /// that installed instance (`NoVisibleCandidate` uses `None`, keeping the old
    /// cp-scoped dedup). Two installed instances of one cp in different slots are two
    /// nodes with independently walked deps. Separate from `resolved_slots` (which
    /// keys merge-bound instances). Keyed by resolving root.
    // Portage: _package_tracker keyed by pkg.slot_atom
    pub(crate) other_outcomes: HashSet<(String, String, Option<String>, bool)>,
    /// Per-pass memo for [`arg_pinned_installed_reuse`]: `cat/pkg` -> the
    /// `(version, main slot)` pairs the top-level atoms keep installed. A missing key
    /// means uncomputed for this pass (an empty vec is a computed miss).
    pub(crate) top_pinned_installed: HashMap<(String, String), Vec<(String, String)>>,
    /// (category, package) -> already added a `targets_running_root` entry for it.
    /// Separate from `resolved_slots`/`other_outcomes` (those dedup ROOT-targeted
    /// resolutions): a package can need building into *both* ROOT and the running
    /// root at once, which must never collide into one shared key.
    pub(crate) root_deps_build_seen: HashSet<(String, String)>,
    /// Per-pass cache for `installed_dep_string`'s `Effective` layer, keyed
    /// `(category, package, version, key)`: the counterpart to Portage applying the
    /// overlay once per installed instance and keeping it for every pass.
    // Portage: FakeVartree._apply_dynamic_deps dynamic_deps_applied
    pub(crate) installed_meta_memo: HashMap<(String, String, String, String), String>,
    /// Backtracking: every atom text (bare or constrained) that targeted a given
    /// `cat/pkg` this pass. On a solvable slot conflict the whole set becomes that
    /// package's `slot_constraints` entry for the next attempt.
    pub(crate) slot_want: HashMap<(String, String), Vec<String>>,
    /// Backtracking: for each `cat/pkg` pulled in this pass, the `(cat, pkg, version)`
    /// of every puller plus the pulling atom text.
    pub(crate) slot_pullers: SlotPullers,
    /// Set when this pass folded a new `--autounmask-use` flip into
    /// `autounmask_use_config`. The driver re-runs the whole walk with the grown
    /// config, so the flipped package's `flag?`-gated deps appear.
    pub(crate) autounmask_grew: bool,
    /// Set when the pass rescued a failed dependency via a parent USE flip with
    /// `--autounmask-backtrack` off. Rides into `PassResult` (same name).
    pub(crate) parent_flip_rescued: bool,
    /// The `(category, package)` keys of dependencies whose `NoVisibleCandidate` the
    /// `'parent_flip` off-arm collected into a parent USE flip (record-and-fail), so
    /// no masked/use-unsat/plain-miss disclosure is recorded for them below.
    // Portage: depgraph.py, _apply_parent_use_changes; _unsatisfied_deps_for_display
    pub(crate) parent_flip_recorded: HashSet<(String, String)>,
    /// Set (once) when this pass hit a dependency `NoVisibleCandidate` whose
    /// non-top-level parent isn't already latched: the driver masks `!=parent-cpv`
    /// and re-runs.
    // Portage: depgraph.py, _feedback_missing_dep
    pub(crate) missing_dep_trigger: Option<((String, String), String, String)>,
    pub(crate) entries: Vec<GraphEntry>,
    /// REQUIRED_USE violations collected anywhere in the walk: Portage's `_add_pkg`
    /// sets `_required_use_unsatisfied` and returns 0 without aborting the walk, so
    /// the BFS keeps going and the call only fails at the very end, once every
    /// reachable candidate has resolved (or failed) on its own terms.
    // Portage: depgraph.py _add_pkg; _unsatisfied_deps_for_display
    pub(crate) required_use_violations: Vec<String>,
    pub(crate) slot_conflicts: Vec<SlotConflict>,
    /// Masked-dependency disclosures for this pass (see `MaskedDepReport`): rebuilt
    /// every attempt like `slot_conflicts`, so only the final pass's reports render.
    pub(crate) masked_deps: Vec<MaskedDepReport>,
    /// USE-unsatisfied disclosures for this pass (see `UseUnsatDepReport`): rebuilt
    /// every attempt like `masked_deps`.
    pub(crate) use_unsat_deps: Vec<UseUnsatDepReport>,
    /// Plain-miss disclosures for this pass (see `PlainMissDepReport`): rebuilt every
    /// attempt like `masked_deps`.
    pub(crate) plain_miss_deps: Vec<PlainMissDepReport>,
    /// The unevaluated dep atom behind each dependency `NoVisibleCandidate` entry
    /// (first requirer wins), for `abort_outcome`'s `UnsatisfiedAtom` reason.
    pub(crate) nvc_dep_atoms: HashMap<(String, String), String>,
    /// Top-level set-member atoms (`@world`/`@selected` expansion, see
    /// [`ResolveRequest::set_args`]) that selected no package on this pass. The walk
    /// continues; they ride into [`PassResult::missing_args`]. Deduped per pass.
    // Portage: depgraph.py, _missing_args
    pub(crate) missing_args: Vec<String>,
    /// `--changed-deps-report`: keyed by installed `Package` object in Portage, so a
    /// repeat visit to the same installed category/package/version collapses to one
    /// entry; mirrored here with an explicit dedup set preserving first-encountered
    /// order.
    // Portage: depgraph.py, _changed_deps_pkgs
    pub(crate) changed_deps_report_seen: HashSet<(String, String, String)>,
    pub(crate) changed_deps_report_entries: Vec<ChangedDepsReportEntry>,
    /// Each queued atom carries its own depth (0 for a top-level atom, one more than
    /// its parent otherwise; only consulted by `Deep::recurses_at`) and the `(category,
    /// package)` that pushed it if any (`None` for a top-level atom; only consulted
    /// for `GraphEntry::required_by`).
    pub(crate) queue: VecDeque<QueueItem>,
    /// Seeds not yet popped: while positive, each pop closes the previous
    /// seed's whole-subtree block, so Portage's LIFO `_dep_stack` order holds
    /// (the last argument's subtree walks first). Set from the seeded queue
    /// length once per pass.
    // Portage: depgraph.py, _create_graph
    pub(crate) seeds_left: usize,
    /// Queue length right after the previous seed popped: the next pop splits
    /// off everything past it into `held_blocks`.
    pub(crate) seed_split_at: Option<usize>,
    /// Whole-subtree blocks held back while an earlier seed's block drains.
    pub(crate) held_blocks: VecDeque<VecDeque<QueueItem>>,
    pub(crate) pending_blockers: Vec<PendingBlocker>,
    /// Top-level atoms matched by `package.provided`. See `GraphResult::pprovided_atoms`.
    pub(crate) pprovided_atoms: Vec<String>,
    /// Portage `--autounmask` changes applied during this walk. See the
    /// `GraphResult::autounmask_*_changes` fields.
    pub(crate) autounmask_keyword_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_use_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_license_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_mask_changes: Vec<AutounmaskChange>,
    /// (category, package) -> every distinct owner that reached it via a dependency
    /// string, accumulated separately from the BFS's own dedup decisions so a diamond
    /// dependency's *second* (deduped) owner is still recorded. Merged into `entries`
    /// in a single post-pass at the end.
    pub(crate) required_by_map: HashMap<(String, String), HashSet<(String, String)>>,
    /// See `EdgeKindMap`: an edge counts as unbreakable only when `has_hard &&
    /// !has_soft` (one run-time or optional route makes the whole edge breakable).
    // Portage: DepPriority per-edge
    pub(crate) edge_kind_map: EdgeKindMap,
    /// This pass's `--autounmask-use` flips (`(cat, pkg) -> {flag -> desired}`),
    /// overlaying `bp.autounmask_use_config` (see `overlay_use_want`). Buckets are
    /// never empty: entries are created only together with an insert.
    pub(crate) use_overlay: HashMap<(String, String), HashMap<String, bool>>,
    /// This pass's `The following USE changes are necessary to proceed:` records,
    /// overlaying `bp.autounmask_use_change_records` (deduped by `(atom, token)`
    /// across the union).
    pub(crate) use_change_overlay: Vec<AutounmaskChange>,
    /// This pass hit an autounmask contradiction (a flag wanted both on and off);
    /// `collect_feedback` + driver latches it into `bp.autounmask_use_broke`.
    pub(crate) use_broke: bool,
    /// This pass dropped a `NoVisibleCandidate` on the `other_outcomes` dedup while a
    /// mask was active: an unsatisfiable dependency `collect_feedback`'s `has_nvc`
    /// scan cannot see. Set only under a non-empty `runtime_pkg_mask`.
    pub(crate) suppressed_nvc: bool,
    /// Portage `_add_pkg`'s per-instance `Parent Dep:` rows, collected as each queue
    /// item resolves.
    // Portage: depgraph.py, _add_parent_atom
    pub(crate) parent_atoms: Vec<resolver_trace::ParentAtom>,
}
