# R3 premise check (2026-10-27, §P)

The plan's premise ("on the G0 combination Portage backtracks 1/20 by masking
the parent; portuale does 0 restarts") is mostly wrong (`s0.md`):
- portuale already masks the parent of a masked dependency
  (`pass/unresolved_dep.rs:53-131`, `feedback/missing_dep.rs`); counts, rc and
  rows match Portage on the p/k/l/u/g cells.
- G0's `0/20` against Portage's `1/20` is R6's count bug (case 1: a failed pass
  with an empty stack is not counted), not a missing restart.
- The real gaps: T1 (a USE-only miss that comes first must win), T2 (the
  `--autounmask-backtrack=n` stop must also cover missing-dependency feedback),
  and G0's extra `p569prov:0` residual conflict block (assemble.rs:536-545,
  shown although the walk stopped).
R3 is amended accordingly in the plan. RED 7 (cross-parent, `r594qtop
--autounmask=n`) needs #609's LIFO order and stays out.
