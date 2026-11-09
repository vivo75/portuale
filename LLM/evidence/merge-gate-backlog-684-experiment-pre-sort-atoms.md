# Merge-path safety gate: branch backlog/684-experiment-pre-sort-atoms (#684)

Tree under test: `bbe04be9` (clean working tree). That commit moved the
linkage-map code (`needed_elf`) and the read side of the preserved-libraries
registry out of the binary into `portage-repo`; the merge and unmerge paths
(`ebuild_merge/plib_registry.rs`, `plib_unmerge.rs`) now call the library.

The bed builds this tree's binary itself (`ensure_pm_built`). From pmtest:

```
differential-test-bed/run/l1-merge-from-binpkg.sh \
  differential-test-bed/atomlists/l1-merge-gate.txt          # builds the pair
L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 \
  differential-test-bed/run/l1-merge-from-binpkg.sh \
  differential-test-bed/atomlists/l1-merge-gate.txt          # re-merge over the running pair
L1_PORTUALE_VDB=redb L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 \
  differential-test-bed/run/l1-merge-from-binpkg.sh \
  differential-test-bed/atomlists/l1-merge-gate.txt          # same, redb VDB backend
```

| Run | Report | rc |
|---|---|---|
| first merge, default VDB | `l1-20261108T235703Z` | 0 |
| re-merge over the running pair, default VDB | `l1-20261109T003049Z` | 0 |
| re-merge over the running pair, redb VDB | `l1-20261109T003250Z` | 0 |

In every run `sys-libs/glibc` (2.43-r4) and `app-shells/bash` (5.3_p20-r1)
were merged by both Portage and portuale (`merged_count 2`, `merge_rc 0` on
both sides):

- hard findings 0, explained 0, UNEXPLAINED 0, payload diffs 0
- mtime-only diffs 1420 (non-fatal)
- health of the installed state: 0 failures introduced by the merge
- the sqlite-default and redb re-merges gave identical counters

Logs are local-only under pmtest `differential-test-bed/logs/`.
Not run: the L0 resolver bed and the other L1 cells.
Verdict: PASSED.
