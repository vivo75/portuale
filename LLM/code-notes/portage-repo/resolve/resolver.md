# crates/portage-repo/src/resolve/resolver.rs
The resolver trait and solver selection.

## enum SolverKind
- Portuale-only (`--solver=`): Portage's `emerge` has no such flag. `Portage` is the default backtracking walk Portage runs; `PubGrub` drives lu-zero's `portage-atom-pubgrub` bridge (see `solver_bridge.rs`) and is Rust-only. (Until #755 a `Resolvo` variant drove lu-zero's `portage-atom-resolvo` bridge; both the variant and the backend are deleted.)

## fn SolverKind::parse
- `None` for anything outside the two accepted values.

## trait Resolver
- Default implementation is `BacktrackingResolver`; alternates plug in wholesale via `active_resolver_for` without touching a call site.

## struct BacktrackingResolver
- A single-pass BFS graph walk wrapped in Portage's `_emerge/resolver/backtracking.py` retry loop.

## fn active_resolver
- Hook for runtime selection between resolver implementations.

## fn active_resolver_for
- Backtracking walk for `Portage`, lu-zero's PubGrub bridge for `PubGrub`. The parked engine ignores `--onlydeps` and its `--onlydeps-with-rdeps` / `--onlydeps-with-ideps` root filtering (like `--nodeps` / `--buildpkgonly`). (Until #755 a resolvo bridge served `Resolvo`; deleted.)
