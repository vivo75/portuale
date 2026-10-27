# Task 10b, count list: before, after, Portage (2026-10-27)

Binaries: `before` = HEAD `1a4c3117` built in a worktree (`/var/tmp/claude-10b/base`, removed); `after` = HEAD plus the
uncommitted 10b slice. Driver: `run-counts.sh` (scratch roots under `/var/tmp/claude-10b`, copies of `pmtest/fixtures` with the
world atoms the pytest helper adds; `PORTUALE_BT_TRACE=1`). Raw traces: `before/` and `after/` (`*.err` is the trace,
`*.out` the stdout, `*.rc` the exit code); one-line summaries `before-summary.txt`, `after-summary.txt`.
Portage column: `t8/targets.md` "Full count list" for the 10a cells, host Portage 3.0.83 captures in `captures/` for the new cells
(`<cell>-portage.txt` beside `<cell>-portuale-before.txt` and `<cell>-portuale.txt`).

"printed" is what portuale prints (`backtrack: N/20`). On the cells that end in a dead end on an argument the printed count is
one short of Portage's tries (backlog #600), exactly as in `t10a/counts.md`; those cells did not move.

| cell | before | after | Portage | rows equal Portage after |
|---|---|---|---|---|
| `p563pull` | 2/20 | 2/20 | 2/20 | y |
| `p569pull` | 2/20 | 2/20 | 2/20 | y |
| **T1** `p589apull` | 4/20, `N pull-1` + skip block | **0/20**, `U prov-5.44` + `N pull-2` | 0/20 | y |
| **T2** `p589bpull` | 2/20, `N pull-1`, 2 skip rows | **4/20**, `N pull-1`, 1 skip row `p589bprov:0/5.42=` | 4/20, `N pull-1`, 1 skip row | y (fix round) |
| T3 `p589cpull` | 4/20 | 4/20 | 4/20 | y |
| `p569pull t8x` | 3/20 | 3/20 | 3/20 | y |
| `t8x` | 1/20 | 1/20 | 1/20 | y |
| keeper hard dependency `needer` | 3/20 | 3/20 | 4/20 | y (unchanged; #600) |
| k3 argument `=paired-2.0` | 2/20 | 2/20 | 3/20 | y (unchanged; #600) |
| igw plain `>=igw0x-2` / `<igw0x-1` | 2/20 | 2/20 | 3/20 | y (unchanged; #600) |
| cgp VER `>=cgp1x-2` / `<cgp1x-1` | 2/20 | 2/20 | 3/20 | y (unchanged; #600) |
| triangle `needer othermod`, both orders | 3/20 | 3/20 | 4/20 | y (unchanged; #600) |
| cgp USE `cgp0x` | 0/20 | 0/20 | 0/20 | y |
| `--update --deep p569pull` | rc 1 (abort) | rc 0, 0/20 | rc 0, 1/20, `r U prov`, `rR virt` | n: no rebuild row (#598) |
| `--update --deep p589apull` | rc 1 (abort) | rc 0, 0/20 | rc 0, 0/20 | **y** |
| `--update --deep p589bpull` | rc 1 (abort) | rc 0, 0/20 | rc 0, 1/20, `r U prov`, `rR virt` | n: no rebuild row (#598) |
| `--update --deep p589cpull` | rc 1 (abort) | rc 0, 0/20 | rc 0, 0/20 | **y** |
| `--deep p589apull` (new) | rc 1, spurious slot conflict | rc 0, 0/20, `U prov`, `N core`, `N pull-2` | rc 0, 0/20 | y |
| T1, the three `=n` switches (new) | 4/20, `N pull-1` | 0/20, `U prov` + `N pull-2` | 0/20 | y |
| T1, `=n` switches + `--complete-graph` (new) | 0/20, only `N pull-2` | 0/20, `U prov` + `N pull-2` | 0/20 | y |
| T1 `--deep =p589aprov-5.44`, `p589avirt` in world (new) | rc 1, 2/20 | rc 0, 0/20, `U prov-5.44` | rc 0, 0/20 | y |
| T2, the three `=n` switches (new) | 2/20, `N pull-1` | 0/20, `U prov` + `N pull-2` | 0/20 | y |
| T3, the three `=n` switches (new) | 4/20, `N pull-1` | 0/20, `U prov` + `N pull-2` | 0/20 | y |
| Q19 `p590vpull`, `p590vpar` in world (new) | 2/20, both `=5.42*` rows | 2/20, both `=5.42*` rows | 6/20, both `=5.42*` rows | rows y, count n |

Rows: every `[ebuild ...]` line and conflict block of the cells that did not move is byte-identical before and after
(`diff before/*.out after/*.out`, apart from the first-run "Performing Global Updates" notice of a fresh scratch root).

Notes:
- Predictions checked: T1 0/20, T2 4/20 with the single `p589bprov:0/5.42=` row, T3 4/20: all confirmed. T2 needed a fix round: a built `:S/SS=` atom that a PostMerge consumer cannot satisfy post-merge but an installed instance matches becomes a pin (D:8803-8834). Not applied when `_want_installed_pkg` is false and backtracking is allowed (D:7308-7330, D:8816-8832: not selective, not excluded, an argument atom matches the installed provider). An earlier version of the guard (skip with `--update`) was backwards and was replaced by this predicate; a version of the pin that rewrote an already-broken stripped constraint to the verbatim atom broke `test_oracle_209` and was narrowed. New cells with Portage captures (`captures/u-*`): `--update p569pull` 2/20, `p589apull` 0/20, `p589bpull` 4/20, `p589cpull` 4/20, rows equal Portage for all four.
- The Q19 cell: Portage takes 6 tries and portuale 2. The rows are equal. Why Portage needs 4 more tries here (T3 needs 4 in all) was not investigated.
- The three `--update --deep` cells that moved from abort to rc 0 are the R5 (#598) cells. Two of four now equal Portage; the two
  `:=` shapes (`p569pull`, `p589bpull`) lack Portage's `r U prov` + `rR virt` rebuild rows.
