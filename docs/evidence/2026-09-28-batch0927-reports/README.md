# batch-2026-09-27 sdd material, tracked copies

Backlog entries filed by batch 2026-09-27 cite "batch-2026-09-27 sdd"
reports. The reports themselves live in the home checkout's gitignored
`.superpowers/sdd/batch-2026-09-27/`, so a worktree pair or a fresh
clone cannot follow those pointers. This directory holds tracked copies
([`batch-2026-09-28.md`](../../batch-2026-09-28.md) H0 step 3).

## probe-logs/ (copied 2026-09-28 from the volatile `/tmp/opencode/`)

Copied verbatim. Nothing was edited.

| Path | Source | What it is |
|---|---|---|
| `probe-logs/g195/in-probe.sh` | `/tmp/opencode/g195/in-probe.sh` | The in-container driver for the g195 round-1 real probe: stages the fixtures with `layers/l0-fixture-oracle/stage.sh`, then runs `emerge --pretend --autounmask =dev-libs/aup0b-1` and the six `aub0` argument orders under `--pretend --autounmask-backtrack=y`, with `PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=$FX`. |
| `probe-logs/g195/real-probe.txt` | `/tmp/opencode/g195/real-probe.txt` | Real `emerge` output from that probe, one `### CELL` block per cell with its `rc`. `g195-report.md` round 1 ("Real's answer") quotes it and dates it to real 3.0.82.2 in `localhost/test-portuale:latest`. It is the capture behind [`batch-2026-09-28_244.md`](../../batch-2026-09-28_244.md) §4, "What real printed". In every order real prints the same `aub0d[-foo]` miss; only the orders `a c b` (backtrack 2/20) and `a b c` (backtrack 3/20) add a slot-conflict block, and the entry order in that block differs between the two. |
| `probe-logs/g216/probe.sh` | `/tmp/opencode/g216/probe.sh` | The g216 S0 probe driver. It stages the fixtures with `/TEST/layers/l0-fixture-oracle/stage.sh` and runs `emerge -p --color=n` on `app-misc/g216top` and `dev-lang/g216comp`, each once by default and once with `--backtrack=0`. |
| `probe-logs/g216/probe.log` | `/tmp/opencode/g216/probe.log` | Output of that probe (its first line records real `Portage 3.0.81.3`). This is the log cited by the header of pmtest's `l0-fixture-oracle-g216.txt`. It shows real's dual `g216comp` rows (bare + `to <ROOT>`) under default staging, and the `--backtrack=0` self-cycle abort with a `--tree` partial list (`Total: 4`). Background for [`batch-2026-09-28_242.md`](../../batch-2026-09-28_242.md) Slice C and #245. |

The `*-report.md` copies themselves are still to be added by 09-28 H0
step 3.
