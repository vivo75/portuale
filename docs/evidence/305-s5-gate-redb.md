# #305 (feat#157) — S5.6 merge-path gate on redb

Run 2026-10-05 by the coordinator in scope `oc-305-s56`, portuale
`8c78d252` (S6.3's parent pipe included), pmtest `1b57f48`
(`L1_PORTUALE_VDB=sqlite|redb`), cached glibc/bash binpkgs from the
S4.6 build run `l1-20261005T040917Z`.

| Re-merge (`L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`) | Run | Result |
|---|---|---|
| files | `l1-20261005T055426Z` | glibc-2.43-r2 + bash-5.3_p15 merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only |
| **redb** (`L1_PORTUALE_VDB=redb`) | `l1-20261005T055610Z` | merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only; database: 320 installed, generation 14, counter 463, nothing pending |
| sqlite (`L1_PORTUALE_VDB=sqlite`) | `l1-20261005T055800Z` | merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only; same database counters |

glibc's `pkg_preinst` calls `has_version` while `mrg` holds the redb file,
so the redb run also exercises the S6.3 parent pipe on a real package: a
failing pipe exits 4 and phase-helpers.sh would have aborted the merge.
