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

## Reports (copied 2026-09-28 from `.superpowers/sdd/batch-2026-09-27/`)

Copied verbatim. These are the reports cited by name in
`backlog-tasks.md` and in the four 09-28 batch plans, and `g215`, which
the plans also name. The source directory holds more material (briefs,
run logs, diffs, reviews, and the reports of items nobody cites), which
stays untracked. The reports are implementer write-ups: treat them as
background, not as evidence. The raw captures they quote are the
evidence where one was kept.

| Report | Title |
|---|---|
| [`g195-report.md`](g195-report.md) | Track G4, #195: USE-change suggestion + argument-order display (the six `aub0` orders, round 1) |
| [`g198-report.md`](g198-report.md) | Tracks G5 (#198) + G6 (#205): autounmask paths that never fall back to an older version |
| [`g201-report.md`](g201-report.md) | Track G3, #201: backtracking-conflict WARNING where real "resolves silently" |
| [`g207-report.md`](g207-report.md) | #207 then #208 S0: the `cyc0*` circular-dependency cases |
| [`g209-report.md`](g209-report.md) | Track G10, #209: `--backtrack=0` must not skip the reverse-dependency feed loop |
| [`g210-report.md`](g210-report.md) | #210: slot/sub-slot change without revbump as version change |
| [`g211-report.md`](g211-report.md) | #211: slot-operator update probe; §5 "G13b" is the finding #236 stands on |
| [`g213-report.md`](g213-report.md) | #213: `prune_rebuilds` |
| [`g215-report.md`](g215-report.md) | #215: the in-walk unsatisfied probe |
| [`g216-report.md`](g216-report.md) | #216: the `\|\|` pick that keeps in-graph `>=dev-lang/go` live |
| [`g221b-report.md`](g221b-report.md) | #221 round 2 (reconcile with #216, review fixes) |
| [`g243-report.md`](g243-report.md) | #243: the `slot_operator_mask_built` probe and ebuild visibility |
| [`n196-report.md`](n196-report.md) | #196: the GLEP 42 news-count notice |
| [`n230-report.md`](n230-report.md) | Track N17, #230: skipped-update block rendered like real |
| [`r25c-report.md`](r25c-report.md) | #25 S1c gate + S2/S3 |

Still open under 09-28 H0 step 3: repoint each citing backlog entry at
its tracked copy here.
