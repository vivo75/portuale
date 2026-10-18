# S9 (347-GB, 347-PX) report — 2026-10-18

The worker ended its turn while waiting for `cargo test` and never wrote a
report; the coordinator ran the remaining gates and reviewed the diff.

- Branch `backlog/346-rest` (both repos). Oracle: Portage 3.0.82.2 G cells
  in `LLM/evidence/2026-10-15-346-s0/g/`. Q7 = yes (per-binrepo keys and
  conflict warnings included).
- Gates: `cargo fmt --check` clean; clippy 0 warnings; `cargo test --release`
  2802 passed, 0 failed; pmtest contract suite 4043 passed, 37 skipped,
  2 xfailed, 0 failed, no corpus drift (nothing blessed).
- One earlier full pmtest run showed 14 failures (depclean/prune/help/
  slot-operator pins) that all passed in isolation and in a clean second
  full run; not reproducible, probably the leftover worker run overlapping.
  Noted, not explained.
- No existing pin moved. The bed (L0) was not run for this slice.

## Bed (added by coordinator, 2026-10-18, after Z)

`differential-test-bed/run/l0-resolver.sh` on `backlog/346-rest` (S1–S9
merged with main incl. #369): 31 UNEXPLAINED, 6 explained, 0 invariant
violations (rc=1 as for the previous run). The finding lines are identical
to the previous bed run `l0-20261017T205457Z` (same 31). Report
`pmtest/differential-test-bed/logs/l0-20261018T033226Z/`. L1/L2/L3 and the
fixture oracle were not run.
