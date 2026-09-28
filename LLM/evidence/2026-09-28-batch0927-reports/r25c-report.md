# R25c report — backlog #25 S1c gate + S2/S3 (2026-09-27)

Worktree `/home/vivo/repo/PORTUALE/wt-25-dynamic-deps/{portuale,pmtest}`,
branch `backlog/25-dynamic-deps`, at portuale `dc9e624a` / pmtest `9c7f399`
(unchanged — **no commits** by this slice). No judgment call taken;
one is needed from the owner (see §4).

## Status: NEEDS_CONTEXT

## 1. S1c gate measurements (current binary, release, rebuilt = fresh)

Binary `portuale/rust/target/release/portuale` verified fresh
(`cargo build --release` → "Finished in 0.12s", clean tree).

- **r25 shape C** (ad-hoc ROOT, committed ebuilds+vdb, `/tmp/opencode/r25c/probe.py`,
  log `/tmp/opencode/r25c/r25c-probe.log`): portuale prints
  `[U] r25up-2.0 + [N] r25target-1.0` **plus the skipped-update warning**
  (`dev-libs/r25lib:0` block, both consumer atoms `<2.0:0/1=` and `<2.0:=`);
  **`--json` `restarts=1`**. Real per S0: same two rows, silent,
  `backtrack: 0/20`. **STILL DIFFERS.**
- **#107 live shape** (`emerge -puD --getbinpkg --color=n net-libs/rest` on this
  host, worktree binary, log `/tmp/opencode/r25c/live-ptl.log`): portuale
  `md4c U + rest N[ebuild]` plus the `libdisplay-info:0` conflict block
  (both weston atoms `:0/3=` and `:=`); real (`docs/evidence/2026-09-27-107-reprobe/real-full.log`):
  `md4c U + rest N[binary]`, silent, `backtrack: 0/20`. **STILL DIFFERS**
  (ignoring the `[binary]`/`[ebuild]` row = #192, as instructed).
- **S0 regression set** on current code: 56 passed / 3 xfailed / 0 failed
  (`-k "slot_operator or slotop or keeper or needer or whpin or oracle_90 or
  oracle_91 or socc or undo or installed_consumers or rebuild"`,
  basetemp `/var/tmp/pmtest-r25c2`, log `/tmp/opencode/r25c/regression-set-full.log`;
  xfails are pre-existing strict ones; no code changed). **GREEN.**

So the brief's first fork condition is half-met: a shape still differs.
The second half ("`_minimize_children` is the cause") is true in real
(S0 `realC-debug.log:43-47`, r25fix-report §2) — but a literal port does
not produce real's outcome here (§3). That is the judgment call.

## 2. Why S1c-as-planned cannot land as specified

Real `_minimize_children` (`3rdparty/portage/lib/_emerge/depgraph.py:4751-4840`,
call sites `:4521`/`:4660`) is a **pure per-parent same-CP collapse**:
`_select_package` per atom, group by `pkg.cp`, installed-first elimination
(bug 631894), abi/conflict/normal yield order (bug 291142).

The r25 mid-puller and the slotop consumer present **byte-identical atom
shapes** to it (fixture-verified):

- r25mid: live `dev-libs/r25lib:=` + recorded `>=dev-libs/r25lib-1.0:0/1=`
- consrdep: live `dev-libs/provpkg:=` + recorded `>=dev-libs/provpkg-1.0:0/1=`

Both compute to: `_select_package(:=)` → 2.0 merge, `_select_package(>=:0/1=)`
→ installed 1.0, elimination drops 2.0 (every parent atom of 2.0 also matches
installed; installed is kept by the unique `>=` edge), both atoms → installed.
Real distinguishes the two shapes **downstream**: `_slot_operator_update_probe` /
`_slot_operator_check_reverse_dependencies` (`:2472-2538`, `:2494-2502`) +
`_slot_operator_trigger_reinstalls` (`:3089-3132`) schedule the slotop
upgrade+rebuilds while the r25 probe withholds — the probe, not the minimizer,
decides the upgrade (r25fix-report §2 says exactly this about real).

Portuale has no update probe by documented cut
(`slot_operator_rebuild_scan` docstring: "no `_slot_operator_check_reverse_-
dependencies` rejection, no `_slot_operator_update_probe` family (v2 `#24b`)").
Its provider upgrade flows **only** from selection (the `:=` edge resolving the
2.0 `Upgrade` entry), and both post-pass scans are gated on such entries:

- `reverse_dependency_constraints`: `if upgrading.is_empty() { return empty }`
  (`rust/portage-repo/src/lib.rs`, scan over `Upgrade`/`Downgrade` entries);
- `slot_operator_rebuild_scan`: rebuild scheduling keyed on `new_slot`, built
  only from `Upgrade`/`Downgrade`/`Reinstall` entries.

A literal minimizer at the selection-collapse point therefore collapses the
slotop consumer onto installed provpkg-1.0 exactly as it collapses r25mid —
killing the `provpkg-2.0` `Upgrade` entry, emptying both scans, and returning
the `BEDS 62951cd0: STOP` shape (no provider row, no `rR` rows; 19 unexplained
slotop `@world` rows). The S1b-fix record-site skip would be bypassed, not
extended: with no 2.0 node there is nothing to not-record. The hermetic pin
`test_oracle_slotop_world_upgrade_with_eapi_installed_bindings` (fails on
`62951cd0`, passes on `dc9e624a`) would fail again — verifiable without beds,
but the outcome is certain enough from the code that no prototype was built.

## 3. What was NOT done (and why)

- **No S1c commit.** Neither fork arm applies: the "nothing observable depends
  on it" arm is false (r25/live still differ, minimizer is real's cause), and
  the "do S1c as planned" arm produces a known regression, not real's outcome.
  No `READY-FOR-BEDS` line written (`r25s1-progress.md` untouched).
- **No S2.** As specified it asserts the two merge rows plus warning absence
  plus `--json` `restarts: 0` — that pin fails on current code, and writing it
  red is not landing S2. It waits on the §4 decision.
- **No S3.** Entry #25/#107 edits and the `what-this-proves.md` paragraph all
  depend on the §4 outcome. Nothing flipped, nothing edited, nothing committed.

## 4. Question for the owner (B8) — three options

The r25 silence needs the *outcome* "same-CP atoms settle on installed in pass 1
when the merge is vetoed", but portuale's architecture can only keep the slotop
upgrade if the collapse is veto-gated, which is not `_minimize_children`:

- **(a) Veto-gated selection collapse (recommended by this slice if any code is
  wanted):** at the selection point, when an atom resolves to a merge while an
  installed instance exists in the same slot AND a reachable/graph consumer pin
  vetoes the merge candidate (i.e. `built_slot_operator_rebuild_trigger` is
  false for it — the S1b-fix predicate reused), resolve to the installed
  instance instead. r25mid's `:=` → installed (consumer `<2.0` vetoes) → silent,
  restarts 0; slotop `:=` → 2.0 (relaxed `:=` accepts) → upgrade+rebuilds
  unchanged. Small and reuses proven predicates, but it is a new selection rule
  real does not have (real collapses unconditionally and probes later), with
  `--update`-semantics blast radius to assess against the full suite + beds.
- **(b) Literal minimizer + probe scheduling:** port `_minimize_children` AND
  the `_slot_operator_update_probe`/`_trigger_reinstalls` family (v2 `#24b`
  territory). Faithful, but an order of magnitude beyond S1c's "one commit +
  unit test" scope.
- **(c) Close without S1c:** document the residue (selection creates the L-new
  node; scan+restart+warning follow) in entry #25, keep S2 unpinned, leave the
  minimizer to the v2 probe item that owns the downstream half. Concedes the
  r25/`--json`-restarts/`#107`-warning divergences indefinitely.

## BED-PENDING (coordinator only, for whichever option lands code)

From the pmtest worktree, after any future S1c-attempt commit `<sha>`:

1. `FX_WORLD_EXTRA=dev-libs/r25consumer differential-test-bed/run/l0-fixture-oracle.sh differential-test-bed/atomlists/l0-fixture-oracle-r25.txt` — expect 0 unexplained (option (a)/(b)) or the standing warning row (option (c)).
2. `FX_SLOTOP_BDEP=1 FX_HOST_ROOTS=1 differential-test-bed/run/l0-fixture-oracle.sh differential-test-bed/atomlists/l0-fixture-oracle-slotop.txt` — expect 0 unexplained in all options (guards the S1b-fix).
3. `differential-test-bed/run/l0-fixture-oracle-all.sh` and `differential-test-bed/run/l0-resolver.sh` — expect no movement outside the r25 cell.
