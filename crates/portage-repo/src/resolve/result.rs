//! The resolver's result and how a run aborts.

use crate::*;

/// `--changed-deps-report`: an installed package, still in the graph at `version`,
/// whose vdb-recorded dependency strings differ from the repo's current ebuild for
/// that exact version -- but reported, not reinstalled. `repo_name` is the repo that
/// currently provides `version`, standing in for Portage's `pkg.repo` (no vdb
/// `REPOSITORY` reader; Portage's `if pkg.repo != ebuild.repo: continue` filter
/// requires equality first, so nothing is lost).
// Portage: depgraph.py, _changed_deps_report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedDepsReportEntry {
    pub category: String,
    pub package: String,
    pub version: String,
    pub repo_name: String,
}

/// Why a resolve was abandoned: Portage's `_create_graph` returns 0 the moment a
/// required dep cannot be satisfied or `_serialize_tasks` gives up on a cycle, and
/// `action_build` then runs `display_problems()` without ever calling `display()`,
/// so the shown merge list is at most partial and the exit status is 1. Produced by
/// [`abort_outcome`] from the settled graph; rendered and given its error block by
/// later slices, the exit code by `pretend::run`'s gated arm.
///
/// There is deliberately no "autounmask + cycle" variant: autounmask changes with a
/// cycle only cut the backtrack loop at try 0, and the list shown is still the stuck
/// remainder plus the autounmask block (an `UnserializableCycle` with non-empty
/// `autounmask_*_changes`, per the oracle).
// Portage: depgraph.py _create_graph, _serialize_tasks; actions.py action_build
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortReason {
    /// A dependency atom matching masked-only ebuilds: the disclosure half
    /// ships as [`MaskedDepReport`]; the abort half suppresses the merge list and
    /// exits 1. `atom` is the report's display atom, `parent_cpv` the first
    /// merge-bound requirer.
    MaskedDep { atom: String, parent_cpv: String },
    /// A dependency atom no visible package satisfies for any other reason:
    /// same abort shape, different error block. `atom` is the unevaluated dep atom as
    /// queued.
    // Portage: depgraph.py no-ebuilds-with-USE-flags flavour
    UnsatisfiedAtom { atom: String, parent_cpv: String },
    /// `_serialize_tasks` drained everything serializable and got stuck:
    /// the display is the stuck remainder only. `members` are `cat/pkg-version` CPVs
    /// in leaf-drain remainder order; tree nesting/`[nomerge]` rows stay a deliberate
    /// cut (dedup-by-design).
    UnserializableCycle { members: Vec<String> },
    /// A merge-bound `virtual/` package (transitively) requires itself through selected
    /// virtual providers, so it can never resolve. Portage raises out of the walk and
    /// fails with no merge list. `members` are `cpv::repo` lines in Portage's sorted
    /// order (see [`find_virtual_cycles`]); the renderer prefixes the two spaces.
    // Portage: depgraph.py _virt_deps_visible; _virtual_cycle_error:6215-6216
    VirtualCycle { members: Vec<String> },
}

/// Whether the resolve ran to completion or was abandoned: Portage's `select_files`
/// falsy-success paths as data. The `Aborted` partial list is what the renderer shows
/// *instead of* `entries` (masked/unsat shapes carry an empty partial; the cycle
/// shape carries the stuck remainder, one entry per `cycle_display` CPV in order).
// Portage: depgraph.py
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    Complete,
    Aborted {
        reason: AbortReason,
        partial: Vec<GraphEntry>,
    },
}

/// Rollout gate for the abort path (flag-gated): unset or any value but
/// `0` enables the abort rendering + exit codes; `PORTUALE_ABORT_PATH=0`
/// keeps the legacy "report, don't enforce" list and exit 0. Read off the environment
/// at the use site, so neither `ResolveRequest` nor the marshaller grows a parameter.
pub fn abort_path_enabled() -> bool {
    match std::env::var("PORTUALE_ABORT_PATH") {
        Ok(v) => v != "0",
        Err(_) => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphResult {
    pub entries: Vec<GraphEntry>,
    /// [`abort_outcome`] over the settled graph: `Aborted` exactly when Portage would
    /// have abandoned the resolve, `Complete` otherwise. `entries` is still the full
    /// graph either way; the renderer switches to `partial` when aborted.
    pub outcome: ResolveOutcome,
    /// Portage's `_backtrack_depgraph` `backtracked` counter: loop restarts
    /// (`passes - 1`), reported as `(backtrack: N/M)`. The wall-clock timing half
    /// stays a deliberate cut (non-deterministic); the count rides out in `--json`.
    // Portage: _show_resolution_report
    pub backtrack_restarts: u64,
    /// Portage's `--backtrack` budget `M` (`max_retries`).
    pub backtrack_max: u32,
    pub slot_conflicts: Vec<SlotConflict>,
    /// Withheld upgrades Portage reports through `WARNING: One or more
    /// updates/rebuilds have been skipped due to a dependency conflict:` (merge list
    /// keeps the keeper, rc 0). Rendered after the merge list; suppressed under
    /// `--quiet` unless `--debug`; `--json` returns before the block.
    // Portage: _show_missed_update
    pub skipped_updates: Vec<SkippedUpdate>,
    /// Slots Portage reports through the abbreviated unsatisfied-dependencies tail
    /// (see [`SkippedMissingDep`]). Rendered right after `skipped_updates`, in
    /// first-mask order.
    pub skipped_missing_deps: Vec<SkippedMissingDep>,
    /// Updates Portage reports through the full unsatisfied-dependencies form (see
    /// [`SkippedMissingDepFull`]). Rendered right after `skipped_missing_deps`.
    pub skipped_missing_dep_full: Vec<SkippedMissingDepFull>,
    /// Unresolved blocker rows whose owner has no display entry (scan-collected,
    /// absent from the graph). The renderer prints them in the trailing blocker group,
    /// counts them in `Conflict:`, and fails the run (Portage's n1b/n6b shape).
    pub orphan_blockers: Vec<OrphanBlocker>,
    pub changed_deps_report: Vec<ChangedDepsReportEntry>,
    /// `--buildpkgonly`: `true` when some entry that would newly merge has a
    /// `required_by` owner that would *also* newly merge. Portage only ever builds a
    /// binary without merging it, so every dependency of a to-be-built package must
    /// already be installed; otherwise it refuses to resolve at all. Always `false`
    /// when `buildpkgonly` wasn't requested.
    // Portage: lib/_emerge/depgraph.py; display_problems
    pub buildpkgonly_deps_unsatisfied: bool,
    /// The best-run pass still carries a non-empty replace set with missed updates and
    /// no prune latch: Portage wants the `prune_rebuilds` restart, cannot take it, and
    /// returns failure (graph displayed, exit 1).
    // Portage: depgraph.py, _resolve:5763-5775; allow_backtracking=False re-run
    pub prune_unresolved: bool,
    /// Directly-requested atoms that matched a `package.provided` entry and were
    /// therefore *not* resolved. The caller prints the `WARNING: … listed in
    /// package.provided:` block for these, in requested order. A *dependency* atom
    /// matching `package.provided` is silently dropped from the walk instead, never
    /// recorded here.
    // Portage: depgraph.py _pprovided_args; dep_check.py
    pub pprovided_atoms: Vec<String>,
    /// Top-level `@world`/`@selected` set members that selected no package on the
    /// settled pass. The caller prints the world-file notice plus the
    /// masked-or-don't-exist list from these, in requested order.
    // Portage: depgraph.py _missing_args; display_problems
    pub missing_args: Vec<String>,
    /// Portage `--autounmask` keyword changes applied to make the graph resolve: one
    /// per entry that only became visible because `autounmask_suggest_keywords`
    /// accepted a `KEYWORDS`-alone mask. Printed as the `The following keyword
    /// changes are necessary to proceed:` block, after the merge list. Empty unless
    /// `--autounmask` was explicit.
    // Portage: depgraph.py, _display_autounmask unstable_keyword_msg
    pub autounmask_keyword_changes: Vec<AutounmaskChange>,
    /// Portage `--autounmask` USE changes: one per entry whose atom use-deps a
    /// `package.use` flip fixed (`suggested_use_flip`). Printed as the `The following
    /// USE changes are necessary to proceed:` block, after the keyword one. On by
    /// default (unlike the keyword kind).
    // Portage: depgraph.py, _display_autounmask use_changes_msg
    pub autounmask_use_changes: Vec<AutounmaskChange>,
    /// Portage `--autounmask-license` changes: one per entry that only became visible
    /// because `autounmask_suggest_license` accepted a `LICENSE`-alone mask. `token`
    /// is the space-joined sorted missing-license names; `atom` uses the
    /// `>=`/`>=…:slot`/`=` form Portage picks. Printed as the license-changes block,
    /// last. Off unless `--autounmask` is explicit or `--autounmask-license=y`.
    // Portage: depgraph.py, _display_autounmask license_msg; check_if_latest
    pub autounmask_license_changes: Vec<AutounmaskChange>,
    /// Portage `--autounmask-keep-masks=n` changes: one per entry that only became
    /// visible because `autounmask_suggest_masks` accepted a `package.mask`-alone
    /// mask. `token` is empty; `atom` is always `=<cpv>`. Printed as the mask-changes
    /// block, between keyword and USE. Off unless `--autounmask-keep-masks=n`. v1 cut:
    /// no `# <filename>:` + masking-comment lines (no source-file provenance).
    // Portage: depgraph.py, _display_autounmask p_mask_change_msg
    pub autounmask_mask_changes: Vec<AutounmaskChange>,
    /// The settled pass took an autounmask path on which Portage's final `_resolve`
    /// would NOT have set `_success_without_autounmask`, so `need_config_change`
    /// reaches its third branch and the "terminated early" notice prints. Two shapes:
    /// a `package.use` flip folded for an already-graphed package, or a failed
    /// dependency rescued by a parent USE flip after the walk failed (default
    /// `--autounmask-backtrack` off). A fresh-candidate flip (picked with the flip
    /// before ever being graphed) leaves this false. Approximation: portuale folds a
    /// graphed-package flip unconditionally while Portage additionally requires the
    /// reduced deps to change, so this only ever keeps the notice, never removes it.
    // Portage: depgraph.py need_config_change:11708,11752; _resolve:5793;
    // want_restart_for_use_change:7719; _apply_parent_use_changes:5820
    pub autounmask_no_clean_tail: bool,
    /// The reported graph comes from a pass Portage runs with backtracking still
    /// allowed (the loop ended early on `need_config_change`), so the slot-conflict
    /// advisory omits its `--backtrack=30` hint (shown only when backtracking is off).
    // Portage: depgraph.py
    pub settled_with_backtracking: bool,
    /// `(provider-cpv, consumer-cpv)` pairs behind each slot-operator auto-rebuild:
    /// the consumer got a `Reinstall { slot_operator_rebuild: true }` entry because
    /// the provider's new sub-slot breaks its built `:S/SS=` link. Sorted, deduped.
    /// Rendered as the "The following packages are causing rebuilds:" block (grouped
    /// by provider), unless `--verbose-slot-rebuilds=n`.
    // Portage: _compute_abi_rebuild_info _forced_rebuilds; _show_abi_rebuild_info
    pub abi_rebuilds: Vec<(String, String)>,
    /// Unbreakable dependency cycles among the merge-bound entries: each inner `Vec`
    /// is one cycle as an ordered `cat/pkg-version` list where every element depends
    /// on the next via an unsatisfied build-time dep with no run-time alternative.
    /// Currently at most one entry (the shortest cycle), starting at Portage's `[0]`
    /// (`discovery` order stands in for digraph insertion order). Rendered as the
    /// `* Error: circular dependencies:` block with exit 1. Empty in ordinary resolves.
    // Portage: circular_dependency_handler _ignore_runtime; _find_cycles shortest_cycle;
    // _prepare_circular_dep_message; _show_circular_deps
    pub circular_deps: Vec<Vec<String>>,
    /// The settling pass's recorded cycle edges, empty when no backtrack restart
    /// happened. Fed back into the display-side derivations so `--tree` and the
    /// blocker disposition keep the cycle-breaking branch the walk took.
    // Portage: backtrack_infos[config][circular_dependency]
    pub circular_dependency: HashMap<RootCp, Vec<CircularDepChild>>,
    /// The full elementary-cycle enumeration counted more than three records. The
    /// renderer prints the "lots of cycles" trailer when this is set *and* a concrete
    /// `Change USE:` suggestion is shown (Portage only reaches the trailer inside the
    /// suggestions branch). Computed only when a hard cycle was already reported.
    // Portage: circular_dependency_handler.large_cycle_count; digraph.get_cycles medium_soft
    pub large_cycle_count: bool,
    /// The stuck-remainder members in leaf-drain order, as `cat/pkg-version` CPVs,
    /// merge-bound members only. Re-displayed as flat merge lines between the merge
    /// list and the circular-dependencies block (minus tree nesting: portuale's tree
    /// model dedups shared nodes by design).
    // Portage: circular_dependency_handler.merge_list; merge_order::reduced_merge_order
    pub cycle_display: Vec<String>,
    /// Portage's `depgraph._virt_deps_visible` probe-stack members for a virtual cycle
    /// (see [`find_virtual_cycles`]): `cpv::repo` lines in Portage's sorted order.
    /// Non-empty exactly when the outcome is `Aborted { VirtualCycle }`; rendered as
    /// the `!!! virtual cycle detected:` block with exit 1.
    pub virtual_cycle: Vec<String>,
    /// Masked-only dependency disclosures (Portage's "All ebuilds that could satisfy …
    /// have been masked" block for a *dependency* atom): one per dependency
    /// `NoVisibleCandidate` whose atom still matches ebuilds by version, every one of
    /// which is masked. Rendered in place of the bare `no visible ebuild` line with
    /// the `(dependency required by …)` chain. Empty when every dependency resolved or
    /// missed for other reasons.
    // Portage: depgraph.py, _show_unsatisfied_dep
    pub masked_deps: Vec<MaskedDepReport>,
    /// `[use]`-unsatisfied disclosures (Portage's separate "no ebuilds built with USE
    /// flags to satisfy" block): one per dependency `NoVisibleCandidate` whose atom
    /// names visible ebuilds that miss on USE. Rendered ahead of `masked_deps`.
    // Portage: depgraph.py, _show_unsatisfied_dep
    pub use_unsat_deps: Vec<UseUnsatDepReport>,
    /// Plain-miss disclosures (Portage's `emerge: there are no ebuilds to satisfy
    /// "<atom>"` block): one per dependency `NoVisibleCandidate` that is neither
    /// masked-only nor `[use]`-unsatisfied. Rendered after the two sibling blocks'
    /// precedence.
    // Portage: depgraph.py, _show_unsatisfied_dep
    pub plain_miss_deps: Vec<PlainMissDepReport>,
}

/// One Portage `--autounmask` change: the implicit `package.accept_keywords` /
/// `package.use` line plus its own `#required by …` dep-chain comment.
// Portage: depgraph.py, _display_autounmask; _get_dep_chain_as_comment
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutounmaskChange {
    /// The left-hand atom of the change line, op prefix included: `=<cpv>` for a
    /// keyword change, `>=<cpv>` / `>=<cpv>:<slot>` / `=<cpv>` for a USE change.
    // Portage: autounmask_unrestricted_atoms default n; check_if_latest
    pub atom: String,
    /// The token(s) to add after `atom`: a `~<arch>` keyword, or a space-joined
    /// `flag`/`-flag` list for a USE change.
    pub token: String,
    /// The `#required by …` comment lines, top-to-bottom, WITHOUT the leading `# `
    /// (the renderer adds it): `required by <cpv>::<repo>` for a package parent,
    /// `required by <atom> (argument)` for a command-line argument.
    // Portage: _get_dep_chain_as_comment
    pub dep_chain: Vec<String>,
    /// The `(category, package)` whose atom mismatch forced this USE flip. `None`
    /// for keyword/license/mask changes (rendered without the unsatisfied filter) and
    /// for flips whose trigger is the change owner itself (parent-flip rescues). The
    /// post-loop fill starts the chain there instead of at the first requirer; a stale
    /// trigger falls back to the legacy start. Travels with the record across
    /// backtrack passes (it names the flip site, which the re-resolve reproduces).
    // Portage: _get_dep_chain unsatisfied_dependency first parent
    pub trigger: Option<(String, String)>,
}
