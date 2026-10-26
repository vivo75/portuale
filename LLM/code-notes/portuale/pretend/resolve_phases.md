# crates/portuale/src/pretend/resolve_phases.rs
Running the resolver: the first pass, the locked merges, the complete-graph pass, and the running-root repaint.

## fn resolve_once
- #72 B2c: the blocker classification follows Portage's complete-mode retry even on a non-complete pass (`blocker_retry_seed_atoms`), while `complete_seed_atoms` stays gated on `with_seeds` -- filling *that* field here would switch the slot-operator-rebuild scan on for every plain resolve (the trap the plan calls out).
- New code builds a `ResolveRequest` and calls the runtime-selected `Resolver` directly (see `resolve_pretend_graph`'s doc comment) -- this is the one production call site that carries `--solver=`.

## fn run_resolve_phases
- Portage never prefixes a `!!!`-headed report (masked packages, the REQUIRED_USE "has unmet requirements" block) with `emerge: ` -- it is `writemsg`-emitted verbatim by `_show_unsatisfied_dep`.
- Phase 1 is **always non-complete** -- the authoritative merge list. Portage's `_complete_graph` runs *after* the normal graph is built and only extends it (graph-or-installed selection, never a new merge).
- The `cp`s phase 1 would actually merge -- Portage's "already in the graph" for `_select_pkg_from_graph`.
- Phase 2 (`_complete_graph`): run when `--complete-graph` is explicit, or auto-enabled -- Portage's `depgraph.py:8581-8648`, re-walk in complete mode when the first graph changes an already-installed package and `--complete-graph-if-new-use`/`-if-new-ver`/`--rebuild-if-new-slot` is on. Portage has no `--deep` condition (it forces deep itself when not already deep); when `--deep` already forced the deep walk, the locked-gate re-resolve is skipped -- the deep walk already visited the reachable closure -- and the phase-1 walk is instead re-run below with only the `complete_seed_atoms` fed in (they feed `slot_op_reachable` and nothing else -- see `ResolveCtx::new`).
- `--deep` in force (the only way to reach here with `auto_enable` true): Portage would still run `_complete_graph`, so the phase-1 walk is re-run with the seeds fed in.
- Backlog #242 Slice D: repaint running-rooted rows from the running root's own profile (S0 section 2) -- merge entries and abort-partial rows alike (the circular re-display renders the partial list). Single-root runs (and hermetic trees without a running profile) keep the resolve-time target paint.
- Backlog #351 S7: the resolve-error handler prints the resolution notice (stdout) before the report when `top_level_abort_notice_shown` fires — a non-`--pretend` top-level all-masked abort — via `show_resolution_header_with_counts(opts, 0, opts.backtrack_max)`. The notice precedes the stderr block as in Portage (resolution display runs before `display_problems` there).

## fn run_resolve_phases (#394 restart + locked-merge reuse)
- Backlog #394: the restart itself lives in the driver's feedback loop (`step_rebuild_if_restart`, the #24 S3 precedent -- routing the rebuild through the `Backtracker` instead of synthesising an entry), not here: the budget, the `passes - 1` count and the `--backtrack=0` gate come for free, and every `resolve()` (phase 1 *and* phase 2) re-derives its own rebuilds. The report's sketch put the loop here; the driver home is smaller and keeps pubgrub untouched (the bridge never runs the driver).
- Phase 2 reuses phase-1 merges (`complete_reuse`, except restart reinstalls -- the complete walk re-derives those through its own scan + restart, which is also what records their `r`, exactly like the slot-operator rebuilds) so complete mode's no-update rule cannot demote them. The phase-1 restart count carries over only when phase 1 did a `--rebuild-if-*` restart (`rebuild_if_reinstalls` non-empty): an unseeded first walk can misfire a slot-operator probe the seeded re-walk avoids (r25: 1 vs 0), and Portage reports the re-walk's own count there.

## fn run_resolve_phases (#394 reuse excludes reinstalls)
- `complete_reuse` carries only `New`/`Upgrade`/`Downgrade` picks: every `Reinstall` re-derives through the complete walk's own machinery, so a reused record can neither freeze a dropped `r` (the `soc` top-level consumer) nor skip the eliminate latch that withholds the display pair. `--reinstall-atoms`/USE/argument reinstalls re-derive identically through their complete-safe flips.

## fn run_resolve_phases (complete_reuse, multi-slot)
Bed G-R (2026-10-20) found that reusing phase 1's picks keyed by cp lost one
slot of a multi-slot cp: `www-client/firefox`, thunderbird, nautilus and
gnome-shell dropped `dev-lang/rust-bin:1.96.1` next to `:1.94.1` (Total off by
one). Cps with more than one phase-1 entry are now removed from the reuse map
and re-resolve as before the #394 change. Verified on the bed: the four probes
are byte-identical to the pre-#394 baseline.

## fn colorize_abort_report
- Backlog #453 A: the resolver library builds abort reports plain, so Portage's `_show_unsatisfied_dep` spans (`BAD`/`INFORM` masked and REQUIRED_USE headers, `green` missed atom, `INFORM` chain nodes; `depgraph.py:6932-6936,6993-6999,7040-7043,7074`) are coloured at the print site in `run_resolve_phases::handle`, line by line. Disabled colour is the identity, so uncoloured pins are untouched.

## allow-list wiring (#571)
- The production `ResolveRequest` carries the run-shared `Categories` (`Some`); the abort path's `misspell_suggestion_block`, the complete-graph auto-enable gate and the running-root repaint gate on the same shared set, so the unlisted-category `!!!` fires once per run.

## fn run_resolve_phases required-use preamble (#519)
- A REQUIRED_USE unmet-requirements abort prints the resolution preamble first (stdout), like every other `--pretend` run that reaches resolution: Portage emits it at action start (`_start_resolution_display`), ahead of the rows.
