# crates/portage-repo/src/resolve/resolver.rs
The resolver trait and solver selection.

## enum SolverKind
- Portuale-only (`--solver=`): Portage's `emerge` has no such flag. `Portage` is the default backtracking walk Portage runs; `PubGrub`/`Resolvo` drive lu-zero's `portage-atom-pubgrub` / `portage-atom-resolvo` bridges (see `solver_bridge.rs`) and are Rust-only.

## fn SolverKind::parse
- `None` for anything outside the three accepted values.

## trait Resolver
- Default implementation is `BacktrackingResolver`; alternates plug in wholesale via `active_resolver_for` without touching a call site.

## struct BacktrackingResolver
- A single-pass BFS graph walk wrapped in Portage's `_emerge/resolver/backtracking.py` retry loop.

## fn active_resolver
- Hook for runtime selection between resolver implementations.

## fn active_resolver_for
- Backtracking walk for `Portage`, lu-zero's PubGrub / resolvo bridges for the other two. The parked engines ignore `--onlydeps` and its `--onlydeps-with-rdeps` / `--onlydeps-with-ideps` root filtering (like `--nodeps` / `--buildpkgonly`).
