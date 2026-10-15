# Task #34 S5e re-measure — 5 modes × corpus, `--solver=pubgrub`, plus one DEFAULT-solver run

PM HEAD (portuale): `5e0cc0c962bf42767c058b1d32bdc2fe8e3d93de`, dirty
(`M LLM/code-notes/portage-repo/solver_bridge.md`,
`M crates/portage-repo/src/solver_bridge.rs`,
`?? LLM/evidence/2026-10-15-34-s5/g5/`,
`?? LLM/evidence/2026-10-15-34-s5/g6/`; HEAD is the committed G3+G7 slice,
the dirty `solver_bridge.rs` diff on top is uncommitted).
pmtest HEAD during the runs: `023d4408b2bdc762a02616a03de38dd14c7c1ca4`
(`M pytests-contract-suite/test_emerge_pretend_contract.py`, untouched by this task).
pmtest HEAD moved to `7a07bd2793bbf4acf8c84314e14502b791ee68e2` (clean) at
12:38:13Z, after the chain finished (12:35:07Z) — a concurrent commit adding
72 lines to `differential-test-bed/compare/known-divergences.yaml`.
That edit's file mtime is 12:37:59Z, after the last count step, and
`l0-resolver.sh`/`resolve-compare.py` are untouched since Oct 14, so all six
runs and all six count steps used the same scripts and the same allowlist
as the S5c runs.

Binary: `portuale/target/release/portuale` (the runner rebuilds the PM itself;
every run reports `PM under test: portuale 5e0cc0c9-dirty` and
`Finished release profile [optimized] in 0.10s` — a no-op — so the binary
already contained this rev; no cargo build/test was run during the bed runs).
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
failure; count exit 1 in all 6 runs for the same reason.)

Environment note: same gentoo tree `11c58b7a`, same buildovl `3b1df681`, same porttest
`b4b5d17a`, same VDB snapshot (320 installed packages), same image as the S5c runs
(headers of all six S5e `counts.txt` confirm the tree/overlay hashes).

## Totals per pubgrub mode (unexplained only, 117 probes each)

| mode | S5e run dir | S5e missing | S5e extra | S5e version | S5e action | S5e exit | S5c missing | S5c extra | S5c version | S5c action | S5c exit | delta missing | delta extra | delta version | delta action | delta exit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `-pv` | `differential-test-bed/logs/l0-20261015T113635Z/` | 0 | 0 | 0 | 0 | 26 | 0 | 4 | 0 | 0 | 26 | 0 | -4 | 0 | 0 | 0 |
| `-pvu` | `differential-test-bed/logs/l0-20261015T114608Z/` | 0 | 0 | 0 | 0 | 26 | 0 | 4 | 0 | 0 | 26 | 0 | -4 | 0 | 0 | 0 |
| `-pvuD` | `differential-test-bed/logs/l0-20261015T115231Z/` | 0 | 0 | 0 | 0 | 26 | 0 | 4 | 0 | 0 | 26 | 0 | -4 | 0 | 0 | 0 |
| `-pvuDN` | `differential-test-bed/logs/l0-20261015T120249Z/` | 0 | 0 | 0 | 0 | 26 | 0 | 4 | 0 | 0 | 26 | 0 | -4 | 0 | 0 | 0 |
| `-pve` | `differential-test-bed/logs/l0-20261015T121422Z/` | 2 | 0 | 0 | 0 | 26 | 0 | 10 | 0 | 0 | 26 | +2 | -10 | 0 | 0 | 0 |

(S5c totals and S5c run dirs per mode are quoted from the S5c modes-c.md:
S5c run dirs `-pv` `l0-20261015T091741Z/`, `-pvu` `l0-20261015T092431Z/`,
`-pvuD` `l0-20261015T093045Z/`, `-pvuDN` `l0-20261015T093942Z/`, `-pve`
`l0-20261015T094939Z/`. The operative delta in this S5e re-measure is S5e vs S5c.)

Notes:
`-pv`/`-pvu`/`-pvuD`/`-pvuDN`: the only change is the `extra` drop
`kde-apps_kdecore-meta` `extra=4` → 0 (the 4 extras are gone, not reclassified:
explained findings are identical to S5c at 11 in all four modes, and the S5e
`counts.txt` files contain zero `[extra]` findings anywhere). The remaining
counted findings in each of the four modes are exactly `exit=26`
(13 probes × `exit=2`, the same probe set as S5c). `kde-apps_kdecore-meta`
itself is still not clean (retains a non-counted `[order]` finding).
`-pve`: `extra` 10 → 0 (`kde-apps_kdecore-meta` 9 → 0, `app-editors_emacs`
1 → 0; emacs has no findings section at all in the S5e run) but `missing`
0 → 2 — one probe, `virtual_pager`, went UP (see below). Remaining counted
findings are `virtual_pager` `missing=2` plus `exit=26` (same 13 probes).
Context (not counted): S5e clean probes 60/63/66/14/84
(S5c: 60/63/66/14/83 — `-pve` +1: kdecore and emacs became clean, pager left it).
explained findings 11/11/11/8/8 (identical to S5c in every mode).

## Probes whose counted totals went UP versus S5c (name, before, after)

Per-probe totals are the sums of the five counted categories from the
`## counts by probe` section of each run's `counts.txt`; S5c per-probe totals from
the S5c run dirs' `counts.txt` (still present in `differential-test-bed/logs/`).

### -pv (`differential-test-bed/logs/l0-20261015T113635Z/`)

No probe went UP vs S5c (S5c run dir `l0-20261015T091741Z/`).

1 probe went DOWN vs S5c (context, not required):

| probe | S5c total | S5e total | S5e breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 4 | 0 | — (no counted findings; retains non-counted `[order]`) |

### -pvu (`differential-test-bed/logs/l0-20261015T114608Z/`)

No probe went UP vs S5c (S5c run dir `l0-20261015T092431Z/`).

1 probe went DOWN vs S5c (context, not required) — same one as `-pv`:

| probe | S5c total | S5e total | S5e breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 4 | 0 | — (no counted findings; retains non-counted `[order]`) |

### -pvuD (`differential-test-bed/logs/l0-20261015T115231Z/`)

No probe went UP vs S5c (S5c run dir `l0-20261015T093045Z/`).

1 probe went DOWN vs S5c (context, not required) — same one as `-pv`:

| probe | S5c total | S5e total | S5e breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 4 | 0 | — (no counted findings; retains non-counted `[order]`) |

### -pvuDN (`differential-test-bed/logs/l0-20261015T120249Z/`)

No probe went UP vs S5c (S5c run dir `l0-20261015T093942Z/`).

1 probe went DOWN vs S5c (context, not required) — same one as `-pv`:

| probe | S5c total | S5e total | S5e breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 4 | 0 | — (no counted findings; retains non-counted `[order]`) |

### -pve (`differential-test-bed/logs/l0-20261015T121422Z/`)

1 probe went UP vs S5c (S5c run dir `l0-20261015T094939Z/`):

| probe | S5c total | S5e total | S5e breakdown |
|---|---|---|---|
| `virtual_pager` | 0 | 2 | missing=2 |

(S5c `virtual_pager` had only a non-counted `[order]` finding —
`merge order diverges at #188: real virtual/pager vs portuale dev-libs/libuv` —
so its counted total was 0.)

2 probes went DOWN vs S5c (context, not required):

| probe | S5c total | S5e total | S5e breakdown |
|---|---|---|---|
| `kde-apps_kdecore-meta` | 9 | 0 | — (no counted findings; retains non-counted `[order]` at #114) |
| `app-editors_emacs` | 1 | 0 | — (no findings section at all; clean) |

#### Unexplained cell: `-pve virtual_pager` (missing=2)

Atom: `virtual/pager`.
Real argv: `/usr/sbin/emerge -pve --getbinpkg=n --usepkg=n --color=n virtual/pager`
(`in-container.sh`: `timeout 600 "$PORTAGE_EMERGE" $EOPTS --color=n "$atom"`),
real rc=0.
Portuale argv: `/usr/local/bin/emerge -pve --getbinpkg=n --usepkg=n --solver=pubgrub --color=n virtual/pager`
(`in-container.sh`: `timeout 600 "$PTL" $EOPTS $POPTS --color=n "$atom"`),
portuale rc=0.
Diff hunk (real `real/virtual_pager.txt` vs portuale `portuale/virtual_pager.txt`
in `differential-test-bed/logs/l0-20261015T121422Z/`; quoted findings from
`counts.txt` lines 122–123):

```diff
--- real/virtual_pager.txt (Total: 324 packages — 1 downgrade, 43 new, 280 reinstalls)
+++ portuale/virtual_pager.txt (Total: 322 packages — 1 downgrade, 43 new, 278 reinstalls)
@@ real line 15 (present for real, absent for portuale) @@
 [ebuild   R    ] app-misc/editor-wrapper-4-r1::gentoo  0 KiB
@@ real line 190 (present for real, absent for portuale) @@
 [ebuild   R    ] sys-apps/less-704::gentoo  USE="pcre -test -verify-sig" 974 KiB
```

i.e. `[missing] ebuild app-misc/editor-wrapper-4-r1 present for real, absent
for portuale` and `[missing] ebuild sys-apps/less-704 present for real, absent
for portuale. (Same cell also carries non-counted `[order] merge order
diverges at #6: real dev-libs/xxhash vs portuale
sec-keys/openpgp-keys-gentoo-release` and `[totals] real Total 324 vs portuale
Total 322`.) No diagnosis — for the oracle/review agents.

## DEFAULT-solver run (`-pvuDN --getbinpkg=n --usepkg=n`, no `L0_PORTUALE_OPTS`)

Run dir: `differential-test-bed/logs/l0-20261015T122818Z/`
Totals (unexplained only, 117 probes): missing 0, extra 448, version 0, action 0,
exit 0 — equal to the S5c default run (`l0-20261015T100150Z`: 0/448/0/0/0).
Context (not counted): clean probes 14, explained findings 0 (same as S5c).

No probe's counted total changed vs the S5c default run (0 up, 0 down;
per-probe counted totals are identical probe-for-probe:
`_dev-libs_libevent-2.1.12`, `app-alternatives_awk`, `app-containers_runc`,
`dev-build_make`, `dev-build_ninja`, `dev-libs_openssl`, `sys-apps_coreutils`,
`virtual_editor`, each `extra=56`).

Per-probe fingerprints of the portuale side: `diff -rq` of the `portuale/` output
directories between the S5c and S5e default runs reports no differences — the
portuale side is byte-identical file-for-file (117/117). `fingerprint.tsv` differs
only in the `date_utc` line (`2026-10-15T10:01:51Z` vs `2026-10-15T12:28:19Z`).
The real (Portage) side differs in 0 files when the
`Dependency resolution took ...s (backtrack: ...)` timing line is excluded (0/117
with a non-timing diff). `vdb-list.txt` (320 packages) and `world.txt` are
byte-identical between the two default runs.

---
## Controller notes (added when committing)

## The one new finding: `virtual/pager` under `-pve` (missing 2)

Portage: `app-misc/editor-wrapper-4-r1` and `sys-apps/less-704` (`R`, from `virtual/pager`
`|| ( sys-apps/less sys-apps/most sys-apps/util-linux[ncurses] ... )`).
portuale: picks `sys-apps/util-linux[ncurses]`, which another parent (systemd) also pulls.
Cause: Portage prefers an alternative only if it is already in the graph at the moment
the `||` is processed (depth-first order); `util-linux` enters the graph after
`virtual/pager`, so Portage takes the first-listed `less`. The bridge fixpoint treats any
independently selected package as "in the graph", so it is order-blind. Net effect of S5e:
-14 extras (4+4+4+4+10 -> 0) against +2 missing in one probe. Filed as #366
(`owner: portuale-bug`), not fixed here: a faithful fix needs Portage's processing order.

## After the allowlist (pmtest `known-divergences.yaml`, layer `l0-pubgrub`)

Re-graded the same run dirs: missing/extra/version/action/exit = 0/0/0/0/0 in all five
modes. 13 exit probes -> #363 (9), #364 (3), #365 (1); `virtual_pager` -> #366.
