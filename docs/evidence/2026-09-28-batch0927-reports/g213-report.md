# G213 report — backlog #213 (v2 #24d): `prune_rebuilds`

Worktree `/home/vivo/repo/PORTUALE/wt-213-prune-rebuilds/{portuale,pmtest}`,
branch `backlog/213-prune-rebuilds`.
Real source: vendored portage 3.0.82.2 (`3rdparty/portage/lib/_emerge/depgraph.py`,
`lib/_emerge/resolver/backtracking.py`).

## Status: DONE (BED-PENDING — coordinator runs the beds)

## Commits (paired, pmtest first)

- pmtest `9aab356` — `fixtures+contract: #213 prune_rebuilds S0 shape,
  bed cell and count pin` (names the portuale branch + slice).
- portuale `54d019a9` — `resolver: #213 prune_rebuilds restart on
  replace-set-meets-missed-updates (pmtest 9aab356)` (quotes the pmtest
  short sha).

`READY-FOR-BEDS 54d019a9` written as the last line of `g213-progress.md`.
No `BEDS 54d019a9: …` reply was present at report time; per rule 13 the
beds below are coordinator-only and DONE is reported with BED-PENDING.

## §1. What landed

Real's `prune_rebuilds` restart, next to the #211/#212 probes
(`rust/portage-repo/src/lib.rs`, one rule, one product commit):

- `BacktrackParams::prune_rebuilds: bool` (real's `prune_rebuilds`
  backtrack parameter, `depgraph.py:710`), compared by `params_equal`
  like real's parameter `__eq__` (`backtracking.py:76`).
- `collect_feedback` fires it exactly where real does — after the
  replace-set restart (real `:5700-5710` returns first when the set
  grew this pass), before `_eliminate_rebuilds` (real `:5777-5780`,
  skipped on the prune pass by the early return): replace set
  non-empty, no live slot conflict (real's `:5685-5690` conflict
  return precedes the check), and the mask half of
  `backtrack_missed_updates` (real `_get_missed_updates`,
  `:1529-1565`) non-empty. `apply_prune_rebuilds` latches the flag,
  clears the whole set and drops the `slot_operator_mask_built`
  steers (real `_feedback_config`, `backtracking.py:247-257`;
  portuale keeps no parallel set — the
  `MaskReason::SlotOperatorBuilt` negatives ARE the record), then a
  budget-free Config restart. `_ENABLE_PRUNE_REBUILDS` (`:628`) is
  unconditionally true upstream.
- Documented cuts (in-code): the
  `_ignored_binaries_autounmask_backtrack` disjunct (`:5764`, needs
  ignored-binaries/autounmask-USE state the A3 overlay does not track
  — no pin diverges on it); real's second missed-update source
  `_conflict_missed_update` (rides `pass.skipped_updates` mixed with
  #90-withhold rows, no clean read at this layer — the mask half
  fires on every probed shape). The `--solver=` bridges have no
  backtrack loop, so no prune there (same standing cut as #212).

## §2. S0: shape, probe, divergence (all verified, none assumed)

Hand fixture (all EAPI 8, profiles untouched EAPI 5):
`app-misc/pprov-1` (`0/1`), `app-misc/pprov-2` (`0/2`, the
subslot-bump target), `app-misc/pcons-1` (`RDEPEND="app-misc/pprov:="`),
installed `pprov-1` + `pcons-1`
(`RDEPEND=">=app-misc/pprov-1:0/1="`, real's versioned built-atom
shape, with `EAPI=8` vdb files per the g211b lesson), next to the
committed `dev-libs/btparent` conflict trio (backtracking masks
`bttarget-2.0`).

Shape archaeology — host `ResolverPlayground` against the vendored
3.0.82.2, i.e. real's own `depgraph.py`, EAPI 8 throughout,
`PYTHONHASHSEED=0` (scripts in `/tmp/opencode/g213/`, every GnuPG
home killed + removed per rule 17):

- Same-slot subslot bump + ebuild-installed consumer FIRES the
  update probe: replace set `{pcons, pprov}`, mergelist
  `[pprov-2, pcons-1]` (exp3). A slot MOVE with an installed
  consumer (`mmprov` shape) does NOT fire (empty replace set,
  mergelist `[mmprov-3]` only — exp1/exp2; see §5 observation).
- No-update runs never schedule (exp6); a consumer-side update
  (`pcons-2.0` available) suppresses the probe entirely (exp5).
- Combo (replace set + solved conflict) at `--backtrack 10` and
  `20`: prune ON = 8 passes, OFF = 6 passes, byte-identical
  mergelists — the +2 passes ARE the prune clear+regrow cycle
  (`_ENABLE_PRUNE_REBUILDS` False/True is the only delta; exp4/exp8).
  Low budgets (1–3) never reach the prune point (identical
  unresolved-conflict answers, exp5).

The one allowed real probe (single `podman run --entrypoint
/bin/bash localhost/test-portuale:latest` running the bed's
`stage.sh` with `FX_PRUNE_VDB=1`, then `emerge -p --color=n -uDvN
app-misc/pprov app-misc/pcons dev-libs/btparent`), pasted verbatim
(full log `/tmp/opencode/g213/probe-real.log`, throwaway container,
`--rm`):

```
===== emerge --version
Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability
===== CONTROL: emerge -p --color=n -uDvN app-misc/pprov app-misc/pcons dev-libs/btparent
Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability

Performing Global Updates
(Could take a couple of minutes if you have a lot of binary packages.)
  .='update pass'  *='binary update'  #='/var/db update'  @='/var/db move'
  s='/var/db SLOT move'  %='binary move'  S='binary SLOT move'
  p='update /etc/portage/package.*'
/tmp/g213stage/fixtures/repo/profiles/updates/2Q-2024..
@s

Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability

Performing Global Updates
(Could take a couple of minutes if you have a lot of binary packages.)
  .='update pass'  *='binary update'  #='/var/db update'  @='/var/db move'
  s='/var/db SLOT move'  %='binary move'  S='binary SLOT move'
  p='update /etc/portage/package.*'
/tmp/g213stage/fixtures/repo/profiles/updates/2Q-2024..
@s

Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability

These are the packages that would be merged, in order:

Calculating dependencies  ... done!
Dependency resolution took 0.41 s (backtrack: 7/20).

[ebuild  N     ] dev-libs/bttarget-1.0::testrepo to /tmp/g213stage/fixtures/ 0 KiB
[ebuild  r  U  ] app-misc/pprov-2:0/2::testrepo [1:0/1::testrepo] to /tmp/g213stage/fixtures/ 0 KiB
[ebuild  N     ] dev-libs/btconsumer-1.0::testrepo to /tmp/g213stage/fixtures/ 0 KiB
[ebuild  N     ] dev-libs/btpin-1.0::testrepo to /tmp/g213stage/fixtures/ 0 KiB
[ebuild  rR    ] app-misc/pcons-1::testrepo to /tmp/g213stage/fixtures/ 0 KiB
[ebuild  N     ] dev-libs/btparent-1.0::testrepo to /tmp/g213stage/fixtures/ 0 KiB

Total: 6 packages (1 upgrade, 4 new, 1 reinstall), Size of downloads: 0 KiB

WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:

dev-libs/bttarget:0 for /tmp/g213stage/fixtures/

  (dev-libs/bttarget-2.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/g213stage/fixtures/') USE="" ELIBC="glibc" conflicts with
    <dev-libs/bttarget-2.0 required by (dev-libs/btpin-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/g213stage/fixtures/') USE="" ELIBC="glibc"
    ^                  ^^^


!!! The following update(s) have been skipped due to unsatisfied dependencies
!!! triggered by backtracking:

dev-libs/btconsumer:0 for /tmp/g213stage/fixtures/

The following packages are causing rebuilds:

  (app-misc/pprov-2:0/2::testrepo, ebuild scheduled for merge to '/tmp/g213stage/fixtures/') causes rebuilds for:
    (app-misc/pcons-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/g213stage/fixtures/')
rc=0
```

Version disclosure: the image carries Portage 3.0.81.3 (separate
`emerge --version` query: `Portage 3.0.81.3 (python 3.14.6-final-0,
…)`), not the 3.0.82.2 pin — the bed's `in-container.sh`
upgrades to the pin before running cases, so the coordinator's bed
run is the version-authoritative check. The behaviour is tied to
3.0.82.2 independently: the Playground runs above execute the
vendored 3.0.82.2 `depgraph.py` and agree exactly (8 passes, same
six rows, `backtrack: 7/20` equivalent). All `file:line` citations
are from the vendored 3.0.82.2.

So the S0 divergence is the pass count, not the rows — exactly like
upstream's own prune test (`test_skip_update.py`, bug 915494), which
pins only the backtrack budget (3 vs 2) for an identical mergelist.
The bed comparator does not read the `backtrack: N/M` line, so the
new `l0-fixture-oracle-g213.txt` cell (tenth list in
`l0-fixture-oracle-all.sh`, knob `FX_PRUNE_VDB=1`) locks the
rows/warnings, while the contract pin
(`test_oracle_prune_rebuilds_restart_adds_passes`) pins the count
through `--json` (`backtrack.restarts == 7`; portuale prints no
timing line under `--pretend`). RED→GREEN by patch dance (no
stash, rule 14): pre-fix tree `restarts: 5` with identical rows,
post-fix `7`. No CASES entry: the shape needs ad-hoc vdb, like the
mmcons/somass pins.

## §3. Verification (all green, release profile)

- `cargo fmt --check`: clean.
- `cargo clippy --release --all-targets`: zero warnings.
- `cargo test --release` (whole workspace): **1858 passed / 0 failed**
  (log `/tmp/opencode/g213/cargo-test.log`), incl. the new
  `slot_operator_prune_rebuilds_clears_replace_and_built_masks`.
- Focused sets: `slot_operator` 29, `prune` 8, `missed` 4,
  `backtrack` 14 — all green.
- Full pmtest suite: **2166 passed / 37 skipped / 4 xfailed /
  0 failed** (log `/tmp/opencode/g213/pmtest-full.log`, fresh
  private basetemp `/var/tmp/pmtest-g213-full`, 493 s). The 5→4
  xfail delta vs the g212 row is main-motion (commits between that
  base and this worktree's HEAD), not this slice: zero failures
  means no strict-xfail flipped, and no `xpassed` means no
  non-strict flip either. Corpus drift: NONE (nothing to bless —
  and nothing blessed).
- `test_fixture_caches`: 6 passed (new md5-cache entries valid).
- `pmtest/3rdparty/portage` present. Rule 17: every Playground
  GnuPG home I created killed (`gpgconf --kill`) + removed; `ps`
  shows none of my `--homedir /tmp/portuale-g213-gpg-*` agents
  (the remaining `/tmp/n194-oracle-gpg-*` daemons belong to another
  implementer — not mine to kill).

## BED-PENDING (coordinator only, from the pmtest worktree)

1. `differential-test-bed/run/l0-fixture-oracle-all.sh` — expect the
   new `l0-fixture-oracle-g213.txt` cell green (rows/warnings
   byte-match real; the backtrack count is comparator-invisible)
   and no movement elsewhere (the prune only adds passes where a
   replace set meets masks; the contract suite asserts the one
   shaped cell, and the full suite shows no other count moves).
2. `differential-test-bed/run/l0-resolver.sh` — expect
   identical-or-better vs the last green L0 row (Track G guard).

## §4. Concerns / disclosures

1. Did not flip backlog entry #213 (brief: "Do not flip the entry").
2. Nothing blessed (`PORTUALE_CORPUS_BLESS=1` untouched); no
   oracles, allowlists, thresholds or existing pins touched.
3. Never pushed, merged, or rebased (brief + rule 2); started no
   processes that outlive the slice (the detached suite runner
   completed; the silent-death rebuild was restarted, not killed).

## §5. Observation for the coordinator (out of scope, not a verdict)

On the `mmprov` shape (installed `mmprov-1` `0/1` + `mmprov-2`
`1/1`, consumer `mmcons-1` bound `0/1=`, tree `mmprov-3` `1/2`,
`emerge -uD mmprov mmcons`), real's depgraph (Playground, vendored
3.0.82.2, EAPI 8, `PYTHONHASHSEED=0` — script
`/tmp/opencode/g213/exp1.py`, rule-17-clean) merges ONLY
`mmprov-3`: the new-slot update probe does not fire (empty
`slot_operator_replace_installed`, spied in `exp2.py`). The
committed #211-item-2 pin
(`test_oracle_slotop_update_probe_mismatched_upgrade_entry`)
asserts portuale DOES rebuild `mmcons-1` there as "MATCHES real
(code-grounded…)" — code-grounded, never probed; this is the first
real-Portage data on that shape and it disagrees. The same-slot
arm (the `pprov` shape of this slice) fires fine in both. No
behaviour change made here (out of scope for #213); recommend a
container probe of the `mmprov` shape before trusting that pin,
and a backlog number if it confirms.
