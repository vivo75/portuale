# G211 report — backlog #211 (v2 #24b): slot-operator update probe (2026-09-27)

Worktree `/home/vivo/repo/PORTUALE/wt-211-slotop-update-probe/{portuale,pmtest}`,
branch `backlog/211-slotop-update-probe`.
Real source: vendored portage 3.0.82.2 (`3rdparty/portage/lib/_emerge/depgraph.py`).

## Status: DONE (BED-PENDING — coordinator runs the beds)

## Commits (paired, pmtest first)

- pmtest `fdbbbdf` — `contract: #211 un-xfail the conflict-mass rebuild pin`
  (names the portuale branch + slice; corpus drift on the cell left
  unblessed).
- portuale `d3ff24fd` — `resolver: #211 new-child-slot update-probe arm in
  the rebuild scan (pmtest fdbbbdf)` (quotes the pmtest short sha).

`READY-FOR-BEDS d3ff24fd` written as the last line of `g211-progress.md`.
No `BEDS d3ff24fd: …` reply was present at report time; per rule 13 the
beds below are coordinator-only and DONE is reported with BED-PENDING.

## Verification (all green, release profile)

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo clippy --release --all-targets`: zero warnings (one new
  `#[allow(clippy::too_many_arguments)]` on the scan — 9 args; precedent:
  five existing allows in the same file).
- `cargo test --release`: 26 suites green, 1765 passed / 0 failed
  (log `/tmp/opencode/g211/cargo-test.log`), incl. the new
  `slot_operator_rebuild_scan_schedules_consumers_of_a_slot_moving_provider`
  (1 positive + 5 guard arms) and every pre-existing scan test unchanged.
- Regression set (`-k "slot_operator or slotop or keeper or needer or whpin
  or oracle_90 or oracle_91 or socc or undo or installed_consumers or
  rebuild or r25"`, log `/tmp/opencode/g211/regression-set.log`):
  59 passed; the only failure was the strict-xfail XPASS on the pin itself
  (i.e. the fix), resolved by the pmtest commit.
- Full pmtest suite: **2074 passed / 37 skipped / 5 xfailed / 0 failed**
  (log `/tmp/opencode/g211/pmtest-full.log`, private basetemp
  `/var/tmp/pmtest-g211-full`). Corpus drift (NOT blessed): exactly one
  entry, `test_oracle_slotop_conflict_mass_rebuild#0: stdout changed` —
  the intended behaviour change (leaves now merge).
- Live shape check (ad-hoc ROOT, committed `somass*` ebuilds+vdb):
  `[ebuild rS] somassb-2 [1]` + 5× `[ebuild rR] somasscNc-1` +
  `[ebuild N] somassa-1` + the `causing rebuilds` block — upstream's
  `[A-1, B-2, 5 leaves]` mergelist.

## BED-PENDING (coordinator only, from the pmtest worktree)

1. `FX_SLOTOP_BDEP=1 FX_HOST_ROOTS=1 differential-test-bed/run/l0-fixture-oracle.sh differential-test-bed/atomlists/l0-fixture-oracle-slotop.txt` — expect 0 unexplained (the scan's same-slot arm is byte-frozen; the new arm needs a slot-moving provider, absent from that list).
2. `differential-test-bed/run/l0-fixture-oracle-all.sh` — expect no movement outside the r25 cell (still warning-red until G13b/S1c).
3. `differential-test-bed/run/l0-resolver.sh` — expect identical-or-better vs the last green L0 row (Track G guard).

## Concerns / disclosures

1. The `check_reverse_dependencies` refusal (R2) is not ported: a parent
   whose atom the fresh child violates does not veto the rebuild. No
   hermetic pin diverges on it; the r25 veto shape is decided upstream of
   the scan (record site), verified green.
2. `Upgrade`/`Downgrade`/`Reinstall` entries with a bound-slot mismatch do
   not probe (only `New` feeds the fresh map). Real would probe an upgrade
   in slot B with a consumer pinned to slot A. No pin covers it; noted as
   v2 `#24b` remainder in-code.
3. The whole trigger (both arms) stays off under `--rebuild-if-new-slot=n`
   via the pre-existing call-site gate; real gates only the new-slot arm
   that way. Pre-existing narrowing, untouched.
4. Did not flip backlog entry #211 (brief: "Do not flip the entry").

## S0: probe → portuale mapping (brief task S0)

### Real's machinery (all in `depgraph.py`)

- `_slot_operator_deps` fill (`_add_slot_operator_dep`, :4142-4148, called from
  `_add_pkg_deps` :3813): **every** `:=` dep (built or not) registers a
  `Dependency{parent, child, atom, want_update}` at walk time.
  `want_update = (not complete_mode) and (arg-chain or --update) and depth-ok`
  (:3805-3809; `_want_update_pkg` :7304).
- `_slot_operator_trigger_reinstalls` (:3089-3132) visits each dep **in order**:
  1. unbuilt atom (`:=`/`:S=`, not soname/built) → `_slot_change_probe` (slot
     move w/o revbump) → **ported** as `slot_operator_slot_change_probe` (#24 S5).
  2. skip unless parent is a built `Package` (`:3110-3113`).
  3. `want_update_probe = dep.want_update or not dep.parent.installed` (`:3118`).
  4. **new-slot arm** (`:3121-3126`): `rebuild_if_new_slot and want_update_probe`
     → `_slot_operator_update_probe(dep, new_child_slot=True)` →
     `_slot_operator_update_backtrack(dep, new_child_slot=…)`.
  5. **same-slot arm** (`:3128-3132`): `want_update_probe` →
     `_slot_operator_update_probe(dep)` → `_slot_operator_update_backtrack(dep)`.
- `_slot_operator_update_probe` (:2576-2800): for each `replacement_parent`
  (available ebuild in parent's slot, :2609): skip downgrades unless
  `_downgrade_probe`; **`_slot_operator_check_reverse_dependencies`
  (:2472) gates both the replacement parent (:2622) and each candidate child
  (:2738)** — a parent whose atom the candidate violates vetoes the whole
  replacement (relaxation: built `:S/SS=` parents are checked as `:=`,
  :2494-2502). Child candidates must differ in slot (new-slot arm) or sub-slot
  (same-slot arm) and be higher-version (downgrades need `_downgrade_probe`).
- `_slot_operator_update_backtrack` (:2400-2452): writes
  `slot_operator_mask_built` masks (v2 #24c) + `slot_operator_replace_installed`
  reinstall set (v2 #24b scheduling) + `_need_restart`.
- Further gates: probe-from-slot-conflict (:8786 via
  `_slot_operator_update_probe_slot_conflict` + `_slot_conflict_backtrack_abi`
  :2282 — v2 #24e/#214); in-walk unsatisfied probe (:3447-3458 — v2 #24f/#215);
  `prune_rebuilds` restart (:5763-5780 — v2 #24d/#213).

### What portuale already stands in for it

- `slot_operator_rebuild_scan` (`rust/portage-repo/src/lib.rs:15154`): post-pass
  vdb scan keyed on `Upgrade`/`Downgrade`/`Reinstall` entries (`new_slot` map).
  Covers the **same-slot** arm's outcome (bound `S` == new `S`, bound `SS` !=
  new `SS` → schedule + `abi_rebuilds` pair) for complete-mode-reachable,
  non-in-graph consumers — without the probe's replacement-parent search and
  without the `check_reverse_dependencies` refusal.
- `Backtracker` replace set → `run_pass` seed + flip (S3, `lib.rs:25468-25520`):
  exactly real's `@__auto_slot_operator_replace_installed__` scheduling half —
  the probe's result can reuse it unchanged.
- `built_slot_operator_rebuild_trigger` (`lib.rs:14609`, #25 S1b-fix): the
  check's **relaxation half** (`Atom.with_slot("=")`, :2494-2502) as a
  record-site forgiveness — not a probe gate. This is why the r25 shape
  withholds (version veto still records → direct solve drops 2.0 → no
  `Upgrade` entry → scan sees nothing) while the slotop matrix upgrades.
- `reverse_dependency_constraints` (`lib.rs:14729`): installed-consumer parent
  atoms as vetoes — the check's *unrelaxed* shadow for non-slot-op pins.

### Exactly which real sub-rule is missing (in real's order)

- **R1 (first target): the new-slot arm (:3121-3126).** The scan's provider
  filter requires `a_slot == n_slot`, so a provider that moves slot
  (`somassb` `1` → `2/2`) never schedules its installed built consumers.
  Live shape: `test_oracle_slotop_conflict_mass_rebuild` (bug 486580,
  `somassa -uD --backtrack 3`): portuale merges `{somassa-1, somassb-2}`,
  real merges those plus the 5 leaves. Verified red on current `main`-branch
  worktree binary (strict-xfail holds; `--runxfail` shows exactly the 5 leaf
  cpvs missing).
- R2 (remainder of #24b): `check_reverse_dependencies` as a *refusal* gate on
  both arms + the `want_update`-at-registration nuance (arg-chain half) +
  `_downgrade_probe` interaction. No hermetic pin diverges on it today; the
  r25 (`<2.0:0/1=` veto) and slotop-matrix shapes are decided upstream of the
  scan (record site / S1b predicate), so they cannot regress from R1.
- Out of scope (own numbers): R3 mask side-effects (#212/#24c), R4 prune
  (#213/#24d), R5 conflict-fired probe (#214/#24e), R6 unsatisfied probe
  (#215/#24f), R7 per-dep `_slot_operator_deps` registration (architecture;
  the vdb+entries triple carries the same data).

### Commit proposal (at most 3 — proposing 2, pins + product)

- **C2 (pins, pmtest first): un-xfail `test_oracle_slotop_conflict_mass_rebuild`**
  (fixtures already EAPI 8 — `somass*` — no new fixtures) + extend the Rust
  `slot_operator_rebuild_scan` unit tests with the slot-move shape.
  Paired commit per rule 8.
- **C1 (product, portuale): new-slot update arm in `slot_operator_rebuild_scan`.**
  Second filter beside the same-slot one, keyed on **`New` entries**
  (`new_slot_fresh` map — a provider that moves slot only ever arrives as
  `New`, since `Upgrade`/`Downgrade` require an installed instance in the
  same main slot, `lib.rs:14041-14072`): installed consumer with a built
  `:S/SS=` atom whose bound slot differs from the fresh slot → insert into
  `scheduled` + push the `(provider_cpv, consumer_cpv)` display pair.
  Gates (mirroring real): a superseded installed child in the bound slot
  (else no `dep.child`); fresh version not older via `vercmp` (real's
  `pkg < dep.child` skip — downgrades never probe); `ctx.update` or
  provider in `top_level_cps` (stands in for `want_update`'s
  `arg-chain or --update`, `:3805-3809`; registration-time
  complete-mode/depth nuance documented, not modelled); consumer has a
  visible tree candidate (a `replacement_parent` exists, else the forced
  reinstall would dead-end); existing call-site gates unchanged
  (`backtrack_max > 0`, `rebuild_if_new_slot` — which is exactly the
  new-slot arm's own gate, `:3121`). Walked consumers qualify even with an
  empty `reachable` set (real's gate is `want_update`, not reachability;
  the empty-world conflict-mass shape needs this). Merge-bound consumers
  stay skipped by the existing `in_graph` rule (their live `:=` re-binds
  through their own walk; real's not-installed-parent disjunct would probe
  them, but the refusal half is v2 remainder either way). The `--solver=`
  bridge keeps the old 7-arg behaviour (new arm off — frozen).
  Narrowings documented in-code: no `check_reverse_dependencies`
  refusal (R2); `Upgrade`/`Downgrade`/`Reinstall` entries with a
  bound-slot mismatch are not probed (only `New` feeds the fresh map).

No design decision needed from the owner: every fork above is settled by
real's text (`:3118-3126`, `:3805-3809`) or by an existing red pin.

---

# Continuation — #211 remainder (R2, mismatch arm, I1) + #25 G13b verdict
(2026-09-28, same worktree pair, branch `backlog/211-slotop-update-probe`)

## Status: Part 1 DONE (BED-PENDING — coordinator runs the beds);
## Part 2 (G13b) NEEDS_CONTEXT — design decision for the owner (§5).

## §1. Part 1 commits (paired, pmtest first; one rule per commit)

- pmtest `58062bb` — `contract: #211 R2 pin for the update-probe
  refusal` (new EAPI-8 `app-misc/somassveto-1` fixture + the refusal pin;
  also adds `EAPI` files to the C1 conflict-mass pin's ad-hoc vdb — see
  the EAPI finding below).
  portuale `e4c443e8` — `resolver: #211 R2 update-probe refusal via
  check_reverse_dependencies (pmtest 58062bb)`.
- pmtest `b3481a1` — `contract: #211 item-2 pin for bound-slot-
  mismatched upgrade entries` (new EAPI-8 `app-misc/mmprov-{1,2,3}` +
  `app-misc/mmcons-1` fixtures + the mismatch pin).
  portuale `ad8476cd` — `resolver: #211 probe Upgrade/Downgrade/
  Reinstall entries with bound-slot mismatch (pmtest b3481a1)`.
- pmtest `b0cccdd` — `contract: #211 I1 pin for bridge pubgrub
  agreement` (mmprov shape under `--solver=pubgrub`).
  portuale `83814e19` — `resolver: #211 thread update/top_level_cps
  into the solver bridge (pmtest b0cccdd)`.

`READY-FOR-BEDS e4c443e8`, `READY-FOR-BEDS ad8476cd`,
`READY-FOR-BEDS 83814e19` appended to `g211-progress.md` as the brief
requires; no `BEDS …` reply was present at report time, so per common
rule 13 the beds below are coordinator-only (BED-PENDING).

## §2. R2 — what landed and what the one probe showed

`collect_probe_parents` + `probe_refused` (`rust/portage-repo/src/
lib.rs`): the fresh candidate on each scan edge must match every other
in-scope (reachable-or-walked) installed parent's atom, built `:S/SS=`
relaxed through the S1b `with_slot("=")` helper; stale pins skipped
(merge-bound/`already`/scheduled = elimination + `_upgrade_available`,
`Uninstall` entries = bug 612772, `--exclude` newly threaded through
the scan + bridge signatures). Applied on both arms. The `:2622`
replacement-parent gate is vacuous for same-version reinstalls
(documented, not silent); `_too_deep`, the direct-cycle escape and
soname atoms are documented cuts. Pins: Rust
`slot_operator_update_probe_refusal_vetoes_both_arms` (new-slot veto,
same-slot veto, excluded-parent, out-of-scope-parent) + the contract
pin; both watched RED pre-fix (patch dance, no stash/worktree).

The one allowed real probe (single `podman run`,
`localhost/test-portuale:latest`, host-exact `ROOT=/` shape per the #71
lesson, `_b1_` vdb mirrored file-for-file, `PYTHONHASHSEED=0`):

```
===== CONTROL: emerge -p --color=n --backtrack 3 --update --deep app-misc/somassa
[ebuild  NS    ] app-misc/somassb-2 [1]
[ebuild  rR    ] app-misc/somassc0c-1
[ebuild  rR    ] app-misc/somassc1c-1
[ebuild  rR    ] app-misc/somassc2c-1
[ebuild  rR    ] app-misc/somassc3c-1
[ebuild  rR    ] app-misc/somassc4c-1
[ebuild  N     ] app-misc/somassa-1        (backtrack: 1/3)
===== VETO (world parent app-misc/somassveto-1 pinning <app-misc/somassb-2)
[ebuild  NS    ] app-misc/somassb-2 [1]
[ebuild  N     ] app-misc/somassa-1        (backtrack: 0/3)
```

Full log `/tmp/opencode/g211b/probe-out4.log` (plus `probe.sh`; the
first two stagings are kept as `probe-out.log`/`probe-out2.log`:
`ROOT=$FX` splits the cascade across roots, and EAPI-less vdb
silences the probe — see below).

EAPI finding (load-bearing, changes a prior pin): with EAPI-less vdb
the control merges NO leaves — real's `FakeVartree` overlay refuses
EAPI-less records, so the leaves' built `:1/1=` dep is never
registered and the probe never fires. The C1 pin staged EAPI-less vdb
while citing the EAPI-5 Playground oracle: that pin pinned a shape
live real answers differently. Both pins now stage `EAPI=8` vdb
(portuale is EAPI-indifferent here — the scan reads raw vdb — so its
answers did not move). Second: the veto must live in the parent's
LIVE ebuild (`somassveto-1` now carries `RDEPEND="<app-misc/somassb-2"`;
r25consumer precedent) — real only sees parent pins through the walked
depstring + the built-`:=` overlay, never raw vdb.

## §3. Item 2 — mismatch arm (no probe left; code-grounded)

The new-slot arm now tries fresh (`New`) entries first, then
`Upgrade`/`Downgrade`/`Reinstall` entries in other slots (first
acceptable candidate wins; all gates + R2 refusal per candidate).
Real's candidate loop (`_iter_similar_available`, `:2660-2695`) has no
entry-kind restriction — that text is the whole grounding (the probe
budget went to R2; the direction follows the R2-validated arm).
Entries at the bound slot stay with the same-slot arm (unit-pinned
non-leak). Pins: Rust
`slot_operator_new_slot_arm_probes_bound_slot_mismatched_entries` +
the `mmprov` contract pin, both watched RED pre-fix. Incidental unit
finding, kept: `vdb_aux_get` serves a rewritten field file until the
package dir mtime changes (real's own staleness property) — the first
draft of the mismatch test rewrote `RDEPEND` in place and read stale
data; the test now uses two consumer packages instead.

## §4. I1 — threaded, with measured agreement limits

Threading was possible (the g211 "no context at this layer" note is
stale): `ResolveRequest` carries `update` + `atoms`, so the bridge
fixpoint now takes both (same construction as
`ResolveCtx::top_level_cps`) and runs the full new-slot arm + refusal.
Unit-pinned (`slot_operator_bridge_entries_run_the_new_slot_arm`).
End-to-end on ad-hoc ROOTs (worktree release binary): the `mmprov`
shape agrees byte-for-byte under `--solver=pubgrub` (new contract
pin; verified RED pre-threading), including the causing-rebuilds
block. Two engine-side gaps below this layer, for the coordinator to
file: (a) on the conflict-mass shape both engines resolve walked `:=`
deps to the installed instance (no entry for the scan — agreement
needs engine-side probe scheduling); (b) `resolvo` resolves neither
the conflict-mass deps nor the direct `somassb` upgrade arg at all
(empty mergelist, rc 0).

## §5. G13b — NEEDS_CONTEXT (no commit; nothing flipped)

The literal `_minimize_children` port cannot satisfy "matrix stays
green + r25 goes silent" without machinery this brief does not scope.
Evidence, all verified on the worktree release binary (not assumed):

- E1 (selection-driven upgrade): slotop argument shape
  (`-uDvN --oneshot dev-libs/consrdep`, ad-hoc ROOT, EAPI-faithful
  vdb) merges `U provpkg-2.0 + rR consrdep-1.0`; `--debug` shows the
  live `provpkg:=` resolving to the **merge** 2.0 while the recorded
  `>=provpkg-1.0:0/1=` resolves to installed 1.0, with the S1b-fix
  forgiveness the only reason the Upgrade entry survives. The upgrade
  exists purely because portuale's `:=` selects highest-tree; real's
  binds installed and re-upgrades via the probe.
- E2 (deterministic kill): applying real's elimination
  (`:4809-4832`) to that observed pair — cp `provpkg` with pkgs
  {2.0 (atoms: {`:=`}), 1.0 (atoms: {recorded})}, installed-first —
  eliminates 2.0 (bare `:=` matches installed 1.0) and keeps 1.0
  (recorded matches only 1.0). Both atoms repoint at installed: no
  `Upgrade` entry, so every post-pass scan (same-slot arm, R1 new
  arm, R2, item 2 — all keyed off entries) sees nothing. The argument
  cells (15 bed cells) go from `{U provpkg-2.0, rR…}` to `{}`.
  The `@world` cells would survive (provpkg's own world-seed atom is
  single-atom, minimizer-vacuous, so its Upgrade entry stands and the
  scan still rebuilds) — the regression is exactly the
  non-seeded-provider cells, i.e. r25c's prediction made precise.
- E3 (no collapse point): the walk has no per-parent atom-set
  grouping to port the function onto — `enqueue_dependencies`
  resolves through `use_reduce_flat_disjunctive` + per-atom
  `disjunction_preference`; (atom→pkg) pairs never meet as sets.
  Building the grouping + repointing is new selection architecture,
  not "one commit + unit test".
- E4 (no within-scope alternative): any selection rule that keeps
  the slotop upgrade while collapsing r25 (same byte-identical atom
  shapes per r25c §2) is veto/want_update-gating real does not have —
  B13 rejected exactly this (option (a)).

What G13b actually needs (option (b) completed): the minimizer PLUS
probe-side upgrade creation — from the collapsed state, force the
provider upgrade + consumer rebuilds exactly when the R2 refusal
passes (real `_slot_operator_update_backtrack`: masks + replace set +
restart; portuale has no upgrade-forcing path — its scans only
schedule rebuilds off entries). That is a multi-commit design (how to
force an upgrade in portuale's loop: seed? select-override? masks?),
beyond this brief. Options for the owner: (i) scope that project
(separate brief); (ii) close G13b without S1c (document the residue:
selection creates the L-new node; scan+restart+warning follow);
(iii) re-approve veto-gated collapse with new justification. S2/S3
are untouched (S2's assertions fail today by design — r25 shape
re-verified below — and S3's doc edits depend on the outcome); #25
and #107 not flipped.

r25 status quo (ad-hoc ROOT, committed ebuilds+vdb, post-Part-1
binary): `U r25up-2.0 + N r25target-1.0` + the skipped-update warning,
`--json` `restarts: 1` — unchanged from r25c. #107 live host probe
(2026-09-28, worktree binary, `emerge -puD --getbinpkg --color=n
net-libs/rest`): `md4c U + rest N[ebuild]` + the libdisplay-info
warning block, restarts 1; real (`docs/evidence/2026-09-27-107-
reprobe/real-full.log`): `md4c U + rest N[binary]`, silent,
`backtrack: 0/20`. Still differs (warning + restart + the #192
`[ebuild]`/`[binary]` row). Log `/tmp/opencode/g211b/107-ptl.log`.

## §6. Verification (release profile throughout)

- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`:
  zero warnings (two new `#[allow(clippy::too_many_arguments)]`, same
  precedent as the scan).
- `cargo test --release` (whole workspace) after each product commit:
  R2 `1775/0`, item 2 `1776/0`, I1 `1777/0` (logs
  `/tmp/opencode/g211b/cargo-test-{r2,mm,br}.log`).
- Regression set after each commit (`slot_operator|slotop|keeper|
  needer|whpin|oracle_90|oracle_91|socc|undo|installed_consumers|
  rebuild|r25`): 61, 62, 63 passed respectively, 2 pre-existing
  xfails, 0 failed.
- Full pmtest suite at the end: **2087 passed / 37 skipped /
  5 xfailed / 0 failed** (log `/tmp/opencode/g211b/pmtest-full.log`,
  basetemp `/var/tmp/pmtest-g211b-full`). Corpus drift: NONE (do NOT
  bless — nothing to bless).
- Private basetemaps used for every pytest run (`/var/tmp/pmtest-
  g211b-*`); `pmtest/3rdparty/portage` present.

## BED-PENDING (coordinator only, from the pmtest worktree)

1. After `e4c443e8`: `FX_SLOTOP_BDEP=1 FX_HOST_ROOTS=1
   differential-test-bed/run/l0-fixture-oracle.sh
   differential-test-bed/atomlists/l0-fixture-oracle-slotop.txt` —
   expect 0 unexplained (R2 only withholds where a veto parent exists;
   the matrix has none; the refusal unit test pins the veto shape).
2. After `ad8476cd`: same slotop list — expect 0 unexplained (the
   mismatch arm needs a U/D/R entry in a non-bound slot; single-slot
   providers never produce one) + the full
   `run/l0-fixture-oracle-all.sh` (new `mmprov`/`mmcons`/`somassveto`
   ebuilds must not perturb other cells).
3. After `83814e19`: same as (2) (bridge default-path output
   untouched; `--solver=` paths are not bedded).
4. Always: `differential-test-bed/run/l0-resolver.sh` — expect
   identical-or-better vs the last green L0 row (Track G guard).
