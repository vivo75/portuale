# g198 report — Tracks G5 (#198) + G6 (#205): autounmask paths that never fall back to an older version

Status: DONE (no NEEDS_CONTEXT — no behavioural choice was left open by real's source; two disclosed residues below, unnumbered, for the coordinator to file).

Branch: `backlog/198-autounmask-older-fallback` (both worktrees).
Portuale: `d93b7609` (R1) → `f713be43` (R2) → `a321452a` (R3) → `db0e23bd` (R4).
Pmtest: `a45e7c5` (P1, akk0 pins + btnr re-pin) → `a0340f8` (P2, abk0 pins + bwd extension).
Do not flip the entries (per brief).

Rule-13 probes: one `podman run` per item (scripts `/tmp/opencode/g198/probe-198.sh`,
`/tmp/opencode/g198/probe-205.sh`; full logs `/tmp/opencode/g198/probe-198.log`,
`/tmp/opencode/g198/probe-205.log`, ephemeral). Image portage is 3.0.81.3, not the
3.0.82.2 the mechanisms are cited from (same caveat as g209: the autounmask/backtrack
paths below are long-stable; behaviour evidence 3.0.81.3, mechanism 3.0.82.2 source).
Result blocks below are verbatim modulo staging noise (Global Updates `2Q-2024` error,
`package.use.force` invalid-atom, `FREE` license-group, `cgroup, observability`
FEATURES, missing-`repo_name` warnings) and the staged `$FX` root path.

## #198 — `--autounmask-keep-keywords=y` never falls back to an older stable version

### S0 — mechanism + probe

Real (3rdparty/portage 3.0.82.2): with keep-keywords on, no `_autounmask_levels` step
(`lib/_emerge/depgraph.py:7446`) unmasks `~amd64`, so `akk0a-2`'s `akk0b` dep is
unsatisfiable even USE-free. `_add_dep` (`depgraph.py:3473-3503`) re-selects with
`dep.atom.without_use`, still finds nothing, and records
`_backtrack_infos["missing dependency"] = dep` with `_need_restart` (`:3502`);
`Backtracker._feedback_missing_dep` (`lib/_emerge/resolver/backtracking.py:197-207`)
masks `dep.parent` (`akk0a-2`) in `runtime_pkg_mask`. No top-level exemption exists
anywhere on this path. The retry picks `akk0a-1` (+`C[foo]` USE change in the
playground; on fixtures `foo` is globally on, so no change is owed).

Probe (rule 13, staged fixtures, `emerge -p --autounmask-keep-keywords=y dev-libs/akk0a`):

```
Dependency resolution took 0.25 s (backtrack: 1/20).

[ebuild  N     ] dev-libs/akk0c-1 to /tmp/g198-probe/fixtures/ USE="foo"
[ebuild  N     ] dev-libs/akk0a-1 to /tmp/g198-probe/fixtures/

!!! The following update has been skipped due to unsatisfied dependencies:

dev-libs/akk0a:0 for /tmp/g198-probe/fixtures/

  selected: (dev-libs/akk0a-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/g198-probe/fixtures/')
  skipped: (dev-libs/akk0a-2:0/0::testrepo, ebuild scheduled for merge to '/tmp/g198-probe/fixtures/') (see unsatisfied dependency below)

!!! All ebuilds that could satisfy "dev-libs/akk0b" for /tmp/g198-probe/fixtures/ have been masked.
!!! One of the following masked packages is required to complete your request:
- dev-libs/akk0b-1::testrepo (masked by: ~amd64 keyword)

(dependency required by "dev-libs/akk0a-2::testrepo" [ebuild])
For more information, see the MASKED PACKAGES section in the emerge
man page or refer to the Gentoo Handbook.
rc=0
```

Contrast cell in the same run (`--autounmask-keep-keywords=n`): `backtrack: 0/20`,
`[akk0b-1 ~] + [akk0a-2]`, keyword-changes block for `=akk0b-1 ~amd64`, rc 1.
Note the keep=y cell is rc 0 on fixtures (foo globally on) while the upstream
playground oracle is success=False (its profile has foo off) — the CASES row now
asserts rc 0 with the fixture-true reason in its description.

Portuale root cause: the `missing_dep_trigger` gate exempted top-level-selected
parents (`!ctx.top_level_cps.contains(...)`, added for the `||`-alternative case in
`716be6a1`), so `akk0a-2` was never masked and the run failed holding it.

### S1 — port (two rules, two commits)

- R1 (`d93b7609`, pmtest `a45e7c5`): drop the top-level exemption (the mask names one
  cpv, never the requested package; a pinned `=ver` request retries into the same NVC).
  Convergence half of the same rule: a retry whose walk fails dead-ends (failed
  iteration, best-run fallback — real's fruitless `_backtrack_depgraph` retry) instead
  of propagating run_pass's fatal top-level NVC. Without it R1 regressed ~30
  blast-slice cells (all top-level-NVC-after-mask shapes); with it the slice is back
  to 1 by-design failure. Rust unit test pins the fallback (`akk0c-1 + akk0a-1`, one
  restart).
- R2 (`f713be43`, same pmtest pair): render the full per-update form
  (`SkippedMissingDepFull`: header, `selected:`/`skipped:` rows, masked/plain-miss
  disclosure chained to the single skipped parent — real
  `_show_missed_update_unsatisfied_dep`, `depgraph.py:1592-1636`). The abbreviated
  tail keeps its shape for backtrack-masked atoms. Unit test extended with the row
  assertions. Portuale prints miss notices on stdout (the orbtblocked `WARNING`
  precedent); real prints them on stderr.
- P1 (`a45e7c5`): keep=y CASES row rc 1→0; keep=y/keep=n pins (final-state text);
  023 btnr re-pin (see below).

Side-effect triaged as a genuine fix: R1 moves the 023 btnr oracle
(`testBacktrackNoWrongRebuilds`, bug 375573 family) from upgrades+conflict-report
rc 1 to nothing-merged rc 0 — the upstream `mergelist=[]` + `success=True`. The new
`WARNING` (slot mask on `btra-2`) + abbreviated tail (`btrc:0`, missing-dep mask
whose `>=btra-2` still names backtrack-masked `btra-2`) follow real's deterministic
miss mapping from that mask state; notice verbatim beyond the oracle is recorded as
observed. Corpus drift on that cell is for the coordinator to bless.

Gates at R2: fmt clean, clippy release 0 warnings, `cargo test --release` green
(whole workspace), full pmtest suite 2108 passed / 0 failed (private fresh basetemp).
Corpus drift, NOT blessed: btnr rc+notices; `--json` mg2top/slotconfgroup
restarts-only (14→11, 6→4 — tighter R1 search, entries byte-identical).

## #205 — `--autounmask-backtrack=y` keeps the newest version where real backtracks

### S0 — mechanism + probe

Real: `_complete_graph` (`depgraph.py:8562`, `recurse` default-on; called
arg-less at `:9452`, `world` reached via the `_required_set_names` path at
`:8686-8709`) re-walks `@world`, pulls installed `B-1` in as a nomerge node, and its
`<A-3` dep fails against the graphed `A-3` (`_select_pkg_from_graph`). The end-of-walk
unsatisfied-dep loop (`depgraph.py:8756-8770`) finds the installed satisfier (`A-1`),
declares "scheduled installation broke a deep dependency", adds it, and the resulting
slot collision backtracks `A-3` away. The retry settles `A-2` (+`C[y]` for its dep,
`C[x]` for `D`), `backtrack: 2/2`, rc 1 (USE changes).

Critical scoping finding (verified locally before spending the probe): with `abk0b`
in `@world`, portuale's existing reverse-dep enforcement ALREADY settles
`[C-1(R x*y*), A-2(U), D-1(N)]` — the filed "portuale picks A-3" compares the
upstream oracle (`world=[B]`) against the fixture cell (world without `B`), where
real ALSO picks `A-3` (next probe block). Selection was never broken; the product
work is the two display rules below.

Probe (rule 13; staged fixtures twice: `fx1` = world + `dev-libs/abk0b` (upstream
shape), `fx0` = pristine world (literal CASES cell)).

fx1 `emerge -p --autounmask-backtrack=y --backtrack=2 dev-libs/abk0d` (verbatim):

```
Dependency resolution took 0.29 s (backtrack: 2/2).
[ebuild   R    ] dev-libs/abk0c-1 [1] to /tmp/g205-probe/fx1/ USE="x* y*"
[ebuild     U  ] dev-libs/abk0a-2 [1] to /tmp/g205-probe/fx1/
[ebuild  N     ] dev-libs/abk0d-1 to /tmp/g205-probe/fx1/

WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:

dev-libs/abk0a:0 for /tmp/g205-probe/fx1/

  (dev-libs/abk0a-3:0/0::testrepo, ebuild scheduled for merge to '/tmp/g205-probe/fx1/') USE="" ELIBC="glibc" conflicts with
    <dev-libs/abk0a-3 required by (dev-libs/abk0b-1:0/0::__unknown__, installed in '/tmp/g205-probe/fx1/') USE=""
    ^               ^


The following USE changes are necessary to proceed:
 (see "package.use" in the portage(5) man page for more details)
# required by dev-libs/abk0d-1::testrepo
# required by dev-libs/abk0d (argument)
>=dev-libs/abk0c-1 x y
rc=1
```

fx0, same args (verbatim): `backtrack: 0/2`, `[C-1(R x*), A-3(U [1]), D-1(N)]`, USE
block (`abk0d-1` + argument, `>=abk0c-1 x` only), NO conflict block, rc 1 — i.e. the
pristine CASES cell already matches real's selection; only its USE attribution
(3 lines vs 2) diverges.

fx1 without the flag (verbatim, third invocation): `backtrack: 0/2`, `A-3 + x`, the
full "Multiple package instances" collision for `A-3` vs installed `A-1` (B-1's
bound), the USE block, plus the terminated-early trailer, rc 1 — matches portuale's
pre-existing early-termination shape (not pinned; noted for #195's family).

### S1 — port (two rules, two commits; selection already correct)

- R3 (`a321452a`, pmtest `a0340f8`): record enforced-pin rejections as
  slot-conflict-style masks on the chosen version (real "Record missed updates",
  `depgraph.py:2085-2106`) so `backtrack_missed_updates` derives the `WARNING` row;
  the positive pin still drives selection and the negative excludes nothing it does
  not already exclude (budget-free, `backtrack_max > 0` gated). Installed enforcing
  consumers render vdb-based (`installed`, `__unknown__` fallback, vdb USE) like the
  withhold rows' established shape. Same rule adds the bwd warning's slot-op binding
  parent line (real records every non-matching kept-instance parent atom) — locked
  into that pin.
- R4 (`db0e23bd`, same pmtest pair): USE chains start at the recorded forcing
  parent (`AutounmaskChange::trigger`, set only at the main USE-suggestion site
  where the current dep's mismatch forces the flip by construction; overlays and
  parent-flip rescues keep `None` since their owner is often not forcing —
  aucasctop/aubreaktop pins prove it). Stale triggers fall back to the legacy
  first-requirer start; keyword/license/mask unchanged (real renders those without
  the unsatisfied filter); ascent unchanged. An earlier last-merge-parent variant
  was tried and reverted (wrong where a satisfied parent sorts last).
- P2 (`a0340f8`): CASES abk0d description corrected (pristine cell matches real);
  pristine pin (A-3+x, 2-line chain); world=B test-local-ROOT pin (vdb mirrors the
  fixtures minus the absent `repository` file; full probe text); bwd parent-line
  extension.

R3 interim (verified by patch-dance rebuild + slice, not just reasoning): exactly
the 2 abk0-pin attribution failures, everything else green.

Gates at R4: fmt clean, clippy release 0 warnings, `cargo test --release` green
(1820/0), full pmtest suite 2110 passed / 0 failed (private fresh basetemp).
Corpus drift, NOT blessed: btnr (re-pinned); bwd stdout (rule-grounded parent
line, pin extended); `--json` mg2top/slotconfgroup restarts-only.

## Residues (unnumbered — coordinator to file)

1. USE_EXPAND grouping in the fuller USE display: real `pkg_use_display`
   (`lib/_emerge/UseFlagDisplay.py:55-109`; `:52` is just the `_flag_info`
   namedtuple) groups effective-USE expand flags, so the skipped row
   shows `USE="" ELIBC="glibc"`; portuale's `pkg_use_display_for` builds from IUSE
   tokens only, so every skipped-update row (old and new) shows `USE=""`. Pre-existing
   family gap; the P2 pin docstrings disclose the delta line-by-line.
2. Same-version-reinstall `[1]` oldbest on repo drift: real shows it via the
   `not quiet_repo_display and repo differs` disjunct
   (`lib/_emerge/resolver/output.py:720-731`, third disjunct at `:729-730`);
   portuale deliberately cuts that disjunct (`resolve_pretend`'s `myoldbest`
   comment). Reopening the cut is a policy call. P2 pins disclose the delta.

## BED-PENDING (coordinator; run from the pmtest worktree of this pair)

- `differential-test-bed/run/l0-resolver.sh` after each of `d93b7609`, `f713be43`,
  `a321452a`, `db0e23bd`: expect identical-or-better vs Z0 row by row (akk0 keep=y
  selects A-1+C-1; btnr settles empty, rc 0).
- `differential-test-bed/run/l0-fixture-oracle-all.sh` ditto: expect 0 unexplained
  (akk0 keep=y row flips rc 1→0 with real's merge list).
- No merge-path beds: nothing touched the merge code (resolver + display only).

## Repro (hermetic, no container)

- keep=y: `emerge --pretend --autounmask-keep-keywords=y dev-libs/akk0a` → rc 0,
  `[akk0c-1 USE="foo", akk0a-1]` + full-form skip notice + masked-`akk0b` block.
- keep=n: same with `=n` → rc 1, `[akk0b-1 ~, akk0a-2]` + keyword block.
- abk0 pristine: `emerge --pretend --autounmask-backtrack=y --backtrack=2
  dev-libs/abk0d` → rc 1, `[C-1(R x*), A-3(U), D-1(N)]` + 2-line USE block.
- abk0 world=B: test-local ROOT per `test_autounmask_use_backtrack_world_bound_…`
  → rc 1, `[C-1(R x*y*), A-2(U), D-1(N)]` + WARNING + 2-line USE block.
