# Mutation testing — `cargo-mutants` on `portage-repo` (backlog #52 §10)

Tool: `cargo-mutants` v27.1.0 (`cargo install cargo-mutants --locked`),
`--in-place` (the default sandbox copy breaks the crate's tests, which
resolve fixtures relative to the manifest: the unmutated baseline fails
with 233 failures and no mutant is tested). Cadence per the plan:
nightly/weekly, not per-commit.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo mutants -p portage-repo --file portage-repo/src/resolver_trace.rs --in-place --timeout 300
cargo mutants -p portage-repo --file portage-repo/src/solver_bridge.rs --in-place --timeout 300
```

| module | mutants | caught | missed | unviable | wall clock |
|---|---|---|---|---|---|
| `resolver_trace.rs` (361 lines) | 35 | 0 | **35** | 0 | 4 min |
| `solver_bridge.rs` (1625 lines) | 86 | 44 | **26** | 16 | 7 min |
| `merge_order.rs` (3199) | 456 | — | — | — | not run (≈40 min) |
| `lib.rs` (34407) | 2207 | — | — | — | not run (≈3 h) |

`resolver_trace.rs` is the `PORTUALE_MO_SEL`/trace instrumentation
module: 35/35 survivors simply means no unit test asserts its internal
detail (its "tests" are the L0 `MO_ORDER` traces and
`TEST/scripts/mo-trace/`). Expected, not a gap.

## Surviving mutants in `solver_bridge.rs` (26) and their triage

All 26 sit in the `--solver=pubgrub` / `--solver=resolvo` bridge:

- `graph_result_from_order` outcome/ordering mapping (lines 430, 440,
  467, 665, 711, 732) and `newest_installed` (390): 13 survivors from
  guards/`==`/`>`/`&&` flips.
- `BridgeRepo::versions_for` / `desired_use` (824, 864),
  `resolve_pubgrub` (933), `resolve_resolvo` (1042): 5 survivors.
- `cp_exists` (124), `LazyRepo::build` (201), `target_slot` (770, 771):
  8 survivors.

Triage (the plan's own order): §1/§5/§6/§7 all fail to reach these —
the contract suite drives the default resolver, the fixture oracle and
L0 do too, and upstream has no pubgrub/resolvo tests to translate. That
is consistent with backlogs #33 (resolvo cycle linearization) and #34
(pubgrub over-merge), which already record both backends as
non-functional on real targets. **Conclusion: the survivors are a real
test gap in a deliberately parked Tier-4 area, not an accident of
coverage — the follow-up is a Tier-4 slice that wires the backends to
real targets and adds bridge-level unit tests for the mapping
functions, at which point these mutants become the acceptance list.**
No bespoke unit test was written now: it would pin mapping behaviour the
parked backends do not yet guarantee end-to-end.

The default resolver's own survivors (if any) are in
`merge_order.rs`/`lib.rs`, which were not run inside this session's
budget; `cargo mutants -p portage-repo --file portage-repo/src/merge_order.rs
--in-place` is the next useful run (~40 min).
