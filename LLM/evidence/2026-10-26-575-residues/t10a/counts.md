# Task 10a, count list: before, after, Portage (2026-10-27)

Binary: `target/release/portuale` before and after the 10a change (tree `2dc48dda` plus the uncommitted slice).
Driver: `run-counts.sh` (scratch roots under `/var/tmp/claude-10a`, copies of `pmtest/fixtures` with the
world atoms the pytest helper adds). Raw traces: `before/` and `after/` (`*.err` is `PORTUALE_BT_TRACE=1`).
Portage column: `t8/targets.md` "Full count list" (host Portage 3.0.83).

"after, printed" is what portuale prints (`Dependency resolution took (backtrack: N/20)`). "after, tries" is the
number of tries Portage's rule counts over the passes portuale ran (the unit tests'
`portage_tries`: a restart always counts, any other failed pass only while unexplored nodes remain).
The two differ by one on the cells that end in a dead end on an argument: the driver's exit count is
one short there (`driver.md`, "exit count"). Changing it moves the harvested corpus (see the report), so it
is not in this slice.

| cell | before | after, printed | after, tries | Portage | rows equal (Portage) |
|---|---|---|---|---|---|
| `p563pull` | 2/20 | 2/20 | 2 | 2/20 | y |
| `p569pull` | 2/20 | 2/20 | 2 | 2/20 | y |
| T1 `p589apull` | 2/20 | 4/20 | 4 | 0/20 (10b) | n (`N pull-1`, Portage `U prov-5.44` + `N pull-2`; planned intermediate) |
| T2 `p589bpull` | 2/20, two skip rows | 2/20, two skip rows | 2 | 4/20, one row (10b) | y rows, extra `=5.42*` row (planned intermediate) |
| T3 `p589cpull` | 2/20 | 4/20 | 4 | 4/20 | y |
| `p569pull t8x` | 3/20 | 3/20 | 3 | 3/20 | y |
| keeper hard dependency `needer` | 1/20 | 3/20 | 4 | 4/20 | y |
| k3 argument `=paired-2.0` | 0/20 | 2/20 | 3 | 3/20 | y |
| igw plain `>=igw0x-2` | 0/20 | 2/20 | 3 | 3/20 | y |
| igw plain `<igw0x-1` | 0/20 | 2/20 | 3 | 3/20 | y |
| cgp VER `>=cgp1x-2` | 0/20 | 2/20 | 3 | 3/20 | y |
| cgp VER `<cgp1x-1` | 0/20 | 2/20 | 3 | 3/20 | y |
| triangle `needer othermod` | 3/20 | 3/20 | 4 | 4/20 | y |
| triangle reversed | 3/20 | 3/20 | 4 | 4/20 | y |
| cgp USE `cgp0x` | 0/20 | 0/20 | 0 | 0/20 | y |
| `test_oracle_209`, or-choice `--backtrack=0` | 0/0 | 0/0 (suite green, unmoved) | - | 0/0 | y |

Rows: every `[ebuild ...]` line and every conflict block of the cells above is byte-identical before and after
(`diff` over `before/*.out` and `after/*.out`, apart from the count and the first-run "Performing Global Updates"
notice of the scratch roots), with two exceptions that print one more built-atom parent line in the residual
conflict block of the `--update --deep` abort cells `puD-p569pull` and `puD-p589bpull` (`(and 1 more with the same
problems)`): Portage merges there (backlog #598), so there is no Portage block to compare.

Corpus: no drift. The two triangle CASES rows (`backtrack: 3/20`) did not move, because the printed count stays 3.
