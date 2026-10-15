# Task #34 S5b round 2 re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `188ecb57335d243dd8f323622d342dad061f8a59`, dirty
(`M Cargo.lock`,
`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/Cargo.toml`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s5/g2/`,
`?? LLM/evidence/2026-10-15-34-s5/g2r2/`,
`?? LLM/evidence/2026-10-15-34-s5/modes-b.md`; S5b round 2 change in place:
G2 post-solve; binary below already contains it).
pmtest HEAD: `2492d4b2e99bc5d7af351fb882cf12335d125bf8`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-15 07:18 UTC (before the runs;
already contains the S5b round 2 change; the runner's own rebuild said
`Finished release profile [optimized] in 0.09s/0.10s` — a no-op — in every run;
no cargo build/test was run during the bed runs).
Image: `localhost/test-portuale:latest` (`5616d41d00cd`), Portage 3.0.82.2.

Each pubgrub run (from `/home/vivo/repo/PORTUALE/pmtest`, sequentially):
`export GENTOO_MIRRORS=http://10.48.0.229:8080`
`L0_SKIP_MULTI=1 L0_SKIP_INVARIANTS=1 L0_PORTUALE_OPTS=--solver=pubgrub`
`L0_EMERGE_OPTS="<mode> --getbinpkg=n --usepkg=n" differential-test-bed/run/l0-resolver.sh`
with `<mode>` = `-pv`, `-pvu`, `-pvuD`, `-pvuDN`, `-pve` in that order.
Default run: same env except `L0_PORTUALE_OPTS` unset and
`L0_EMERGE_OPTS="-pvuDN --getbinpkg=n --usepkg=n"`.
Counts:
`python3 differential-test-bed/compare/resolve-compare.py --count-categories missing,extra,version,action,exit <rundir> differential-test-bed/compare/known-divergences.yaml > <rundir>/counts.txt`
(runner exit 1 in all 6 runs — the final comparator exiting nonzero is expected, not a
failure; the count command also rewrites `<rundir>/l0-report.txt`/`.json` with the action
split; count exit 1 in all 6 runs for the same reason.)

Environment note: same gentoo tree `11c58b7a`, same buildovl `3b1df681`, same porttest
`b4b5d17a`, same VDB snapshot (320 installed packages), same image as the S5a runs
(headers of all six round-2 `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S5b-r2 run dir | S5b-r2 missing | S5b-r2 extra | S5b-r2 version | S5b-r2 action | S5b-r2 exit | S5a missing | S5a extra | S5a version | S5a action | S5a exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T072339Z/` | 0 | 5 | 0 | 0 | 30 | 23 | 5 | 0 | 0 | 30 | -23 | 0 | 0 | 0 | 0 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T073029Z/` | 0 | 5 | 0 | 0 | 30 | 23 | 5 | 0 | 0 | 30 | -23 | 0 | 0 | 0 | 0 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T073639Z/` | 0 | 4 | 0 | 0 | 30 | 23 | 4 | 0 | 0 | 30 | -23 | 0 | 0 | 0 | 0 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T074542Z/` | 0 | 4 | 0 | 0 | 30 | 0 | 4 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |
| `-pve` | `differential-test-bed/logs/l0-20261015T075540Z/` | 0 | 10 | 0 | 0 | 30 | 0 | 10 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | 0 |

(S5a totals and S5a run dirs per mode are quoted from the S5a modes-a.md:
S5a run dirs `-pv` `l0-20261015T040457Z/`, `-pvu` `l0-20261015T041110Z/`,
`-pvuD` `l0-20261015T041711Z/`, `-pvuDN` `l0-20261015T042557Z/`, `-pve`
`l0-20261015T043546Z/`. The operative delta in this round-2 re-measure is S5b-r2 vs S5a.)

Notes: no probe went UP in any mode (0 up in all five; see per-mode sections).
`-pv`/`-pvu`: all 23 remaining S5a counted `missing` are gone; the remaining counted
findings are exactly `extra=5` (`kde-apps_kdecore-meta` 4, `sys-apps_systemd-utils` 1)
plus unchanged `exit=30`.
`-pvuD`: all 23 remaining S5a counted `missing` are gone; the only remaining counted
findings are `kde-apps_kdecore-meta` `extra=4` plus unchanged `exit=30`.
`-pvuDN` and `-pve` are per-probe identical to S5a (0 up, 0 down).
Context (not counted): S5b-r2 clean probes 57/60/64/12/81
(S5a: 57/60/64/12/81 — identical in every mode).
explained findings 11/11/11/8/8 (identical to S5a in every mode).

## Probes whose counted totals went UP versus S5a (name, S5a total, S5b-r2 total)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S5a per-probe totals from
`counts-pv.txt`, `counts-pvu.txt`, `counts-pvuD.txt`, `counts-pvuDN.txt`,
`counts-pve.txt` in this directory (copies of each S5a run dir's `counts.txt`).

### -pv (`differential-test-bed/logs/l0-20261015T072339Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T040457Z/`).

6 probes went DOWN vs S5a (context, not required) — together exactly the missing
delta 23 − 0:

| probe | S5a total | S5b-r2 total | S5b-r2 breakdown |
|---|---|---|---|
| `dev-lang_rust` | 4 | 0 | — (no counted findings) |
| `kde-apps_kdecore-meta` | 11 | 4 | extra=4 |
| `kde-frameworks_kcoreaddons` | 5 | 0 | — (no counted findings) |
| `media-video_vlc` | 5 | 0 | — (no counted findings) |
| `net-misc_networkmanager` | 1 | 0 | — (no counted findings) |
| `sys-auth_polkit` | 1 | 0 | — (no counted findings) |

(S5a `kde-apps_kdecore-meta` was `missing=7 extra=4`; the `missing=7` is gone.)

### -pvu (`differential-test-bed/logs/l0-20261015T073029Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T041110Z/`).

6 probes went DOWN vs S5a (context, not required) — together exactly the missing
delta 23 − 0:

| probe | S5a total | S5b-r2 total | S5b-r2 breakdown |
|---|---|---|---|
| `dev-lang_rust` | 4 | 0 | — (no counted findings) |
| `kde-apps_kdecore-meta` | 11 | 4 | extra=4 |
| `kde-frameworks_kcoreaddons` | 5 | 0 | — (no counted findings) |
| `media-video_vlc` | 5 | 0 | — (no counted findings) |
| `net-misc_networkmanager` | 1 | 0 | — (no counted findings) |
| `sys-auth_polkit` | 1 | 0 | — (no counted findings) |

### -pvuD (`differential-test-bed/logs/l0-20261015T073639Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T041711Z/`).

6 probes went DOWN vs S5a (context, not required) — together exactly the missing
delta 23 − 0:

| probe | S5a total | S5b-r2 total | S5b-r2 breakdown |
|---|---|---|---|
| `dev-lang_rust` | 4 | 0 | — (no counted findings) |
| `kde-apps_kdecore-meta` | 11 | 4 | extra=4 |
| `kde-frameworks_kcoreaddons` | 5 | 0 | — (no counted findings) |
| `media-video_vlc` | 5 | 0 | — (no counted findings) |
| `net-misc_networkmanager` | 1 | 0 | — (no counted findings) |
| `sys-auth_polkit` | 1 | 0 | — (no counted findings) |

### -pvuDN (`differential-test-bed/logs/l0-20261015T074542Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T042557Z/`).
No probe went DOWN vs S5a — per-probe counted totals are identical to S5a.

### -pve (`differential-test-bed/logs/l0-20261015T075540Z/`)

No probe went UP vs S5a (S5a run dir `l0-20261015T043546Z/`).
No probe went DOWN vs S5a — per-probe counted totals are identical to S5a.

## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T080751Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S5a default run (`l0-20261015T044752Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S5a).

No probe's counted total changed vs the S5a default run (0 up, 0 down).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S5a and S5b-r2 default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-15T04:47:53Z` vs `2026-10-15T08:07:52Z`).
The real (Portage) side differs in 0 files when the
`Dependency resolution took ...s (backtrack: ...)` timing line is excluded (0/117
with a non-timing diff). `vdb-list.txt` (320 packages) and `world.txt` are
byte-identical between the two default runs.
