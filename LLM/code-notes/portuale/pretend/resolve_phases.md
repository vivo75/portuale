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
