# #34 S0 summary (2026-10-14)

Plan: `LLM/04.34-pubgrub-parity.opus.md`, Task S0. Binary built from `8772601e` + the
`PORTUALE_PG_TRACE=1` trace. Image `localhost/test-portuale:latest` (`5616d41d00cd`),
all runs `--getbinpkg=n --usepkg=n`, `L0_PORTUALE_OPTS=--solver=pubgrub`.

Files: `modes.md` (totals per mode), `buckets.md` (bucket table, examples, sample
chains), `counts-<mode>.txt`; baseline captures in `../2026-10-11-34-baseline/`.

## Totals (unexplained, 117 probes per mode)

| mode | missing | extra | version | action | exit |
|---|---|---|---|---|---|
| -pv | 203 | 1679 | 0 | 0 | 34 |
| -pvu | 9 | 1698 | 0 | 0 | 34 |
| -pvuD | 82 | 1698 | 0 | 0 | 34 |
| -pvuDN | 2516 | 137 | 0 | 0 | 34 |
| -pve | 26078 | 137 | 0 | 0 | 16 |

`-pv` reproduces the plan's section 0.1 (nodejs 8 vs 48 rows).

## Diagnosis checks (step 6)

Confirmed with the trace:
- **D-target-all-slots**: `sys-devel/gcc`, `binutils`, `autoconf`, `bash` … are pushed as
  `pg-target <cp> slot=*`; the extra rows are the sibling slots (97 findings in every mode).
- **D-closure**: on host `sys-devel/gcc` the installed target enters the closure as
  `pg-closure sys-devel/gcc shallow` and is never expanded.
- **D-no-R**: `-pve` on nodejs: Portage 333 lines, mostly `R`; pubgrub 51, no `R`.
  24223 of the 26215 `-pve` findings are `R` rows Portage shows and pubgrub cannot.
- **D-installed-expanded / bdeps / config-USE**: 294/305 findings (`-pv`/`-pvu`) have an
  installed parent on the chain; with `-D`, 262 go through DEPEND/BDEPEND (bdeps) and 43
  through runtime classes.

## Limits of the bucket numbers (read before using them as a progress meter)

- `H-other` is still 1343–1893 per mode (about 70% of non-`-pve` findings). Cause: `pg-edge`
  lines are emitted for **every candidate version** the engine loads, not only the chosen
  one, so the shortest chain can run through a version that is not in the plan (sample 1 in
  `buckets.md`: nodejs → `nodejs-20.18.2` DEPEND …). The chains are a heuristic, not proof.
  S1 onward should add the chosen-version edges to the trace (or filter by the final plan)
  before the table is used to judge a slice.
- `H-closure` is 0 with the widened rule 2 (installed-parent chains win first); the
  closure effect is real on gcc but is not separable by this script.
- Not distinguishable by script, left at 0: `H-installed-only`, `H-switches`, `H-slot`,
  `H-or-choice`, `H-cycle`, `H-pdepend-idepend`.
- `action` and `version` are 0 in every mode because set differences land in
  `missing`/`extra` first.
