# Final review — `2026-10-19-open-backlog-plan-425-429.md` (closed 2026-10-20)

Same branch and beds as `2026-10-19-open-backlog-plan.final-review.md`.

| # | Result | portuale | pmtest pin |
|---|---|---|---|
| 425 | DONE: `--resume` resolution notice and banner | `94b721ac`, `01c5c03f` (quiet gate on the fetch-only arm) | `bb71288` |
| 426 | DONE: failed-build block, keep-going re-plan, failure list | `1610e70b`, `33d635dc` | `7c8b0e8` |
| 427 | DONE: no `Elog messages ... written to` line | `d54ac7b7` | in the elog pins |
| 428 | DONE: `FETCHCOMMAND`, shared download dir, full digest names | `f2bc90cd` | `5ca30e6` |
| 429 | DONE: pubgrub autounmask flips and parent flip | `2fd268e2`, bed fix `f8961146` | `e15a1ed` |
| 352 | DONE: running-root `IDEPEND` merge (bed-only opt-in) | `330dbcf4` | `766f894` |

Residues: #434 (pubgrub partial list on abort, allowlisted `pubgrub-abort-partial-list`), #435, #436, #437, #438, #439, #440. The `--quiet` gaps on the re-plan and the verify line are #437e and #436c.
