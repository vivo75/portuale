# crates/portage-repo/src/resolve/pass/mod.rs
One resolver pass (BFS walk) over the dependency graph.

## fn run_pass
- Phase A3 (backlog #023): `PassState` holds `use_overlay` / `use_change_overlay` / `use_broke`, read back overlay-then-`bp` within the pass.
- The same effective-config view the loop feeds `collect_feedback` + driver / `assemble_result` (cloned out of `bp`; only `&bp` is borrowed in here, so no `config_owned` dance is needed).
- `union_constraints` is pass-constant (`bp` is never mutated mid-walk, only the overlay): C1 readers see positives + rendered negatives as one bucket.
- First-pass `--debug` prints each argument as `Arg:` / `Atom:` lines via `resolver_trace::tr!`.

