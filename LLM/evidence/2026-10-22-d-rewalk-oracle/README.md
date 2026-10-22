# `-D` second resolver walk: can it be skipped? (2026-10-22)

Portuale HEAD `1464ee4e` (branch `backlog/441-s0-s1`), Portage 3.0.82.2.
Read-only: all instrumentation was temporary (`instrumentation.diff`),
reverted with `git checkout`, binary rebuilt, both trees `git status` clean.

## Verdict

**Dropping the second walk and keeping the unseeded first-walk result is UNSAFE.**
On 42 of the 666 contract-suite invocations that reach the branch, the result differs.
With the second walk skipped, 23 contract tests fail. Most of them are `test_oracle_*`
pins whose expected output comes from Portage or from upstream Portage tests.

**It is SAFE only with condition X:** under `-D` without `--complete-graph`, the
first walk itself runs with seeds (`with_seeds=true`). Then the `else if auto_enable`
walk is redundant: the seeded first walk and the seeded second walk were struct-equal
(`GraphResult ==`) on all 664 hits, and the whole suite passes. **But X changes
the host abort-path output.** On the frozen `emerge -uDpvN @world` run it adds a
`kde-frameworks/karchive:6` slot-conflict block. Portage does not print that block
(Portage does not abort on this host at all). So X also needs an abort fallback
that re-runs unseeded. That fallback moves ~14 s onto exactly the frozen benchmark,
which pays nothing today. X has not been run through the L0 bed.

**Time:** on the frozen host benchmark the second walk costs **nothing**, because
phase 1 aborts and the branch is never reached. On the non-aborting world-shaped
bench (#461) it costs ~0.23 s of 1.04 s wall (~21%).

## 1. Portage: there is no second walk; `_complete_graph` extends the same graph

- `--deep` alone never sets `complete`: `_emerge/create_depgraph_params.py:157-159` sets
  `deep`. `complete` comes only from `--complete-graph`, `--rebuild-if-new-rev`,
  `--rebuild-if-new-ver` and `--rebuild-if-unbuilt` (`:169-175`), `remove` (`:124`),
  and is dropped by `--nodeps` (`:181-184`).
- Runtime enablers: `depgraph.py:8592-8648`. Complete mode auto-enables when a merge
  node changes an installed package's version or slot/sub-slot (`complete_if_new_ver`),
  its IUSE or enabled USE (`complete_if_new_use`), or adds a new slot/sub-slot of an
  installed cp (`rebuild_if_new_slot`). All three default to `"y"`.
  `_resolve_conflicts` also enables it on unaccepted slot conflicts
  (`depgraph.py:9443-9450`). Uninstall tasks enable it too (`depgraph.py:10371`).
- Call chain: `_resolve` → `_create_graph()` (`depgraph.py:5676`). If that fails,
  it returns early (`:5677-5678`), so a failed first graph never reaches
  `_complete_graph`. Then `altlist()` (`:5681`) → `_resolve_conflicts()` (`:9319`) →
  `_complete_graph()` (`:9452`).
- `_complete_graph` (`depgraph.py:8562-8790`) reuses the **same** depgraph. It switches
  selection to `_select_pkg_from_graph` (`:8661-8666`) and adds `SetArg`s for the
  required sets at `_UNREACHABLE_DEPTH` (`:8724-8749`). Then it calls
  `_create_graph(allow_unsatisfied=True)` once (`:8751-8758`).
  - When `deep is True` and the required sets are already in the graph, and there
    are no ignored or stacked deps (`:8711-8721`), the root `continue`s and nothing
    is added. `-uD @world` is that case.
  - `--deep=N` (an int) is not `already_deep` (`:8668`). In that case the arm adds
    the set args.
- Live probe, `python3 -c` with `parse_opts` + `create_depgraph_params`:
  ```
  ['-uDpvN', '@world'] --deep= True deep= True False
  ['-uDpvN', '--deep=3', '@world'] --deep= 3 deep= 3 False
  ['-uDpvN', '--complete-graph', '@world'] --deep= True deep= True True
  ['-uDpvN', '--rebuild-if-new-rev', '@world'] --deep= True deep= True True
  ```
- Live probe, ResolverPlayground (`pg_probe.py`; installed `lib-1` at `0/1`,
  `app:=`, world `app`, ebuild `lib-2` at `0/2`). Each tuple is
  (call, kwargs, `complete` in myparams, `_complete_mode`, len(dep_stack)):
  ```
  ['@world'] ['--deep','--update'] mergelist= ['dev-libs/lib-2', 'app-misc/app-1']
      ('create_graph', {}, False, False, 1)
      ('complete_graph->', 1, False)
      ('create_graph', {}, False, False, 2)
      ('create_graph', {'allow_unsatisfied': True}, True, True, 0)   <- complete on, empty stack
      ('complete_graph->', 1, True)
  ```
  Under `-uD @world`, complete mode auto-enables, but its `_create_graph` starts
  with an empty dep stack: no re-walk. Without `@world` (`-uD dev-libs/lib`), the
  same call starts with a stack of 1: the incremental extension does real work.
  Portage's analogue of portuale's seeded re-walk is therefore an **incremental
  extension of one graph**, not a second walk.

## 2. Portuale: what the second walk is, and who reaches it

- Branch: `crates/portuale/src/pretend/resolve_phases.rs:477-496`.
  - It is reached only if phase 1 did not abort (`:458-460`).
  - It requires `!want_complete` (`:454`), i.e. no `--complete-graph` and `deep != NotRequested`.
  - It requires `auto_enable` (`:444-453`; predicate `crates/portage-repo/src/resolve/complete_graph.rs:19-50`).
- The second walk is not a deterministic replay. It runs with `with_seeds=true`
  (`:483`), while phase 1 runs with `with_seeds=false` (`:367`). The seeds fill
  `complete_seed_atoms` (`resolve_phases.rs:184-186`). Those feed `slot_op_reachable`
  and `world_reachable` (`crates/portage-repo/src/resolve/ctx.rs:205-222`).
  Those in turn gate:
  - the slot-operator rebuild scan (`resolve/assemble.rs:29`);
  - the slot-conflict backtrack, unsatisfied dead end and slot-conflict ABI paths
    (`feedback/slot_conflict_backtrack.rs:65,81`, `unsatisfied_dead_end.rs:46,65`,
    `slot_conflict_abi.rs:47`);
  - reverse-dep pins (`feedback/reverse_dep_pins.rs:31-35`);
  - the top-level withhold (`pass/top_level_unsatisfied.rs:145`).

  The comment at `:479-481` ("A deterministic input means an identical walk")
  covers only the struct-equality claim. The code itself says the inputs differ.

Contract suite. Run 1 is full, `PMTEST_NO_BUILD=1 PORTUALE_DREWALK_LOG=… scripts/pytest-parallel.sh`:
4262 passed / 2 skipped / 2 xfailed. **666 hits** on the branch: 624 SAME, 42 DIFF
(per-argv list in `run1-hits.txt`). The DIFF argvs:
- ~25 `-uD @world` variants: `--newuse`, `--with-bdeps=y`, `--usepkg`, `--changed-slot`,
  `--backtrack 4`, `--json`, `--solver=pubgrub`;
- `r25target`, `r107target`, `whpuller`+`slotconflict*consumer`, `consdep`/`consbdep`,
  `reinstslottarget`, `pprov`/`pcons`, `somassa`, `=r25lib-2.0 --backtrack=0`.

The differences are in merge entries (e.g. `r25lib` `Upgrade 1.0->2.0` in the first
walk vs `AlreadyInstalled` in the seeded walk; `sounneed` `Reinstall
slot_operator_rebuild` only in the seeded walk), `slot_conflicts`, and `backtrack_restarts`.

## 3. First-walk vs second-walk outputs

- Run 2 keeps the unseeded first result (`PORTUALE_DREWALK_SKIP=1`):
  **23 failed** / 4239 passed (`run2-skip-failures.txt`). Example:
  `test_236_r25_default_backtracking_settles_in_one_silent_pass` (Portage
  fixture-oracle + playground grounded) gets
  `[ebuild  r  U  ] dev-libs/r25lib-2.0 [1.0]` where Portage has `r25target-1.0 [N]`.
- No sufficient cheap predicate was found. "No slot or sub-slot change in the first
  walk ⇒ SAME" fails: 5 of the 42 DIFFs have no such change, e.g. the `whpuller`
  blocker withhold and pubgrub `sounneed`.
- Run 3: seeded first walk under `-D` (`PORTUALE_DREWALK_SEEDFIRST=1`) with the
  second walk skipped: **4262 passed, 0 failed**, no corpus drift.
- Run 4: seeded first walk, second walk still run: **664/664 SAME**.
- Run 5 runs both first walks on every invocation (`run5-seeded-vs-unseeded.txt`).
  The table is approximate: some concurrent log lines interleaved.
  ```
  4874 u_abort=false s_abort=false u_ae=false s_ae=false eq=true   <- seeding is a no-op when auto_enable is off
     2 u_abort=false s_abort=false u_ae=true  s_ae=false eq=false  (today: re-walk == seeded result)
    40 u_abort=false s_abort=false u_ae=true  s_ae=true  eq=false  (today: re-walk == seeded result)
   619 u_abort=false s_abort=false u_ae=true  s_ae=true  eq=true
   572 u_abort=true  s_abort=true  u_ae=false s_ae=false eq=true
  ```
- **The host abort path breaks X.** Both runs below are
  `target/release/portuale emerge -uDpvN @world` with the same host
  EMERGE_DEFAULT_OPTS:
  - Baseline: rc 1, stdout sha256 `d47f40db…`.
  - With X: rc 1, stdout `f9be95dd…`, with an **extra**
    `kde-frameworks/karchive:6` slot-conflict block (`host-abort-seedfirst.diff`).
    stderr is identical.
  - Portage, same argv at the same time (`/usr/bin/emerge -uDpvN @world`): rc 0,
    `backtrack: 20/20`, skipped-update warning for `libdisplay-info`/`mime-types`.
    It never mentions karchive.

## 4. Cost

- Frozen host benchmark, `PORTUALE_PROFILE=1 target/release/portuale emerge -uDpvN @world`:
  one `[profile] resolve_once complete=false: 14.248` span and zero branch hits.
  `phase1_aborted` short-circuits at `resolve_phases.rs:459-460`, so the second walk
  costs that benchmark nothing.
- World-shaped bench, `PMTEST_NO_BUILD=1 python3 bench/world_shaped_benchmark.py --skip-build --runs 5`
  (it hits the branch, SAME). Medians:

  | build | wall | resolve_once Σ | stdout sha |
  |---|---|---|---|
  | current | 1.044 s | 0.818 s | `0f42cee8…` |
  | X (seeded first + skip) | 0.827 s | 0.591 s | `0f42cee8…` |
  | skip, unseeded | 0.805 s | 0.584 s | `0f42cee8…` |

## Not established

- X is not checked against the L0 bed or the fixture-oracle bed. The suite is green
  under X, but the host diff shows X can change abort reports.
- It is not established whether X's extra karchive block is "more wrong" than
  today's: both disagree with Portage, which does not abort here.
- The first playground depgraph instance shows `complete` not enabled even though
  `lib-2` is visible. A guess, not traced: that instance had not yet selected `lib-2`
  before a slot-operator backtrack restart.
- `--deep=N` (int) is not probed on the portuale side.
