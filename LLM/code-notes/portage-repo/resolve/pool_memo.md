# crates/portage-repo/src/resolve/pool_memo.rs
Binary-candidate pool memo for the passes of one resolve (backlog #441 S1 / #457).

## fn collect_memoized
- `collect_binary_candidates` is append-only on `candidates` and pure in its inputs plus resolve-fixed state (config, repos, local and remote binary indexes, VDB under the freeze scope). 22 passes re-ask the same ~100k questions; 90% were repeats. Measured 2026-10-21: resolve_once 21.1 s -> 18.25 s.
- Rows are matched by value on every input (flags, atom, atom_str, root, extra constraints, usepkg include/exclude, and the ebuild candidates the pool started with), so nothing can be stale on inputs.
- The fixed state is guarded by identity: `run_pass` enters a `PoolScope` only when `bp.backtrack_config` is `None` (the pass then uses `ctx.config`), and a call is served only when its config, repos slice and local index pointers are the ctx's own objects (alive for the whole scope, so no address reuse). An autounmask `backtrack_config` is a fresh clone and bypasses the memo.
- Key is `(category, package, atom_str)`; keying by cp alone thrashed the 8-row cap on packages asked with many atom variants (qtbase: 3812 misses).
- The memo lives on `ResolveCtx::pool_memo`, so a post-restart ctx starts empty.

## fn collect_memoized (third step, backlog #441 S1 / #457)
- The starting ebuild pool is compared by `Arc` identity, not deep `Vec<Candidate>` equality: the only producers (`resolve_pretend`, `greedy_pool`) hand over `list_candidates`' process-memoised allocation (or `None` for the by-construction empty pool), so the same question on a later pass carries the same pointer. `list_candidates` is keyed per repo set and cp with immutable trees, and the scope pins the repos, so identity implies identical input.
- The parsed-`Atom` and atom-string comparisons are gone: the bucket key already pins `(cp, atom_str)` and both producers parse with `parse_atom`, so equal strings imply equal atoms. The `added` slice still splits at the starting pool length (single clone now, not two).

## gate threading (#571)
- `CollectBinaryCandidatesArgs` carries the gate through the memo forward.
