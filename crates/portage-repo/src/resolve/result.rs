//! The resolver's result and how a run aborts.

use crate::*;

/// `--changed-deps-report`: an installed package, still in the graph at
/// `version`, whose vdb-recorded dependency strings differ from the
/// repo's current ebuild for that exact version (`deps_changed`) -- but
/// reported, not reinstalled (see `resolve_pretend_graph`'s own doc
/// comment). `repo_name` is the repo that currently provides `version`,
/// standing in for real `pkg.repo` (portuale has no vdb `REPOSITORY`
/// reader to know what the *installed* copy's own repo was) -- real
/// `_changed_deps_report`'s own `if pkg.repo != ebuild.repo: continue`
/// filter requires the two to already be equal before a package is even
/// collected, so using the ebuild's repo here loses no real cases,
/// consistent with every other "no vdb-metadata reader" simplification
/// already documented in this crate (e.g. `enqueue_dependencies`'s own
/// doc comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedDepsReportEntry {
    pub category: String,
    pub package: String,
    pub version: String,
    pub repo_name: String,
}

/// Why a resolve was abandoned (backlog #19, `docs/history/abort-path-spec.md`):
/// real's `_create_graph` returns 0 the moment a required dep cannot be
/// satisfied (`depgraph.py:3254-3271`) or `_serialize_tasks` gives up on
/// an unserializable cycle (`:10262-10294`) — and `action_build` then
/// runs `display_problems()` without ever calling `display()`
/// (`actions.py:460-462`), so the merge list shown is at most a partial
/// one and the exit status is 1. Produced by [`abort_outcome`] from the
/// settled graph (Slice 3); rendered and given its error block by the
/// later slices, the exit code by `pretend::run`'s gated arm.
///
/// There is deliberately no "autounmask + cycle" variant: the oracle
/// (spec §4d, `abort-au-cycle` / `abort-au-restart-cycle`) shows that
/// autounmask changes coinciding with a cycle only cut the backtrack
/// loop at try 0 (`need_config_change`, `:11736-11760`) — the list
/// displayed is still `_show_circular_deps`' stuck remainder, followed
/// by the autounmask block. That is [`AbortReason::UnserializableCycle`]
/// with `autounmask_*_changes` non-empty, not a fourth membership shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortReason {
    /// A dependency atom matching masked-only ebuilds (spec §4a): the
    /// disclosure half already ships as [`MaskedDepReport`]; the abort
    /// half suppresses the merge list and exits 1. `atom` is the report's
    /// display atom, `parent_cpv` the first merge-bound requirer.
    MaskedDep { atom: String, parent_cpv: String },
    /// A dependency atom no visible package satisfies for any other
    /// reason (spec §4b — nothing matches at all; also a `[use]`-dep
    /// mismatch no autounmask flip resolves, real's "there are no
    /// ebuilds built with USE flags" flavour, `depgraph.py:3479-3483`):
    /// same abort shape as [`AbortReason::MaskedDep`], different error
    /// block. `atom` is the unevaluated dep atom as queued.
    UnsatisfiedAtom { atom: String, parent_cpv: String },
    /// `_serialize_tasks` drained everything serializable and got stuck
    /// (spec §4c): the display is the stuck remainder only, with
    /// cumulative counters. `members` are `cat/pkg-version` CPVs in
    /// leaf-drain remainder order (`GraphResult::cycle_display`); the
    /// tree nesting/`[nomerge]` rows stay a deliberate cut
    /// (dedup-by-design, Gate G0.2).
    UnserializableCycle { members: Vec<String> },
    /// Real `depgraph._virt_deps_visible`'s virtual-cycle failure
    /// (backlog #193): a merge-bound `virtual/` package (transitively)
    /// requires itself through selected virtual providers, so the
    /// virtual can never resolve to a real package. Real raises out of
    /// the walk (`_virtual_cycle_error`, `depgraph.py:6215-6216`),
    /// prints `!!! virtual cycle detected:` with one `  {cpv}::{repo}`
    /// line per probe-stack member (`select_files`, `:5002-5013`), and
    /// fails with no merge list. `members` are those `cpv::repo` lines
    /// in real's sorted order (see [`find_virtual_cycles`]); the
    /// renderer prefixes the two spaces.
    VirtualCycle { members: Vec<String> },
}

/// Whether the resolve ran to completion or was abandoned: real's
/// `select_files` falsy-success paths (`depgraph.py:5676-5683`) as data.
/// The `Aborted` partial list is what Slice 4 renders *instead of*
/// `entries` (masked/unsat shapes carry an empty partial — real shows no
/// list at all; the cycle shape carries the stuck remainder, one entry
/// per `cycle_display` CPV in that order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    Complete,
    Aborted {
        reason: AbortReason,
        partial: Vec<GraphEntry>,
    },
}

/// Rollout gate for the abort path (Gate G0.4, flag-gated): unset or any
/// value but `0` enables the Slice-5 abort rendering + exit codes;
/// `PORTUALE_ABORT_PATH=0` keeps the legacy "report, don't enforce"
/// merge list and exit 0. Read directly off the environment at the use
/// site (same pattern as `PORTAGE_SERIALIZE_FRONTIER_DISABLE` in
/// `merge_order.rs`), so neither `ResolveRequest` nor the ~60-arg
/// `resolve_pretend_graph` marshaller grows a parameter for it.
pub fn abort_path_enabled() -> bool {
    match std::env::var("PORTUALE_ABORT_PATH") {
        Ok(v) => v != "0",
        Err(_) => true,
    }
}

/// Arguments of [`abort_outcome`].
pub struct AbortOutcomeArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub masked_deps: &'a [MaskedDepReport],
    pub circular_deps: &'a [Vec<String>],
    pub cycle_display: &'a [String],
    pub cycle_display_idx: &'a [usize],
    pub nvc_dep_atoms: &'a HashMap<(String, String), String>,
    pub virtual_cycle: &'a [String],
}

/// Derives the abort outcome from the settled graph — real's three
/// abandon sites as a post-loop classification, since portuale's
/// `'backtrack` loop (unlike real's `_backtrack_depgraph`) rebuilds
/// `entries` per pass and the final pass is the one real would display
/// (spec §6, "the final (best-run-equivalent) pass deciding").
///
/// Precedence follows real's control flow: a walk-time failure
/// (`_add_dep` returning 0 → `_create_graph` 0 → `_resolve` returns
/// before `altlist()`, `depgraph.py:5676-5681`) wins over a
/// serialize-time one (the `_serialize_tasks` give-up inside
/// `altlist()`), so a masked/unsat dependency beats a cycle in the same
/// graph (oracle: `abort-masked-cycle` — no list, no circular block).
/// Between several walk-time failures real records only the first one
/// its DFS reaches; portuale takes the first in BFS admission order
/// (`entries` order) here. Backlog #244 closed the cut where it shows:
/// the autounmask-breakage clean pass (the `aub0` two-losing-deps shape
/// the fixtures once lacked) is re-cut afterwards to real's fail-fast
/// walk -- first failure, its own chain, only the slot conflicts admitted
/// before it (`fail_fast_truncate`, `merge_order::replay_create_graph`).
///
/// A `NoVisibleCandidate` dependency entry always aborts: real has no
/// installed-parent rescue for a settled miss. Its
/// `_initially_unsatisfied_deps` path (`:3425-3437` complete-mode
/// beyond-depth, `:3458-3470` masked-parent backward-compat, both
/// "error message but do not cause the dependency calculation to
/// fail") is narrow -- a use-constrained miss goes to
/// `_select_package(without_use)` → rebuild-or-hard-abort instead
/// (`:3473-3483`, `:6880-6900`), and a nothing-matches miss to the
/// missing-dependency backtrack. Portuale used to rescue every
/// installed-only NVC here (no merge-bound requirer → `Complete`),
/// which is exactly what hid #132 on the live tree: the evaluated
/// `-wayland` qtbase atom settled `NoVisibleCandidate` with only
/// installed requirers, the block rendered, and the run exited 0
/// where real aborts (`#111 S0`, `#132` arm D oracle
/// `l0-fx-20260921T231520Z`). A top-level miss is already fatal via
/// the argument path, not this one. Masked-only atoms are those with a
/// [`MaskedDepReport`]; every other miss — nothing matches, or a
/// `[use]` dep no autounmask flip resolves — is `UnsatisfiedAtom`
/// (`_select_package(dep.atom.without_use)` succeeding still returns 0,
/// `:3479-3483`, oracled by the `abort-au-plain` capture taken with
/// `--autounmask-use` off). `nvc_dep_atoms` carries each such entry's
/// unevaluated atom as queued.
pub fn abort_outcome(args: AbortOutcomeArgs<'_>) -> ResolveOutcome {
    let AbortOutcomeArgs {
        entries,
        masked_deps,
        circular_deps,
        cycle_display,
        cycle_display_idx,
        nvc_dep_atoms,
        virtual_cycle,
    } = args;
    // Backlog #193: real's virtual-cycle failure is an exception raised
    // out of the walk (`_virtual_cycle_error` propagating through
    // `_resolve` to `select_files`, `depgraph.py:5002`), so it preempts
    // every serialize-time failure the way a walk-time masked/unsat miss
    // does -- checked before both. Against a masked/unsat miss in the
    // same graph real reports whichever its DFS reaches first (a walk-
    // order artefact); virtual-first is the documented choice, and no
    // fixture shapes both at once.
    if !virtual_cycle.is_empty() {
        return ResolveOutcome::Aborted {
            reason: AbortReason::VirtualCycle {
                members: virtual_cycle.to_vec(),
            },
            partial: Vec::new(),
        };
    }
    for entry in entries {
        if !matches!(entry.outcome, PretendOutcome::NoVisibleCandidate) {
            continue;
        }
        // The abort's `parent_cpv` is the first merge-bound requirer;
        // with installed-only requirers it is the first requirer's own
        // installed cpv (same `cat/pkg-version` shape as
        // `merge_bound_cpv`). An NVC entry with no resolvable requirer
        // at all keeps the old rescue -- there is nobody to blame.
        let Some(parent_cpv) = entry
            .required_by
            .iter()
            .find_map(|(c, p)| {
                entries
                    .iter()
                    .find(|o| &o.category == c && &o.package == p)
                    .and_then(merge_bound_cpv)
            })
            .or_else(|| {
                entry.required_by.iter().find_map(|(c, p)| {
                    entries
                        .iter()
                        .find(|o| &o.category == c && &o.package == p)
                        .and_then(|o| match &o.outcome {
                            PretendOutcome::AlreadyInstalled { version } => {
                                Some(format!("{c}/{p}-{version}"))
                            }
                            _ => None,
                        })
                })
            })
        else {
            continue;
        };
        let key = (entry.category.clone(), entry.package.clone());
        let reason = match masked_deps
            .iter()
            .find(|r| r.category == entry.category && r.package == entry.package)
        {
            Some(rep) => AbortReason::MaskedDep {
                atom: rep.atom.clone(),
                parent_cpv,
            },
            None => AbortReason::UnsatisfiedAtom {
                atom: nvc_dep_atoms
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| format!("{}/{}", entry.category, entry.package)),
                parent_cpv,
            },
        };
        return ResolveOutcome::Aborted {
            reason,
            partial: Vec::new(),
        };
    }
    if !circular_deps.is_empty() {
        // Backlog #245/#287: the drain's own node indices keep the two
        // instances of a cp that lives in both roots apart (target and
        // running root share one cpv string); the cpv lookup is the
        // fallback for callers without indices.
        let partial = if cycle_display_idx.is_empty() {
            cycle_display
                .iter()
                .filter_map(|cpv| {
                    entries
                        .iter()
                        .find(|e| merge_bound_cpv(e).as_deref() == Some(cpv.as_str()))
                        .cloned()
                })
                .collect()
        } else {
            cycle_display_idx
                .iter()
                .map(|&i| entries[i].clone())
                .collect()
        };
        return ResolveOutcome::Aborted {
            reason: AbortReason::UnserializableCycle {
                members: cycle_display.to_vec(),
            },
            partial,
        };
    }
    ResolveOutcome::Complete
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphResult {
    pub entries: Vec<GraphEntry>,
    /// [`abort_outcome`] over the settled graph (Slice 3): `Aborted`
    /// exactly when real would have abandoned the resolve, `Complete`
    /// otherwise. `entries` is still the full graph either way; the
    /// renderer switches to `partial` in Slice 4.
    pub outcome: ResolveOutcome,
    /// #59 S2 (K1 option (a)): real `_backtrack_depgraph`'s
    /// `backtracked` counter -- the number of loop restarts the search
    /// made (`passes - 1`), reported by `_show_resolution_report` as
    /// `(backtrack: N/M)`. The wall-clock timing half stays a deliberate
    /// cut (non-deterministic); the count rides out in `--json` only.
    pub backtrack_restarts: u64,
    /// Real's `--backtrack` budget `M` (`max_retries`, the `--backtrack`
    /// option's value).
    pub backtrack_max: u32,
    pub slot_conflicts: Vec<SlotConflict>,
    /// Backlog #90 (S2) + #92: withheld upgrades real reports through
    /// `WARNING: One or more updates/rebuilds have been skipped due to
    /// a dependency conflict:` (merge list keeps the keeper, the
    /// skipped version does not merge, rc 0). Rendered after the merge
    /// list by `pretend.rs`, suppressed under `--quiet` unless
    /// `--debug` (real `_show_missed_update`); `--json` returns before
    /// the block and `--columns` has no gate.
    pub skipped_updates: Vec<SkippedUpdate>,
    /// Backlog #129 (S1): slots real reports through the abbreviated
    /// unsatisfied-dependencies tail (see [`SkippedMissingDep`]).
    /// Rendered right after [`GraphResult::skipped_updates`] by
    /// `pretend.rs`, in first-mask order like the `WARNING` rows.
    pub skipped_missing_deps: Vec<SkippedMissingDep>,
    /// Backlog #198: updates real reports through the full
    /// unsatisfied-dependencies form (see [`SkippedMissingDepFull`]).
    /// Rendered right after [`GraphResult::skipped_missing_deps`] by
    /// `pretend.rs`, in the same first-mask order.
    pub skipped_missing_dep_full: Vec<SkippedMissingDepFull>,
    /// Backlog #80: unresolved blocker rows whose owner has no display
    /// entry (scan-collected, absent from the graph). The renderer
    /// prints them in the trailing blocker group, counts them in
    /// `Conflict:`, and fails the run -- real's n1b/n6b shape.
    pub orphan_blockers: Vec<OrphanBlocker>,
    pub changed_deps_report: Vec<ChangedDepsReportEntry>,
    /// `--buildpkgonly`'s own real depgraph check
    /// (`lib/_emerge/depgraph.py:5706-5717`): `true` when some entry
    /// that would newly merge (`New`/`Upgrade`/`Downgrade`/`Reinstall` --
    /// anything but `AlreadyInstalled`/`NoVisibleCandidate`) has a
    /// `required_by` owner that would *also* newly merge. Real
    /// `--buildpkgonly` only ever builds a binary package without
    /// merging it, so every one of a to-be-built package's own
    /// dependencies must already be satisfied by something already
    /// installed -- if a dependency itself also needs building, real
    /// portage refuses to resolve at all ("--buildpkgonly requires all
    /// dependencies to be merged", `depgraph.py`'s own
    /// `display_problems()`). Always `false` when `buildpkgonly` wasn't
    /// requested at all.
    pub buildpkgonly_deps_unsatisfied: bool,
    /// Backlog #295: the best-run pass (real's `allow_backtracking=False`
    /// re-run) still carries a non-empty replace set with missed updates
    /// and no prune latch. Real's `_resolve` (`depgraph.py:5763-5775`)
    /// then wants the `prune_rebuilds` restart, cannot take it, and
    /// returns failure: the graph is displayed and the run exits 1.
    pub prune_unresolved: bool,
    /// Directly-requested (top-level) atoms that matched a
    /// `package.provided` entry (`config.package_provided`) and were
    /// therefore *not* resolved -- real `depgraph.py:5497-5615`'s own
    /// `_pprovided_args`. The caller (`pretend.rs`) prints real
    /// `depgraph.py:11192-11235`'s `WARNING: … listed in
    /// package.provided:` block for these, in requested order. A
    /// *dependency* atom matching `package.provided` is silently dropped
    /// from the dep walk instead (real `dep_check.py:1052`), never
    /// recorded here.
    pub pprovided_atoms: Vec<String>,
    /// Backlog #286: top-level `@world`/`@selected` set members that
    /// selected no package on the settled pass. Real drops them into
    /// `_missing_args` (`depgraph.py:5577-5605`) and continues the walk;
    /// the caller (`pretend.rs`) prints real `display_problems()`'s
    /// world-file notice plus the masked-or-don't-exist list from these,
    /// in requested order.
    pub missing_args: Vec<String>,
    /// Real `--autounmask` keyword changes that were applied to make the
    /// graph resolve (real `depgraph.py::_display_autounmask`'s
    /// `unstable_keyword_msg`): one per entry that only became visible
    /// because `autounmask_suggest_keywords` let `resolve_pretend` accept
    /// a `KEYWORDS`-alone mask. The caller (`pretend.rs`) prints the
    /// `The following keyword changes are necessary to proceed:` block
    /// from these, after the merge list. Empty unless `--autounmask` was
    /// explicit (see `autounmask_suggest_keywords`).
    pub autounmask_keyword_changes: Vec<AutounmaskChange>,
    /// Real `--autounmask` USE changes applied to make the graph resolve
    /// (real `_display_autounmask`'s `use_changes_msg`): one per entry
    /// whose atom use-deps didn't match its default USE state but a
    /// `package.use` flip fixed it (`suggested_use_flip`). Printed as the
    /// `The following USE changes are necessary to proceed:` block, after
    /// the keyword one. On by default (`autounmask_suggest_use`, unlike
    /// the keyword kind).
    pub autounmask_use_changes: Vec<AutounmaskChange>,
    /// Real `--autounmask-license` changes applied to make the graph
    /// resolve (real `_display_autounmask`'s `license_msg`): one per
    /// entry that only became visible because `autounmask_suggest_license`
    /// let `resolve_pretend` accept a `LICENSE`-alone mask. `token` is the
    /// space-joined sorted missing-license names; `atom` uses the `>=`
    /// (latest) / `>=…:slot` / `=` form real `check_if_latest(pkg)`
    /// (no `check_visibility`) picks. Printed as the `The following
    /// license changes are necessary to proceed:` block, last. Off unless
    /// `--autounmask` is explicit or `--autounmask-license=y`.
    pub autounmask_license_changes: Vec<AutounmaskChange>,
    /// Real `--autounmask-keep-masks=n` changes (real `_display_autounmask`'s
    /// `p_mask_change_msg`): one per entry that only became visible because
    /// `autounmask_suggest_masks` let `resolve_pretend` accept a
    /// `package.mask`-alone mask. `token` is empty (a mask unmask has no
    /// keyword/flag); `atom` is always `=<cpv>` (like keywords, real
    /// `autounmask_unrestricted_atoms` defaults `"n"`). Printed as the
    /// `The following mask changes are necessary to proceed:` block,
    /// between keyword and USE. Off unless `--autounmask-keep-masks=n`.
    /// v1 cut: real's `# <filename>:` + masking-comment lines (no
    /// source-file provenance on portuale's `package_mask` list).
    pub autounmask_mask_changes: Vec<AutounmaskChange>,
    /// Backlog #217: the settled pass took an autounmask path on which
    /// real's final `_resolve` would NOT have set `_success_without_
    /// autounmask` -- so `need_config_change` (`depgraph.py:11708`)
    /// reaches its third branch (`:11752`) and sets `_autounmask_
    /// backtrack_disabled`, and the "terminated early" notice prints.
    /// Two shapes, both observed on the final pass:
    ///
    /// * a `package.use` flip was folded for an already-graphed
    ///   package (the already-resolved-slot re-check or the parent
    ///   flip with `--autounmask-backtrack=y` grew the settled
    ///   `autounmask_use_config` accumulator -- real `_needed_use_
    ///   config_changes` grown for a digraph node). Real's
    ///   `want_restart_for_use_change` (`:7719`) answers True there,
    ///   so `_resolve` returns before the autounmask tail (oracle:
    ///   `aucasctop` prints the notice, `fixtures/abort-captures/`);
    /// * a failed dependency was rescued by a parent USE flip after
    ///   the walk failed (default `--autounmask-backtrack` off: real
    ///   `_apply_parent_use_changes` (`:5820`) collects the flip into
    ///   `_needed_use_config_changes` post-failure, so `_have_
    ///   autounmask_changes()` holds while the tail was never
    ///   reached).
    ///
    /// A fresh-candidate flip (the package is picked with the flip
    /// before it is ever graphed) leaves this False: real reaches the
    /// tail (`:5793`, "reserved for cases where there are *zero* other
    /// problems"), the early return (`:11713-11717`) fires, and the
    /// notice stays off (oracle: `abort-au-plain`).
    ///
    /// Approximation: portuale folds a graphed-package flip
    /// unconditionally while real additionally requires the reduced
    /// deps to change (or a parent USE-dep to break), so such a flip
    /// still counts here -- that only ever keeps the notice (the
    /// status quo), never removes it where real prints it.
    pub autounmask_no_clean_tail: bool,
    /// Backlog #244: the reported graph comes from a pass real runs with
    /// backtracking still allowed -- the loop ended early on
    /// `need_config_change` (`depgraph.py:12228-12234`) -- so real's
    /// slot-conflict advisory omits its `--backtrack=30` hint (shown only
    /// when `not _allow_backtracking`, `depgraph.py:1750-1759`).
    pub settled_with_backtracking: bool,
    /// `(provider-cpv, consumer-cpv)` pairs behind each slot-operator
    /// auto-rebuild (real `_compute_abi_rebuild_info`'s `_forced_rebuilds`):
    /// the consumer got a `Reinstall { slot_operator_rebuild: true }`
    /// entry because the provider's new sub-slot breaks its built
    /// `:S/SS=` link. Sorted, deduped. The caller (`pretend.rs`) renders
    /// real `_show_abi_rebuild_info`'s "The following packages are
    /// causing rebuilds:" block (grouped by provider), unless
    /// `--verbose-slot-rebuilds=n`.
    pub abi_rebuilds: Vec<(String, String)>,
    /// Unbreakable dependency cycles among the merge-bound entries: each
    /// inner `Vec` is one cycle as an ordered list of `cat/pkg-version`
    /// CPVs where every element depends on the next (and the last on the
    /// first) via an unsatisfied build-time dep with no run-time
    /// alternative -- real `circular_dependency_handler`'s
    /// `_ignore_runtime` cycle set, the only case real portage errors on
    /// with `* Error: circular dependencies:`. Currently at most one
    /// entry (the shortest cycle -- real `_find_cycles`'s
    /// `shortest_cycle`, all `_prepare_circular_dep_message` renders),
    /// starting where real's does (its `[0]`, backlog #278 -- `discovery`
    /// order stands in for real's digraph insertion order). `pretend.rs`
    /// renders `_show_circular_deps`'s block and exits 1 when this is
    /// non-empty. Empty in every ordinary resolve.
    pub circular_deps: Vec<Vec<String>>,
    /// Backlog #221: the settling pass's recorded cycle edges (real
    /// `backtrack_infos["config"]["circular_dependency"]`), empty when no
    /// backtrack restart happened. The caller (`pretend.rs`) feeds it back
    /// into the display-side `kept_alt_branches` / `resolved_dep_targets`
    /// derivations so `--tree` and the blocker disposition keep the
    /// cycle-breaking branch the walk took.
    pub circular_dependency: HashMap<RootCp, Vec<CircularDepChild>>,
    /// Real `circular_dependency_handler.large_cycle_count`: the full
    /// elementary-cycle enumeration (`merge_order::cycle_report`, real
    /// `digraph.get_cycles` over the `medium_soft` rung) counted more
    /// than three records. The renderer prints real's "the dependency
    /// graph contains a lot of cycles" trailer when this is set *and* a
    /// concrete `Change USE:` suggestion is shown (real only reaches the
    /// trailer inside the suggestions branch). Computed only when a hard
    /// cycle was already reported (the only path that consumes it).
    pub large_cycle_count: bool,
    /// Real `circular_dependency_handler.merge_list`: the stuck-remainder
    /// members in leaf-drain order (`merge_order::reduced_merge_order`
    /// over cycle members plus their transitive requirers), as
    /// `cat/pkg-version` CPVs, merge-bound members only (installed and
    /// unresolvable entries have no merge line to re-display). The
    /// renderer re-displays these as flat merge lines between the merge
    /// list and the `* Error: circular dependencies:` block -- real's
    /// cycle-only `--tree` re-display minus the tree nesting (portuale's
    /// tree model dedups shared nodes by design, so it cannot duplicate
    /// a package under two parents the way real's ordered tree does;
    /// isolating *which* packages is what's ported).
    pub cycle_display: Vec<String>,
    /// Backlog #193: real `depgraph._virt_deps_visible`'s probe-stack
    /// members for a virtual cycle among the merge-bound entries (see
    /// [`find_virtual_cycles`]): `cpv::repo` lines in real's sorted
    /// order. Non-empty exactly when the outcome is
    /// `Aborted { VirtualCycle }`; the renderer prints real's
    /// `!!! virtual cycle detected:` block from it and exits 1.
    pub virtual_cycle: Vec<String>,
    /// Masked-only dependency disclosures (real `_show_unsatisfied_dep`'s
    /// "All ebuilds that could satisfy … have been masked" block for a
    /// *dependency* atom): one per dependency `NoVisibleCandidate` entry
    /// whose atom still matches ebuilds by version, every one of which
    /// is masked. The caller (`pretend.rs`) renders each in place of the
    /// bare `!!! no visible ebuild for dependency` line, followed by the
    /// `(dependency required by …)` chain walked out of the final
    /// entries. Empty when every dependency resolved or missed for other
    /// reasons (nothing matched at all, or only a `[use]`-dep mismatch
    /// -- the autounmask-use path).
    pub masked_deps: Vec<MaskedDepReport>,
    /// `[use]`-unsatisfied dependency disclosures (backlog #20, real
    /// `_show_unsatisfied_dep`'s separate "no ebuilds built with USE
    /// flags to satisfy" block): one per dependency `NoVisibleCandidate`
    /// entry whose atom names visible ebuilds that miss on USE. Real's
    /// precedence renders this block ahead of `masked_deps`; the caller
    /// (`pretend.rs`) does the same. Empty when no dependency missed
    /// that way.
    pub use_unsat_deps: Vec<UseUnsatDepReport>,
    /// Plain-miss dependency disclosures (#135 (d), real's
    /// `emerge: there are no ebuilds to satisfy "<atom>"` block): one
    /// per dependency `NoVisibleCandidate` that is neither masked-only
    /// nor `[use]`-unsatisfied. The caller renders each in place of the
    /// bare `!!! no visible ebuild for dependency` line, after the two
    /// sibling blocks' precedence.
    pub plain_miss_deps: Vec<PlainMissDepReport>,
}

impl GraphResult {
    /// Backlog #217: real `_dynamic_config._autounmask_backtrack_
    /// disabled` as a pure predicate over the settled result -- the
    /// gate for `_display_autounmask`'s "backtracking has terminated
    /// early" tail (`depgraph.py:11093`, set at exactly one site,
    /// `need_config_change:11752`).
    ///
    /// Real sets the flag iff autounmask changes exist (`_have_
    /// autounmask_changes`), backtracking is allowed (`_allow_
    /// backtracking`, i.e. `--backtrack` budget > 0) and not opted back
    /// in (`--autounmask-backtrack=y`, or `--autounmask-continue`
    /// implying it), AND `need_config_change` reaches its third branch:
    /// the early return (`:11713-11717`) did not fire -- neither
    /// `_success_without_autounmask` (the `_resolve` tail `:5793`,
    /// "reserved for cases where there are *zero* other problems") nor
    /// `_required_use_unsatisfied` (`:3652`) -- and the slot-conflict
    /// handler branch (`:11719-11736`) found no changes to suggest.
    ///
    /// Portuale's terms for "zero other problems" (i.e. real would have
    /// reached the tail): the outcome is `Complete`, no restart-worthy
    /// flip was folded and no post-failure parent rescue ran
    /// ([`GraphResult::autounmask_no_clean_tail`]), no slot conflict
    /// was recorded, no unsolvable blocker exists, and
    /// `--buildpkgonly`'s own check is satisfied. Anything else means
    /// another failure coincides and the notice stays. Known
    /// approximations, all on the keep-the-notice side (never a new
    /// divergence where real prints it): portuale has no slot-conflict
    /// handler (branch 2 would suppress there) and no REQUIRED_USE
    /// early-return signal, and `autounmask_no_clean_tail`
    /// over-approximates `want_restart_for_use_change` (see its docs).
    /// Warnings that do not fail real's `_resolve` (`skipped_updates`,
    /// `skipped_missing_deps`, the masked/use-unsat/plain-miss
    /// disclosures on a `Complete` outcome) correctly do not count.
    pub fn autounmask_backtrack_disabled(&self, autounmask_backtrack: bool) -> bool {
        let has_changes = !self.autounmask_keyword_changes.is_empty()
            || !self.autounmask_mask_changes.is_empty()
            || !self.autounmask_use_changes.is_empty()
            || !self.autounmask_license_changes.is_empty();
        has_changes
            && !autounmask_backtrack
            && self.backtrack_max > 0
            && (matches!(self.outcome, ResolveOutcome::Aborted { .. })
                || self.autounmask_no_clean_tail
                || !self.slot_conflicts.is_empty()
                || self.buildpkgonly_deps_unsatisfied
                || self
                    .entries
                    .iter()
                    .any(|e| e.blockers.iter().any(|b| b.unsolvable))
                || !self.orphan_blockers.is_empty())
    }
}

/// One real `--autounmask` change (`depgraph.py::_display_autounmask`):
/// the implicit `package.accept_keywords` / `package.use` line plus its
/// own `#required by …` dep-chain comment (`_get_dep_chain_as_comment`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutounmaskChange {
    /// The left-hand atom of the change line, op prefix included:
    /// `=<cpv>` for a keyword change (real `autounmask_unrestricted_
    /// atoms` defaults to `"n"`), `>=<cpv>` / `>=<cpv>:<slot>` / `=<cpv>`
    /// for a USE change (real `check_if_latest(check_visibility=True)`,
    /// bug #536392).
    pub atom: String,
    /// The token(s) to add after `atom` -- a `~<arch>` keyword, or a
    /// space-joined `flag`/`-flag` list for a USE change.
    pub token: String,
    /// The `#required by …` comment lines, top-to-bottom, WITHOUT the
    /// leading `# ` (the renderer adds it) -- real
    /// `_get_dep_chain_as_comment`: `required by <cpv>::<repo>` for a
    /// package parent, `required by <atom> (argument)` for a command-line
    /// argument.
    pub dep_chain: Vec<String>,
    /// Backlog #205: the `(category, package)` whose atom mismatch
    /// forced this USE flip (real `_get_dep_chain`'s
    /// `unsatisfied_dependency` first parent). `None` for
    /// keyword/license/mask changes (real renders those without the
    /// unsatisfied filter) and for flips whose trigger is the change
    /// owner itself (parent-flip rescues). The post-loop fill starts
    /// the chain there instead of at the first requirer; a stale
    /// trigger (no longer a requirer) falls back to the legacy start.
    /// Travels with the record across backtrack passes (it names the
    /// flip site, which the re-resolve reproduces).
    pub trigger: Option<(String, String)>,
}
