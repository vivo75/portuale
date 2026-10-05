# #305 (feat#157) — S4.6 merge-path gate on sqlite

Run 2026-10-05 by the coordinator, portuale `2b331a18` (S6.4's native
portageq and the PORTUALE_* saved-environment filter included), pmtest
`109593c` (opt-in `L1_PORTUALE_VDB=sqlite` in the L1 consumer).

The gate had to wait for S6 (plan §3 order change): before the native
portageq, `has_version` in glibc's `pkg_preinst` would have read
`var/db/pkg`, not the database.

| Run | Command | Result |
|---|---|---|
| build | `l1-merge-from-binpkg.sh atomlists/l1-merge-gate.txt` | `l1-20261005T040917Z`: 0 hard / 0 unexplained, 7 mtime-only |
| re-merge, files | `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 …` | `l1-20261005T045121Z`: glibc-2.43-r2 + bash-5.3_p15 merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only (= baseline) |
| re-merge, **sqlite** | same + `L1_PORTUALE_VDB=sqlite` | `l1-20261005T045520Z`: merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only |

In the sqlite run the portuale side converted 320 entries into
`/var/lib/portage/vdb.sqlite`, merged through `mrg --vdb-backend=sqlite`
(database generation 14 afterwards, counter 461 → 463, nothing pending),
and exported back to `/var/db/pkg`; the diff against real portage's
own `--oneshot` merge of the same pair is clean.

## A regression the gate caught

The first files re-merge after S6.4 (`l1-20261005T044236Z`) reported 2
unexplained VDB findings: the glibc and bash `environment` files gained
`declare -x PORTUALE_BIN=…`, exported into phases by S6.4 and saved by
`__save_ebuild_env`. Fixed in `0823b2a6` (`PORTUALE_.*` added to
`__filter_readonly_variables`), confirmed by a temporary-ROOT merge with
and without the fix, then by the clean re-run above.

## Other S4 checks in the same run (`oc-305-s46`)

cargo test 2157 passed / 0 failed; pmtest 2248 passed / 37 skipped /
2 xfailed; L0 `l0-20261005T040143Z` 120 / 101 / 31 unexplained (= baseline,
so S6.2's split of the resolver's installed matcher is neutral).
