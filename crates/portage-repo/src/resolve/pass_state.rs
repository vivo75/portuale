//! The state and result of one resolver pass.

use crate::*;

/// Phase A2 (023): every per-pass value the decision chain or the
/// tail assembly reads. Built once per pass from the walk locals;
/// `Feedback` drops it, `Settle` moves it into `assemble_result`.
/// (C2: plus the overlay trio, folded into the node params by the
/// driver before deciding.)
pub(crate) struct PassResult {
    pub(crate) entries: Vec<GraphEntry>,
    pub(crate) slot_conflicts: Vec<SlotConflict>,
    /// Backlog #90 (S2): withheld upgrades for the skip-conflict
    /// `WARNING` (rc 0): removal rows from the direct solve (run_pass
    /// tail) plus constraint-withhold rows (collect_feedback's
    /// reverse-pin site). Merged into `GraphResult::skipped_updates`
    /// by `assemble_result`.
    pub(crate) skipped_updates: Vec<SkippedUpdate>,
    /// Backlog #80: unresolved blocker rows with no owner entry,
    /// filed by `file_blocker_conflicts` (this pass's scan).
    pub(crate) orphan_blockers: Vec<OrphanBlocker>,
    pub(crate) slot_want: HashMap<(String, String), Vec<String>>,
    pub(crate) slot_pullers: SlotPullers,
    pub(crate) masked_deps: Vec<MaskedDepReport>,
    pub(crate) use_unsat_deps: Vec<UseUnsatDepReport>,
    pub(crate) plain_miss_deps: Vec<PlainMissDepReport>,
    pub(crate) nvc_dep_atoms: HashMap<(String, String), String>,
    pub(crate) missing_dep_trigger: Option<((String, String), String, String)>,
    /// Backlog #286: the pass's dropped set-member atoms (see
    /// `PassState::missing_args`).
    pub(crate) missing_args: Vec<String>,
    pub(crate) autounmask_grew: bool,
    /// Backlog #217: set when the pass collected a failed dependency's
    /// parent USE flip with `--autounmask-backtrack` off (the
    /// `'parent_flip` off-arm records the flip and fails instead of the
    /// overlay). Real's counterpart collects the flip post-failure
    /// (`_apply_parent_use_changes`, `depgraph.py:5820`), so the
    /// `_success_without_autounmask` tail is pre-empted and the
    /// "terminated early" notice prints. Rides
    /// into [`GraphResult::autounmask_no_clean_tail`].
    pub(crate) parent_flip_rescued: bool,
    pub(crate) edge_kind_map: EdgeKindMap,
    pub(crate) changed_deps_report_entries: Vec<ChangedDepsReportEntry>,
    pub(crate) pprovided_atoms: Vec<String>,
    pub(crate) autounmask_keyword_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_use_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_license_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_mask_changes: Vec<AutounmaskChange>,
    /// This pass's overlay accumulators, folded into `bp` by `next_step`
    /// before the decision chain (see `PassState`).
    pub(crate) use_overlay: HashMap<(String, String), HashMap<String, bool>>,
    pub(crate) use_change_overlay: Vec<AutounmaskChange>,
    pub(crate) use_broke: bool,
    /// #24 S3: the `(provider_cpv, consumer_cpv)` pairs this pass's
    /// slot-operator scan found (real `_forced_rebuilds`), stashed by
    /// `collect_feedback` so `assemble_result` renders "The following
    /// packages are causing rebuilds:" without a second vdb scan.
    /// `None` = the scan did not run for this pass (the `get_best_run`
    /// re-pass, which never goes through `collect_feedback`).
    pub(crate) abi_rebuilds: Option<Vec<(String, String)>>,
    /// See `PassState::suppressed_nvc` (#57).
    pub(crate) suppressed_nvc: bool,
    /// #59 S1: the walk's per-instance `Parent Dep:` rows (real
    /// `_add_parent_atom`), rendered by `resolver_trace`.
    pub(crate) parent_atoms: Vec<resolver_trace::ParentAtom>,
}

/// Phase A3 (023): the walk's-Everything per-pass `let mut`s, moved out of
/// the loop body so the `'queue` walk can live in `run_pass`. Field docs
/// are the original declaration-site comments, moved verbatim. Plus the
/// three overlay accumulators (`use_overlay`, `use_change_overlay`,
/// `use_broke`): the walk writes the cross-pass autounmask accumulators
/// through these and reads them back overlay-then-`bp` within the same
/// pass (see `overlay_use_want`); `collect_feedback` + driver folds them into `bp` before
/// the decision chain, which is exactly where today's code already has
/// them (today the walk writes `bp` directly).
#[derive(Default)]
pub(crate) struct PassState {
    /// Backlog #90 (S2): skip-conflict rows produced by the direct-solve
    /// hook at this pass's tail (removal verdicts). The
    /// constraint-withhold rows join in `collect_feedback`; both ride
    /// out through `PassResult::skipped_updates`.
    pub(crate) skipped_updates: Vec<SkippedUpdate>,
    /// Guards against infinite requeuing (e.g. a dependency cycle): the
    /// exact same atom *text* is only ever resolved once. This is
    /// deliberately coarser than the (category, package, slot) dedup
    /// below -- it exists purely for termination, not to decide whether a
    /// given slot has already been fully resolved (two different atom
    /// texts, e.g. a bare "dev-libs/foo" and a slotted "dev-libs/foo:1",
    /// can both need to be resolved even though they'd share a visited-atom
    /// check keyed any coarser than exact text).
    pub(crate) visited_atoms: HashSet<String>,
    /// #236 B: `_minimize_children` verdicts for an owner's same-cp
    /// sibling atoms, keyed `(owner cp, atom text)` -- filled when the
    /// first sibling pops, consumed as each one pops (see `run_pass`).
    pub(crate) minimize_pins: HashMap<((String, String), String), PretendOutcome>,
    /// (category, package, slot) -> index into `entries`, for New/Upgrade
    /// outcomes only. The first atom to resolve a given slot "wins" (its
    /// version is what gets recursed into); every later atom landing on
    /// the same slot is checked against that already-resolved version
    /// (see `SlotConflict`) instead of triggering a second, independent
    /// resolution.
    /// Track X Slice B (#242): keyed by resolving root too -- real
    /// keys graph nodes by package object (`Package.__hash__`), so one
    /// `cat/pkg:slot` needed in both roots is two nodes, and a
    /// running-root atom never reuses a target-root slot (nor vice
    /// versa). Uniformly `false` on same-root runs: identical keys.
    pub(crate) resolved_slots: HashMap<(String, String, String, bool), usize>,
    /// Backlog #218: (category, package, slot) -> the queue depth the
    /// first-resolving atom carried when this slot's entry was pushed.
    /// An in-pass `--autounmask-use` flip on an already-resolved slot
    /// re-expands that package's newly-gated deps at this depth (real
    /// `pkg.depth`, set once in `_add_pkg`, `depgraph.py:3763`), so the
    /// delta atoms queue exactly where a fresh walk would put them.
    pub(crate) resolved_depths: HashMap<(String, String, String), u32>,
    /// #57: (category, package, slot) -> the version of the
    /// `AlreadyInstalled` node this pass put in that slot (slot read from
    /// the vdb `InstalledRef`, since an `AlreadyInstalled` `GraphEntry`
    /// carries none). Deliberately a second map rather than a new
    /// `resolved_slots` arm, so the `unreachable!` at the merge-outcome
    /// slot check below stays truthful.
    ///
    /// Real `_add_pkg` adds installed nomerge nodes to `_package_tracker`
    /// exactly like merge-bound ones (`depgraph.py:3736`), so a slot
    /// holding one of each is an ordinary slot collision
    /// (`PackageTracker.conflicts()`); portuale's walker used to see
    /// installed nodes only through the early `resolved_version.is_none()`
    /// branch, which never consulted `resolved_slots` at all -- the whole
    /// of backlog #57.
    /// Track X Slice B (#242): like `resolved_slots`, keyed by
    /// resolving root (an installed instance in each root is two nodes).
    pub(crate) installed_slots: HashMap<(String, String, String, bool), String>,
    /// `(category, package, slot)` -> already added an
    /// `AlreadyInstalled` entry for that installed instance; a
    /// `NoVisibleCandidate` has no instance, so it uses `None` there and
    /// keeps the old cp-scoped dedup. Real's installed nodes are keyed by
    /// `pkg.slot_atom` (`_package_tracker`), so two installed instances
    /// of one cp in different slots are two nodes with deps walked
    /// independently -- `llvm-core/clang:22` and `:23` coexisting is
    /// exactly the #74 S2a shape. Separate from `resolved_slots` (which
    /// keys merge-bound instances).
    /// Track X Slice B (#242): like `resolved_slots`, keyed by
    /// resolving root.
    pub(crate) other_outcomes: HashSet<(String, String, Option<String>, bool)>,
    /// Backlog #285: per-pass memo for [`arg_pinned_installed_reuse`] --
    /// `cat/pkg` -> the `(version, main slot)` pairs the top-level atoms
    /// on that cp keep installed. A missing key means uncomputed for
    /// this pass (an empty vec is a computed miss).
    pub(crate) top_pinned_installed: HashMap<(String, String), Vec<(String, String)>>,
    /// (category, package) -> already added a `targets_running_root`
    /// entry for it (see `resolve_root_deps_build_entries`'s own doc
    /// comment). Deliberately separate from `resolved_slots`/
    /// `other_outcomes` above -- those two dedup ROOT-targeted
    /// resolutions, and a package genuinely can need building into *both*
    /// ROOT (as an ordinary RDEPEND) and the running root (as some other
    /// package's own BDEPEND) at once, which must never collide into one
    /// shared dedup key.
    pub(crate) root_deps_build_seen: HashSet<(String, String)>,
    /// A1 (#26): per-pass cache for `installed_dep_string`'s
    /// `Effective` layer, keyed `(category, package, version, key)`.
    /// Real `FakeVartree._apply_dynamic_deps` applies the overlay once
    /// per installed instance (`dynamic_deps_applied`) and keeps it for
    /// every backtracking pass; a per-pass cache is portuale's
    /// counterpart to that idempotence (the walk itself re-runs per
    /// pass, so the cache cannot outlive one).
    pub(crate) installed_meta_memo: HashMap<(String, String, String, String), String>,
    /// Backtracking: every atom text (bare or constrained) that targeted a
    /// given `cat/pkg` this pass. On a solvable slot conflict the whole set
    /// for the conflicted package becomes its `slot_constraints` entry for
    /// the next attempt.
    pub(crate) slot_want: HashMap<(String, String), Vec<String>>,
    /// Backtracking: for each `cat/pkg` targeted by some package's
    /// dependency string this pass, the `(cat, pkg, version)` of every
    /// package that pulled it in, plus the pulling atom text. C3's
    /// `slot_conflict_mask_choices` reads the triples to build each
    /// choice's `conflict_atoms` (real's parent side); parent
    /// downgrades now arrive via missing-dep feedback on a later pass,
    /// not pre-masking (see the step-3 site).
    pub(crate) slot_pullers: SlotPullers,
    /// Set when this pass folded a new `--autounmask-use` flip into
    /// `autounmask_use_config` (the already-resolved-slot re-check).
    /// The driver at the bottom re-runs the whole walk with the grown
    /// config, so the flipped package's `flag?`-gated deps appear.
    pub(crate) autounmask_grew: bool,
    /// Backlog #217: set when the pass rescued a failed dependency via
    /// a parent USE flip with `--autounmask-backtrack` off (the
    /// `'parent_flip` off-arm). Rides into `PassResult` (same name).
    pub(crate) parent_flip_rescued: bool,
    /// Backlog #195 (round 2): the `(category, package)` keys of
    /// dependencies whose `NoVisibleCandidate` the `'parent_flip`
    /// off-arm collected into a parent USE flip (record-and-fail, real
    /// `_apply_parent_use_changes`, `depgraph.py:5820`). Real drops each
    /// collected item from `_unsatisfied_deps_for_display`
    /// (`remaining_items`), so no masked/use-unsat/plain-miss disclosure
    /// is recorded for them below.
    pub(crate) parent_flip_recorded: HashSet<(String, String)>,
    /// Set (once) when this pass hit a dependency `NoVisibleCandidate`
    /// whose non-top-level parent isn't already latched -- the driver
    /// at the bottom masks `!=parent-cpv` and re-runs (real
    /// `_feedback_missing_dep`).
    pub(crate) missing_dep_trigger: Option<((String, String), String, String)>,
    pub(crate) entries: Vec<GraphEntry>,
    /// REQUIRED_USE (see the check further below, in the main BFS loop):
    /// real depgraph.py's own `_add_pkg` sets
    /// `_dynamic_config._required_use_unsatisfied = True` and returns 0
    /// on a violation -- it does NOT abort the whole graph walk, unlike a
    /// top-level atom's own `NoVisibleCandidate`. Every violation
    /// encountered anywhere in the walk is collected here and the BFS
    /// keeps going; the whole call only fails at the very end, once every
    /// reachable candidate has had a chance to resolve (or fail) on its
    /// own terms -- matching real portage's own `_unsatisfied_deps_for_
    /// display` list (checked once, at the very end of the real resolve)
    /// rather than portuale's own previous "abort on the first hit"
    /// shortcut.
    pub(crate) required_use_violations: Vec<String>,
    pub(crate) slot_conflicts: Vec<SlotConflict>,
    /// Masked-dependency disclosures for this pass (see
    /// `MaskedDepReport`): rebuilt every attempt like `slot_conflicts`,
    /// so only the final pass's reports are rendered.
    pub(crate) masked_deps: Vec<MaskedDepReport>,
    /// USE-unsatisfied dependency disclosures for this pass (see
    /// `UseUnsatDepReport`): rebuilt every attempt like `masked_deps`, so
    /// only the final pass's reports are rendered.
    pub(crate) use_unsat_deps: Vec<UseUnsatDepReport>,
    /// Plain-miss dependency disclosures for this pass (see
    /// `PlainMissDepReport`): rebuilt every attempt like `masked_deps`.
    pub(crate) plain_miss_deps: Vec<PlainMissDepReport>,
    /// The unevaluated dep atom behind each dependency
    /// `NoVisibleCandidate` entry (first requirer wins, like the entry
    /// itself), for `abort_outcome`'s `UnsatisfiedAtom` reason.
    pub(crate) nvc_dep_atoms: HashMap<(String, String), String>,
    /// Backlog #286: top-level set-member atoms (`@world`/`@selected`
    /// expansion, see [`ResolveRequest::set_args`]) that selected no
    /// package on this pass. Real drops them into `_missing_args`
    /// (`depgraph.py:5577-5605`) and continues the walk; they ride into
    /// [`PassResult::missing_args`] below. Deduped per pass (a repeat
    /// visit of the same atom does not re-record).
    pub(crate) missing_args: Vec<String>,
    /// `--changed-deps-report`: real `_changed_deps_pkgs` is a dict keyed
    /// by the installed `Package` object, so a repeat visit to the same
    /// installed category/package/version (e.g. via both a bare
    /// "dev-libs/foo" and an explicit "dev-libs/foo:0" atom text, or a
    /// diamond dependency) naturally collapses to one entry -- mirrored
    /// here with an explicit dedup set, keyed the same way, preserving
    /// first-encountered order (real dict iteration order) rather than
    /// sorting.
    pub(crate) changed_deps_report_seen: HashSet<(String, String, String)>,
    pub(crate) changed_deps_report_entries: Vec<ChangedDepsReportEntry>,
    /// Each queued atom carries its own depth (0 for a directly-requested
    /// top-level atom, parent's depth + 1 for anything reached only via a
    /// dependency string) -- only consulted by `deep.recurses_at` below,
    /// for deciding whether an AlreadyInstalled package's own further
    /// dependencies get walked; every other outcome ignores it entirely
    /// (see `Deep`'s own doc comment) -- and the `(category, package)`
    /// that pushed it, if any (`None` for a directly-requested top-level
    /// atom), only consulted by `required_by_map` below, for `GraphEntry`'s
    /// own `required_by` field.
    pub(crate) queue: VecDeque<QueueItem>,
    pub(crate) pending_blockers: Vec<PendingBlocker>,
    /// Top-level atoms matched by `package.provided` -- see
    /// `GraphResult::pprovided_atoms`.
    pub(crate) pprovided_atoms: Vec<String>,
    /// Real `--autounmask` changes applied during this walk -- see
    /// `GraphResult::autounmask_keyword_changes` / `autounmask_use_changes`.
    pub(crate) autounmask_keyword_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_use_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_license_changes: Vec<AutounmaskChange>,
    pub(crate) autounmask_mask_changes: Vec<AutounmaskChange>,
    /// (category, package) -> every distinct owner that reached it via a
    /// dependency string, accumulated separately from the BFS's own
    /// dedup/recursion decisions below (`visited_atoms`/`resolved_slots`/
    /// `other_outcomes`) so a diamond dependency's *second* (deduped)
    /// owner still gets recorded even though it never triggers a new
    /// resolution -- merged into `entries` in a single post-pass at the
    /// end, mirroring `pending_blockers`/`resolve_blockers`'s own
    /// "accumulate now, merge once the whole graph is known" shape.
    pub(crate) required_by_map: HashMap<(String, String), HashSet<(String, String)>>,
    /// See `EdgeKindMap`'s own doc comment. An edge counts as
    /// unbreakable only when `has_hard && !has_soft` -- one run-time or
    /// optional route makes the whole edge breakable, matching real
    /// portage's per-edge `DepPriority`.
    pub(crate) edge_kind_map: EdgeKindMap,
    /// This pass's `--autounmask-use` flips (`(cat, pkg) -> {flag ->
    /// desired}`), overlaying `bp.autounmask_use_config` (see
    /// `overlay_use_want`). Buckets are never empty: entries are created
    /// only together with an insert, exactly like the old direct writes.
    pub(crate) use_overlay: HashMap<(String, String), HashMap<String, bool>>,
    /// This pass's `The following USE changes are necessary to proceed:`
    /// records, overlaying `bp.autounmask_use_change_records` (deduped by
    /// `(atom, token)` across the union).
    pub(crate) use_change_overlay: Vec<AutounmaskChange>,
    /// This pass hit an autounmask contradiction (a flag wanted both on
    /// and off); `collect_feedback` + driver latches it into `bp.autounmask_use_broke`.
    pub(crate) use_broke: bool,
    /// #57: this pass dropped a `NoVisibleCandidate` on the
    /// `other_outcomes` dedup while a mask was active -- an
    /// unsatisfiable dependency whose entry `collect_feedback`'s
    /// `has_nvc` scan therefore cannot see. Set only under a non-empty
    /// `runtime_pkg_mask`, so an ordinary pass is unaffected.
    pub(crate) suppressed_nvc: bool,
    /// #59 S1: real `_add_pkg`'s per-instance `Parent Dep:` rows
    /// (`_add_parent_atom`), collected as each queue item resolves.
    pub(crate) parent_atoms: Vec<resolver_trace::ParentAtom>,
}
