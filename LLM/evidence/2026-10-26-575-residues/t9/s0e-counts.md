# Task 9 S0e: cases that reach the dropped-pin heal, with backtrack counts

Written 2026-10-27. Binary: `target/release/portuale` as built (not rebuilt).
Portage: host 3.0.83, `PYTHONHASHSEED=0`, scratch trees only.

## How the list was made

1. **Whole suite through a tracing shim.** A scratch copy of pmtest
   (`/var/tmp/claude-s0e/pmtest`, removed afterwards) ran the suite with a
   registry entry whose `emerge` is `playground/s0e-shim-emerge.py`. The shim
   execs the binary with `PORTUALE_BT_TRACE=1`, strips the `BT` lines from
   stderr (so the tests still see the normal output), and appends one JSON
   record per call: test id, argv, ROOT, rc, pass count, `backtrack: N/M`,
   and two flags.
   - `heal`: a pass with `kind=Feedback:MissingDep` whose mask line has
     `reason=SlotConflict`. That is exactly what `dropped_pin_mask_restart`
     returns (`reverse_dep_pins.rs:158-168`; the other MissingDep masks say
     `reason=MissingDependency`).
   - `resid`: the final stdout has a `Multiple package instances` block that
     names an `installed in '...'` instance, that is, a residual conflict
     built from a dropped reverse-dependency pin. These are the calls whose
     pass has records at `dropped_pin_mask_restart` (the function was
     reached), whether or not G1 or a later gate let it mask.
   Scope: `test_emerge_pretend_contract.py`, `test_output_invariants.py`,
   `test_harvested_corpus.py` (the 4900-row expanded grid), and, in a first
   full-suite run (32860 calls, the whole `pytests-contract-suite`), heal
   only. In the three-file runs only `test_terminal_title_set_and_reset_on_a_pty`
   fails (the shim is not a tty-transparent exec). The first full-suite run
   also had 189 failures and 12 errors, from harness-binary tests
   (atom/versions/use_reduce) that need files the partial pmtest copy lacks;
   those run no resolver. Raw records: `captures/s0e/suite-heal-resid.jsonl`.
2. **Portage pairs** (`playground/s0e-run-all.sh`, `s0e-pair.sh`,
   `s0e-mkcfg.sh`): every distinct cell was re-run on a scratch copy of the
   suite's own root (fixtures copy `fx0`, or the per-test hermetic root copied
   out of the pytest basetemp), portuale and Portage with the suite's argv,
   no `EMERGE_DEFAULT_OPTS`, repos.conf repointed to absolute scratch paths
   for Portage. Raw captures: `captures/s0e/<cell>.{argv,portuale.*,portage.*}`.
   "Rows equal" compares the `[ebuild ...]` lines after dropping the
   `to <root>` suffix and sizes.
3. **Corpus**: `pytests-contract-suite/corpus/contract.json.xz` and
   `expanded.json.xz` searched for these calls and for any stored stdout with
   a residual installed-instance conflict.

## Cells where the heal fires (`heal` = y)

| Cell (test / CASES row) | argv (suite env; ROOT) | heal | portuale | Portage | rows equal | corpus has the count line |
|---|---|---|---|---|---|---|
| CASES `#568/#569: the perl-heal skip block ...`; `test_skip_block_keeps_the_built_atom...`; invariants, determinism and repos.conf-order tests (same call) | `--pretend dev-libs/p569pull` (fixtures) | y | 2/20, rc 0 | 2/20, rc 0 | y | no (no entry) |
| invariants only (`fixture_packages[p]`) | `--pretend dev-libs/p563pull` (fixtures) | y | 2/20 | 2/20 | y | no |
| invariants only | `--pretend dev-libs/p589apull` | y | 2/20 | **0/20** (Portage merges `U p589aprov-5.44` + `N p589apull-2`) | **n** | no |
| invariants only | `--pretend dev-libs/p589bpull` | y | 2/20 | **4/20** | y | no |
| invariants only | `--pretend dev-libs/p589cpull` | y | 2/20 | **4/20** | y | no |
| `test_keeper_reachable_pin_breaks_a_hard_dependency_requirement_and_reports_it` | `--pretend dev-libs/needer` (hermetic root, keeper-1.0 pins `=paired-1.0`) | y | 1/20, rc 1 | **4/20**, rc 1 | y | no (hermetic-root test, no entry) |
| t8 shape `-pv dev-libs/p569pull dev-libs/t8x` | fixtures | y (try 2 is the heal, try 3 a plain MissingDep) | 3/20 | 3/20 | y | no (new fixture shape) |
| t8 shape `-pv dev-libs/p589{a,b,c}pull`, `-pv dev-libs/p569pull` | fixtures | y | same as the `--pretend` rows above | same | same | no |

Not a heal: `-pv dev-libs/t8x` alone (1 plain `MissingDependency` mask;
1/20 both sides, rows equal). The `p563pull`/`p569pull`/`p589*` heals run in 3
passes (2 masks).

## Cells that reach the function with records but do not fire it (`resid` = y, heal = n)

All have an installed consumer's dropped pin in the final conflict block; G1
(argument-only puller), G0, `--backtrack=0` or the `||` supersession gate
keeps the heal from masking. These are the cells whose count can move when 10a
rewrites the scan.

| Cell | argv (suite env; ROOT) | heal | portuale | Portage | rows equal | corpus has the count line |
|---|---|---|---|---|---|---|
| CASES `the needer/othermod triangle reports the installed-instance conflict (#57, #62 ...)`; `test_needer_othermod_triangle...[needer-first]`; `test_keeper_reachable_needer_triangle...` (hermetic root: same count); `test_debug_parent_dep_names...` (`--debug`); invariants and determinism tests | `--pretend dev-libs/needer dev-libs/othermod` (fixtures) | n | 3/20, rc 1 | **4/20**, rc 1 | y | **yes: 3/20** |
| CASES `the same triangle, argv reversed ...`; `...[othermod-first]` | `--pretend dev-libs/othermod dev-libs/needer` | n | 3/20, rc 1 | **4/20**, rc 1 | y | **yes: 3/20** |
| `test_keeper_reachable_pin_breaks_an_explicit_upgrade_and_reports_it` (k3 argument) | `--pretend =dev-libs/paired-2.0` (hermetic) | n | 0/20, rc 1 | **3/20**, rc 1 | y | no |
| `test_ignore_world_contrast_against_a_world_bound_consumer` (igw plain) | `--pretend >=dev-libs/igw0x-2` and `--pretend <dev-libs/igw0x-1` (hermetic) | n | 0/20, rc 1 | **3/20**, rc 1 (both) | y | no |
| `test_complete_graph_ver_break_fails_with_world_consumer` | `--pretend >=dev-libs/cgp1x-2`, `--pretend <dev-libs/cgp1x-1` (hermetic) | n | 0/20, rc 1 | **3/20**, rc 1 (both) | y | no |
| `test_complete_graph_use_break_fails_with_world_consumer` | `--pretend dev-libs/cgp0x` (hermetic) | n | 0/20, rc 1 | 0/20, rc 1 | y | no |
| `test_or_choice_avoids_downgrade_into_the_graphed_update` (second call) | `--pretend --update --deep --backtrack=0 @world` (ROOT b1root, config root fixtures) | n | 0/0, rc 1 | 0/0, rc 1 | n (Portage lists extra BROOT rows; cross-root, not comparable) | no |
| `test_oracle_209_backtrack_zero_enforces_a_satisfiable_consumer_pin_in_pass` | `--pretend --backtrack=0 --update --deep --newuse --oneshot =dev-libs/r25lib-2.0` (hermetic) | n (needs `backtrack_max > 0`) | 0/0, rc 1 | 0/0, rc 1 | y | no (the test pins the count itself; unchanged) |
| invariants `fixture_packages[puD]` | `--pretend --update --deep dev-libs/p569pull` | n | 0/20, **rc 1** | 1/20, **rc 0** | n | no |
| same | `... dev-libs/p589apull` | n | 0/20, rc 1 | 0/20, rc 0 | y (rows before the abort) | no |
| same | `... dev-libs/p589bpull` | n | 0/20, rc 1 | 1/20, rc 0 | n | no |
| same | `... dev-libs/p589cpull` | n | 0/20, rc 1 | 0/20, rc 0 | y (rows before the abort) | no |

The four `puD` rows are a finding of their own: with `--update --deep` portuale
aborts with a residual conflict on the pull fixtures while Portage merges (rc 0).
Only the invariants test runs them (it asserts output shape, not rc or count),
so no pin covers them; they are outside Task 9/10's targets unless the owner
wants them added.

Confirmed from `t8/targets.md`: k3 argument 0/20 vs 3/20; k3 needer 1/20 vs
4/20; igw plain 0/20 vs 3/20 (both signs). T1-T3 (`swap-noslotop`,
`swap-slotop`, `orig-noslotop`) were not re-run: their fixtures do not exist
in pmtest yet (Task 9 adds them); the t8 numbers stand.

No other suite call fired the heal (checked over the full-suite run's 32860
calls) or showed a residual installed-instance conflict (checked over the
pretend-contract, invariants and harvested-corpus runs, which include the
4900-row expanded grid).
Limit: a pass that reaches the function and has no residual record
(returns at `records` empty) leaves no trace without rebuilding; those cannot
change under 10a's scan except through the records it adds, and
they print no conflict block today.

## Allowed 10a drift (corpus count lines)

Only two harvested entries both reach the function with records and store a
count line:

| Corpus entry (contract.json.xz) | today | may drift to |
|---|---|---|
| CASES `the needer/othermod triangle reports the installed-instance conflict (#57, #62: rc 1 like Portage)`, args `--pretend dev-libs/needer dev-libs/othermod` | `backtrack: 3/20` | `backtrack: 4/20` (Portage) |
| CASES `the same triangle, argv reversed, reports the same conflict (#57, #62: rc 1 like Portage)`, args `--pretend dev-libs/othermod dev-libs/needer` | `backtrack: 3/20` | `backtrack: 4/20` (Portage) |

Rows must stay byte-identical (same merge list, same conflict block). Every
other heal or residual cell above has no corpus entry (the p563/p569/p589
fixtures and the hermetic-root keeper tests were added after the harvest), so
a count change there cannot show up as corpus drift. The contract tests for
those cells do not assert the count (checked), except `test_oracle_209` and
the `--backtrack=0` or-choice call, which print 0/0 on both sides and sit
behind `backtrack_max > 0`, so they must not move at all.
The corrected statement for the 10a gate: drift allowed on the two triangle
entries (3/20 to 4/20); the k3 argument, k3 needer, igw and cgp counts in
`targets.md` are not in the corpus and are not asserted anywhere, so they
cannot trip a gate (they still must not move rows).
