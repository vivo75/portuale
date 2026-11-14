# Evidence for #747 — delete the dead allowlist entry

Backlog #747: `l0-737-openssl-world-late-reinstall-order` (in
`../pmtest/differential-test-bed/compare/known-divergences.yaml`) matched
nothing. Both runs below are default `differential-test-bed/run/l0-resolver.sh`
on branch `backlog/747-748-bed-divergences`, binary `409a4698`-based release
build; full reports stay in the cited bed log dirs, excerpts only here.

## Before (entry still present): `l0-20261114T093531Z`

Full report:
`../pmtest/differential-test-bed/logs/l0-20261114T093531Z/l0-report.txt`
(scope log `/tmp/opencode/p747/l0-before.log`; the scope ended after the
probes, so the script's own compare step was run manually on the finished
probe dir — same `resolve-compare.py` invocation, rc=0).

- `probes: 120, clean: 91, parity_rate: 0.758, UNEXPLAINED: 0`
- `## allowlist entries that matched nothing (candidates for removal)` lists:
  `perl-cluster-hash-order-601`, `l0-perl-co-upgrade-698-file-temp-use`,
  **`l0-737-openssl-world-late-reinstall-order`**
- `_world` cell: one `order` finding, attributed to
  `l0-order-only-health-clean-558` (`merge order diverges at #46: real
  sys-apps/shadow vs portuale sys-apps/util-linux`) — never to the #737 entry.
- `check-invariants.py`: 120 probes, 0 portuale violations, 0 control violations.

## The deletion (pmtest side)

`known-divergences.yaml`: removed the whole `#737` block (comment + entry,
33 lines). `pytests-contract-suite/test_health_checker.py:368-375` docstring
reworded to past tense (`formerly allowlisted as …; removed as unused by #747`).

## After (entry deleted): `l0-20261114T094245Z`

Full report:
`../pmtest/differential-test-bed/logs/l0-20261114T094245Z/l0-report.txt`
(scope log `/tmp/opencode/p747/l0-after.log`, full in-scope run, rc=0).

- `probes: 120, clean: 91, parity_rate: 0.758, UNEXPLAINED: 0`
- `## allowlist entries that matched nothing` now lists only the other two ids;
  `grep -c l0-737-openssl-world-late-reinstall-order l0-report.txt` = 0
  (the id appears nowhere — neither as explained nor as unused).
- `_world` cell unchanged: the same single `order` finding still attributed to
  `l0-order-only-health-clean-558`.

## What this proves (and does not)

The entry was dead and is gone; both beds grade `UNEXPLAINED: 0` with byte-
identical finding sets apart from the unused list. Honestly: this tightens
nothing — a returning `_world` order shape would still be explained, by
`l0-order-only-health-clean-558`. #747 removes a dead duplicate.
