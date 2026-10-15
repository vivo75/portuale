# Task #34 S5c re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `0b38d6705fc53407ec67ab3822c323c01950b5d5`, dirty
(`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s5/g3/`,
`?? LLM/evidence/2026-10-15-34-s5/g7/`; S5c changes in place:
G3 and G7; binary below already contains them).
pmtest HEAD: `ff3729964e69dac1baea848624ca99880b492c8b`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).

Binary: `portuale/target/release/portuale`, built 2026-10-15 09:06 UTC (before the runs;
already contains the S5c change; the runner's own rebuild said
`Finished release profile [optimized] in 0.09s/0.10s` — a no-op — in every run
(registry version `0b38d670-dirty`); no cargo build/test was run during the bed runs).
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
`b4b5d17a`, same VDB snapshot (320 installed packages), same image as the S5b round-2 runs
(headers of all six S5c `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S5c run dir | S5c missing | S5c extra | S5c version | S5c action | S5c exit | S5b-r2 missing | S5b-r2 extra | S5b-r2 version | S5b-r2 action | S5b-r2 exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T091741Z/` | 0 | 4 | 0 | 0 | 26 | 0 | 5 | 0 | 0 | 30 | 0 | -1 | 0 | 0 | -4 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T092431Z/` | 0 | 4 | 0 | 0 | 26 | 0 | 5 | 0 | 0 | 30 | 0 | -1 | 0 | 0 | -4 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T093045Z/` | 0 | 4 | 0 | 0 | 26 | 0 | 4 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | -4 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T093942Z/` | 0 | 4 | 0 | 0 | 26 | 0 | 4 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | -4 |
| `-pve` | `differential-test-bed/logs/l0-20261015T094939Z/` | 0 | 10 | 0 | 0 | 26 | 0 | 10 | 0 | 0 | 30 | 0 | 0 | 0 | 0 | -4 |

(S5b-r2 totals and S5b-r2 run dirs per mode are quoted from the S5b modes-b2.md:
S5b-r2 run dirs `-pv` `l0-20261015T072339Z/`, `-pvu` `l0-20261015T073029Z/`,
`-pvuD` `l0-20261015T073639Z/`, `-pvuDN` `l0-20261015T074542Z/`, `-pve`
`l0-20261015T075540Z/`. The operative delta in this S5c re-measure is S5c vs S5b-r2.)

Notes: no probe went UP in any mode (0 up in all five; see per-mode sections).
`-pv`/`-pvu`: the extra drop is `sys-apps_systemd-utils` `extra=1` → 0; the exit
drop is `app-containers_runc` and `dev-lang_go` (`exit=2` → 0 each). Remaining counted
findings are exactly `extra=4` (`kde-apps_kdecore-meta` 4) plus `exit=26`
(13 probes × `exit=2`).
`-pvuD`/`-pvuDN`: only the exit drop (`app-containers_runc`, `dev-lang_go`,
`exit=2` → 0 each); remaining counted findings are `kde-apps_kdecore-meta`
`extra=4` plus `exit=26` (same 13 probes × `exit=2`).
`-pve`: only the exit drop (same two probes); remaining counted findings are
`kde-apps_kdecore-meta` `extra=9` and `app-editors_emacs` `extra=1` plus `exit=26`
(same 13 probes × `exit=2`).
Context (not counted): S5c clean probes 60/63/66/14/83
(S5b-r2: 57/60/64/12/81 — each mode up exactly by its down-probe count).
explained findings 11/11/11/8/8 (identical to S5b-r2 in every mode).

## Probes whose counted totals went UP versus S5b round 2 (name, before, after)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S5b-r2 per-probe totals from
the S5b-r2 run dirs' `counts.txt` (still present in `differential-test-bed/logs/`).

### -pv (`differential-test-bed/logs/l0-20261015T091741Z/`)

No probe went UP vs S5b-r2 (S5b-r2 run dir `l0-20261015T072339Z/`).

3 probes went DOWN vs S5b-r2 (context, not required) — together exactly the
extra delta (5 − 4) and the exit delta (30 − 26):

| probe | S5b-r2 total | S5c total | S5c breakdown |
|---|---|---|---|
| `app-containers_runc` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `dev-lang_go` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `sys-apps_systemd-utils` | 1 | 0 | — (no counted findings; was `extra=1`) |

### -pvu (`differential-test-bed/logs/l0-20261015T092431Z/`)

No probe went UP vs S5b-r2 (S5b-r2 run dir `l0-20261015T073029Z/`).

3 probes went DOWN vs S5b-r2 (context, not required) — same three as `-pv`:

| probe | S5b-r2 total | S5c total | S5c breakdown |
|---|---|---|---|
| `app-containers_runc` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `dev-lang_go` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `sys-apps_systemd-utils` | 1 | 0 | — (no counted findings; was `extra=1`) |

### -pvuD (`differential-test-bed/logs/l0-20261015T093045Z/`)

No probe went UP vs S5b-r2 (S5b-r2 run dir `l0-20261015T073639Z/`).

2 probes went DOWN vs S5b-r2 (context, not required) — together exactly the
exit delta (30 − 26):

| probe | S5b-r2 total | S5c total | S5c breakdown |
|---|---|---|---|
| `app-containers_runc` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `dev-lang_go` | 2 | 0 | — (no counted findings; was `exit=2`) |

### -pvuDN (`differential-test-bed/logs/l0-20261015T093942Z/`)

No probe went UP vs S5b-r2 (S5b-r2 run dir `l0-20261015T074542Z/`).

2 probes went DOWN vs S5b-r2 (context, not required) — same two as `-pvuD`:

| probe | S5b-r2 total | S5c total | S5c breakdown |
|---|---|---|---|
| `app-containers_runc` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `dev-lang_go` | 2 | 0 | — (no counted findings; was `exit=2`) |

### -pve (`differential-test-bed/logs/l0-20261015T094939Z/`)

No probe went UP vs S5b-r2 (S5b-r2 run dir `l0-20261015T075540Z/`).

2 probes went DOWN vs S5b-r2 (context, not required) — same two as `-pvuD`:

| probe | S5b-r2 total | S5c total | S5c breakdown |
|---|---|---|---|
| `app-containers_runc` | 2 | 0 | — (no counted findings; was `exit=2`) |
| `dev-lang_go` | 2 | 0 | — (no counted findings; was `exit=2`) |

## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T100150Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S5b round-2 default run (`l0-20261015T080751Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S5b-r2).

No probe's counted total changed vs the S5b-r2 default run (0 up, 0 down;
per-probe counted totals are identical probe-for-probe).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S5b-r2 and S5c default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-15T08:07:52Z` vs `2026-10-15T10:01:51Z`).
The real (Portage) side differs in 0 files when the
`Dependency resolution took ...s (backtrack: ...)` timing line is excluded (0/117
with a non-timing diff). `vdb-list.txt` (320 packages) and `world.txt` are
byte-identical between the two default runs.
