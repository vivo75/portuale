# Backlog #24 — slot-operator rebuild oracle

Method: `docs/023-oracle.md` (upstream `ResolverPlayground` mergelist =
oracle; portuale's output pinned; divergence under `xfail(strict=True)`
where it diverges). Fixture shape throughout this file = the a522084
shape as `_b1_root` builds it
(`tests/test_emerge_pretend_contract.py::test_oracle_non_slot_operator_update_selects_new_slot`):
world `app-misc/A`; vdb `A-1` (`0/1`, `PDEPEND=app-misc/B`), `B-0`
(`RDEPEND=app-misc/A:0/1=`); tree `A-1`, `A-2` (`0/2`), `B-0`
(`RDEPEND=app-misc/A:=`). `PORTAGE_CONFIGROOT=fixtures`. Real refs =
vendored `3rdparty/portage/lib/_emerge/depgraph.py` unless stated.

## Baseline (S0, 2026-09-12, `main` @ `c7ad477` + plan doc `b2515b2`)

Live re-run, Rust == Python asserted on all three commands
(`rust.stdout == python.stdout` and stderr, byte-for-byte). The `@system`
rows (`dev-libs/newpkg-1.0`, `upgradepkg-2.0`, `systempkg-1.0`,
`withdeps-1.0`, all `N`) come from the fixture `@system` set and are
identical in every run; only the `app-misc` rows below carry signal.

| command | portuale (S0) | real (upstream test) | verdict |
|---|---|---|---|
| `emerge -p -uD @world` | `[ebuild     U  ] app-misc/A-2 [1]` (no `B-0`) | `[A-2, B-0]` (`test_solve_non_slot_operator_slot_conflicts.py`, bug 522084) | **DIVERGENT — complete gate** (closes in S1) |
| `emerge -p -uD --complete-graph @world` | `[ebuild  r  U  ] app-misc/A-2 [1]`, `[ebuild  rR    ] app-misc/B-0` + "causing rebuilds" block | `[A-2, B-0]` | MATCH |
| `emerge -p app-misc/A` | `[ebuild  rR    ] app-misc/B-0` *before* `[ebuild  r  U  ] app-misc/A-2 [1]` | `[A-2, (B-0, C-0)]` (`test_slot_operator_rebuild.py` case 1, minus the `\|\|`-arm `C-0`) | **DIVERGENT — order** (S3 acceptance bar) |

Raw S0 outputs (merge lines only; `@system` `N` rows elided):

- `-p -uD @world`: `[ebuild     U  ] app-misc/A-2 [1]`
- `-p -uD --complete-graph @world`:
  `[ebuild  r  U  ] app-misc/A-2 [1]`,
  `[ebuild  rR    ] app-misc/B-0`,
  plus `The following packages are causing rebuilds:` with
  `(app-misc/A-2:0/2::testrepo, ebuild scheduled for merge)` causing
  `(app-misc/B-0:0/0::testrepo, ebuild scheduled for merge)`.
- `-p app-misc/A`: same two rows + same block, but `B-0` **first**.

### S0 inventory — replace-installed-shaped state (for the S3 PR)

`graft` does not index `rust/portage-repo/src/lib.rs` (31k lines, outside
the 66-file graph), so call sites below are `grep`-grounded, not
`graft callers`-grounded.

| site | file:line (S0) | scope | note |
|---|---|---|---|
| `slot_operator_rebuild_entries` def (the scan) | `rust/portage-repo/src/lib.rs:11908` | per-`assemble_result` call (post-settle) | returns `(Vec<GraphEntry>, abi pairs)`; internal `scheduled`/`new_slot` fixpoint is per-call only |
| `Reinstall { slot_operator_rebuild: true }` construction | `lib.rs:12046-12082` | per emitted entry | `deps: Vec::new()`, `required_by: []` — the S3 cut |
| `assemble_result` call + `.extend` | `lib.rs:18361-18368` | assembly (after settle, before merge-order sort) | gated on `!ignore_built_slot_operator_deps && rebuild_if_new_slot` |
| in-walk `reinstall_atoms` flip (`AlreadyInstalled` → `Reinstall`) | `lib.rs:16052` inside `run_pass` (`lib.rs:15848`) | per-pass, per-node | the hook S3 reuses; `slot_operator_rebuild: false` today |
| `GraphEntry::deps` doc (array-position fallback) | `lib.rs:11446` | merge order | synthetic entries fall back to array position + `required_by` |
| `BacktrackParams` | `lib.rs:15520` | cross-pass | no replace-installed set today; S3 adds `slot_operator_replace_installed` + `slot_operator_undone` latch, covered by `params_equal` (`lib.rs:15088`) |
| `BacktrackFeedback` | `lib.rs:15469` | cross-pass | Config / SlotConflict / MissingDep; S3 feeds a Config node through `collect_feedback` (`lib.rs:18053`) |
| Python mirror: `_slot_operator_rebuild_entries` def | `python/emerge_pretend_reference.py:8737` | per-call | "cuts and all" — mirrors the Rust cuts |
| Python mirror: call site | `python:13612` | assembly | inside `resolve_pretend_graph`, same gate |
| Python mirror: params/feedback | `python:11375` (`_params_equal`), `python:11403` (`_bt_add`), `python:11438` (`_bt_feedback_config`) | cross-pass | S3 mirrors the new set here |
| Python mirror: complete gate | `python:21185-21195` (`not deep and …`) | CLI phase-2 guard | S1 mirrors the Rust fix here |
| `slot_op_reachable` computation | `lib.rs:14984` (`ResolveCtx`), `solver_bridge.rs:657`, `python:13604` / `python:16976` (`_required_set_reachable_cps`) | per-resolve | computed from `complete_seed_atoms` regardless of `complete`; empty when phase 2 skipped |

Fixpoint-loop `continue`s in source order (`lib.rs:11951-11983`):
1. `in_graph || scheduled || !reachable` (line 11955) — per-pass dedup + reachable gate;
2. no stale `:S/SS=` provider (line 11981) — atom parse / slot-match filter.

### S0 inventory — the nine undo rules (`_eliminate_rebuilds` 3859-4000) vs portuale

| # | rule | portuale S0 |
|---|---|---|
| 0 | skip under `--emptytree` / live slot conflict (bug 922038) | missing (S4: skip conditions) |
| 1 | same cpv installed (real upgrade never undone) | missing |
| 2 | pkg not in `_reinstall_nodes` (`--newuse`/`--changed-use` wins) | missing (S4 rule 2 via `changed_flags`) |
| 3 | `--changed-slot` keep (3898-3899) | missing (S5; `slot_changed` ships standalone at `lib.rs:7444`) |
| 4 | every `required_by` parent atom matches installed | missing |
| 5 | non-`selective`: no user-asked parent | missing (`ctx.selective` + `top_level_cps` available) |
| 6 | installed and new `(slot, sub_slot)` equal | missing |
| 7 | built `provides`/`requires` equal (soname) | non-goal (N/A — no binpkgs in this family) |
| 8 | dep-string comparison, `:=` bound against the graph (3935-3970) | missing (S4 binder; `bind_slot_operator` exists only in `rust/portuale/src/ebuild_phases.rs:1132`, against vdb) |
| 9 | demote: re-add installed, `_remove_pkg`, re-serialise | missing (S4 demote + latch) |

### S0 pin list — every pin S3/S4 may move, with current expected output

- `test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer`
  (`tests/...:4140`): `[r  U] slotbindtarget-2.0 [1.0]`, `[rR] slotbindconsumer-1.0`,
  "causing rebuilds" block. Order: target first (named-atom walk).
- `test_ignore_built_slot_operator_deps_suppresses_the_rebuild`
  (`:4191`): `[U] slotbindtarget-2.0 [1.0]` only, `abi_rebuilds: []`.
- `test_slot_operator_rebuild_cascades_through_a_multi_level_chain`
  (`:4258`): `[r U] casctarget-2.0 [1.0]`, `[rR] cascmid-1.0 [1.0]`,
  `[rR] casctail-1.0`, two "causing rebuilds" edges.
- `test_oracle_non_slot_operator_update_selects_new_slot` (`:15211`):
  `[U] app-misc/A-2 [1]`, no `B-0` (second assertion flips in S1).
- Rust unit `slot_operator_rebuild_entries_flags_only_the_stale_bindings`
  (`lib.rs:29052`): only stale `:2/2=` rebuilds; empty entries / empty
  reachable → empty.
- Rust unit `ignore_built_slot_operator_deps_skips_the_rebuild_scan`
  (after `required_set_reachable_cps_*`): consumer rebuilt by default,
  dropped with the flag.
- No `CASES` rows name `slotbind*`/`casc*`/`revdepslot*` fixtures
  directly (those shapes are pinned by the dedicated test functions
  above); `subslot*` rows (`:335`, `:531`) and `changedslotpkg` rows
  (`:1749-1774`) pin neighbouring atom/flag behaviour and must not move.
- `--backtrack=0/30`, `--ignore-built-slot-operator-deps`,
  `rebuild_if_new_slot` rows: behaviour-neutral in S1 except the a522084
  flip; any other move is a defect.

### S2 oracle table (filled by S2 2026-09-12; S0 headers replaced)

Prior art (why the cuts were one piece): `docs/history/
slot-op-rebuild-cascade-plan.md` (2026-09-03, COMPLETE) shipped the
reachability gate (`required_set_reachable_cps`), the tree-`SLOT`
fixpoint cascade, the `r` marker and the "causing rebuilds" render,
container-verified byte-for-byte -- and kept one documented cut: a
rebuild's own `RDEPEND` is never re-walked. `what-this-proves.md`
Increment 4 pins the same shape (`slotbindconsumer`, `casctarget`
chain). So v1 is all reconciliation (route the rebuild through the
`Backtracker`, S3) + undo (`_eliminate_rebuilds`, S4) + the slot-change
contact (S5); the update-probe family stays v2.

Method: upstream test → `_b1_root` case (+ committed
`fixtures/repo/…` ebuilds + `metadata/md5-cache`, listed per row),
one `test_oracle_slotop_*` each, Rust == Python asserted now, real's
mergelist as expectation under `xfail(strict=True)` where divergent.
`so*`/`soc*` renames mark shapes whose upstream names would collide
with an existing fixture package carrying different deps.

| case | upstream file | mechanism | S2 status | expected v1 status |
|---|---|---|---|---|
| a522084 | `test_solve_non_slot_operator_slot_conflicts.py` | complete gate | MATCH (S1) | done |
| rebuild-1 | `test_slot_operator_rebuild.py` case 1 (`emerge A`, `--dynamic-deps=n`; bug 522652) | order + `\|\|`-wrapped `:=` | DIVERGENT-order (set right, incl. the `\|\|`-arm `C-0`) | MATCH after S3 |
| rebuild-2 | same, case 2 (`--usepkg`, binary `E-1` at `F:0/1`) | `slot_operator_mask_built` (bug 652938) | NOT TRANSLATABLE (needs a binpkg with built `F:0/1=` metadata + `--usepkg` mask-built path; no ad-hoc binpkg machinery in `_b1_root` tests) | v2 `#24c` |
| unsat-439694 | `test_slot_operator_unsatisfied.py` | in-walk unsatisfied arm | case 2 MATCH, case 1 strict-xfail (S1) | case 1 → v2 `#24f` (new item, S1 finding) |
| slotchange-1/4 | `test_slot_change_without_revbump.py` cases 1 (`--oneshot`, ebuild variant), 4 (`-uD --changed-slot`) | `_slot_change_probe` + `--changed-slot` (bug 456208) | case 1 strict-xfail; case 4 MATCH (sets+markers via standalone trigger; S5 owns routing); case 2 (`--noreplace` → `[]`) MATCH | cases 1+4 MATCH after S5 (ebuild variant; binary halves → v2 `#24c`) |
| regslotchange | `test_regular_slot_change_without_revbump.py` (`soslotconsumer --oneshot --usepkg`) | same, main slot (renamed: `dev-libs/boost` taken by 023 oracle) | strict-xfail | MATCH after S5 (ebuild variant) |
| complete | `test_slot_operator_complete_graph.py` (bug 614390; `=socmeta-2 socc --backtrack 9`) | complete mode + cascade + undo | strict-xfail (named `socc` out-selects meta's `=socc-1` pin -- #36 overlap) | **S4 acceptance** |
| revdeps | `test_slot_operator_reverse_deps.py` (bug 584626; + ignore-built variant) | selection + scan (no probe needed on this shape) | MATCH both | done (probe family stays v2 `#24b`) |
| revdeps-libgit2 | same file (bug 717140, `-uD` → `[]`) | must-not-downgrade guard | MATCH (guard) | stays `[]` through v1 |
| parentdown | `test_slot_operator_update_probe_parent_downgrade.py` (bug 528610, `-uD` → `[]`) | probe must NOT fire | MATCH (guard) | stays `[]` through v1 |
| missedupd | `test_slot_operator_missed_update.py` (bug 743115, `--backtrack 4`; compacted `soflag` for `python_targets_*`) | `prune_rebuilds` | strict-xfail (upgrades `sosetuptools` instead of holding the rebuild) | v2 `#24d` |
| autounmask | `test_slot_operator_autounmask.py::testSubSlot` ignore-built case (`icu --oneshot`, → `[icu-49]`) | flag contact | MATCH | done (keyword/autounmask/binpkg cases → v2) |
| exclusive | `test_slot_operator_exclusive_slots.py` (bugs 612772, 612874) | slot upgrades + depclean uninstalls | NOT TRANSLATABLE (mergelists carry `[uninstall]`/`!slot` removal ops; `--pretend` has no removal display) | v2 (needs depclean semantics) |
| runtime_pkg_mask | `test_slot_operator_runtime_pkg_mask.py` (`=socmeta-2 --backtrack 14`) | runtime mask + rebuild | MATCH (ambiguous order) | done |
| unsolved | `test_slot_operator_unsolved.py` (ruby cycle, USE-gated solutions) | cycle + probe interplay | NOT TRANSLATABLE without approximating (USE-gated `circular_dependency_solutions`; an approx pin is worse than none) | v2 |
| bdeps | `test_slot_operator_bdeps.py` (`-uD`, + `--usepkg --with-bdeps=y` ebuild-fallback) | `BDEPEND` `:=` rebuild | MATCH both | done (binary-rejection half → v2 `#24c`) |
| required_use | `test_slot_operator_required_use.py` (bug 523048, → fail) | REQUIRED_USE-gated rebuild | strict-xfail (rebuilds instead of reporting) | v2 (new item: REQUIRED_USE gate on forced rebuilds) |
| conflict-rebuild | `test_slot_conflict_rebuild.py` (bug 439688, `-uD --backtrack 4` → `[D-2, E-0]`) | conflict holds `A`, `D` shifts | MATCH (bug 922038 falls out) | done |
| conflict-mass | same file (bug 486580, 5 leaves) | `_slot_change_probe` (main-slot move, renamed `somass*`) + named-atom seeding gap (leaves outside CLI seeds) | strict-xfail | MATCH after S5+S3 (probe + walked node; seeds must cover the walk) |
| missed_update-Qt / blocker | `test_missed_update.py`, `testBacktrackInconsistentForcedRebuildWithBlocker` | — | OUT (upstream `xfail`; blocker + rebuild) | out of scope |
| slotundo-cascade | synthetic (a522084 + consumer tree-`SLOT` bump) | tree-`SLOT` cascade | MATCH | guards `casc*` for S3 |
| slotundo-unnecessary | synthetic (USE-disabled `soflag? ( := )` dep) | rule 8 USE-reduction | strict-xfail (scan reads raw vdb) | flips in S4 |
| slotundo-changed-slot | synthetic (consumer same-version SLOT move) | rule 6 (no flag) / rule 3 (flag) | no-flag MATCH (guard vs over-undo); flag strict-xfail (loses `r` + edge via standalone trigger) | S4 keeps guard; flag display flips in S5 |
| slotundo-rebind | synthetic (consumer gains `sounewdep`) | walked node edges | strict-xfail (`--json` duplicate rows) | flips in S3 |

S2 corrections to the plan (surfaced, not defaulted):

- **unsat case 1 is not S1's** (S1 commit): needs the in-walk
  `_slot_operator_unsatisfied_probe` (`depgraph.py:3447-3454`); filed
  as v2 candidate `#24f` (probe + backtrack; needs #25 nomerge nodes).
- **revdeps cases 1-2 MATCH without any probe** (highest-first
  selection + S1-gated scan suffice on this shape); the plan's blanket
  "v2 — DIVERGENT" holds for the probe *mechanism* (`#24b`), not these
  mergelists.
- **Named atoms are not CLI seeds** (mass-rebuild finding):
  `complete_seed_atoms` = world file + `@system` only; real's required
  sets are args ∪ system ∪ world (`_complete_graph` docstring). With an
  empty world, `emerge somassa` seeds nothing and the scan skips walked
  leaves. S3 (scan inside `collect_feedback`) must seed from the walk's
  own graph, not just the CLI seeds.
- **Rule 6 subsumes rule 3 for ebuilds** (read against
  `_eliminate_rebuilds` 3859-4000 + `_changed_slot` 3247-3252 +
  `_equiv_ebuild` 7390): for an ebuild consumer, rule-3-trigger (flag +
  same-cpv slot move) ⟺ rule-6-keep, so `--changed-slot` never flips an
  ebuild shape by itself -- the S5-observable on
  `slotundo-changed-slot --changed-slot` is display-level (`r` + edge
  restored via the replace set). The flag's demote-vs-keep flip needs
  binaries (v2).
- **Rebuild-2, exclusive, unsolved, slotchange/regslotchange binary
  halves**: labelled per the stop condition (not approximated).

## S1 — complete-mode gate under `--deep` (2026-09-12)

Fix (plan §S1 option (b)): `rust/portuale/src/pretend.rs` (`run_resolve`
gains `with_seeds`; phase-2 guard restructured) + mirror in
`python/emerge_pretend_reference.py` (`_run_resolve` gains `with_seeds`).
When `--deep` is in force, phase 2's locked-gate re-resolve stays
skipped, but whenever `complete_graph_auto_enable` fires on the phase-1
entries the phase-1 walk is re-run with only `complete_seed_atoms` fed
in (they feed `slot_op_reachable` and nothing else --
`ResolveCtx::new`, `solver_bridge.rs:657`). Deterministic input, so the
walk is identical; the only delta is the slot-operator scan now seeing
reachability. Verified live: `-uD @world` on the baseline shape now
prints `[ebuild  r  U  ] app-misc/A-2 [1]`, `[ebuild  rR    ]
app-misc/B-0` + the "causing rebuilds" block -- byte-identical to
`-uD --complete-graph @world`, Rust == Python on all probes (including
`--nodeps`, which stays seed-less per real's `"recurse" not in myparams`
early return). Third-arm check done: portuale passes
`rebuild_if_new_slot` as `if_new_slot`, matching real's
`complete_if_new_slot = rebuild_if_new_slot` (`depgraph.py:8590-8592`).

Judgment calls surfaced (not defaulted):

- **The `test_slot_operator_unsatisfied.py` case 1 (`-uD @world` →
  `[B-0]`, bug 439694) does NOT flip in S1**, contra the plan's "MATCH
  after S1" cell. Verified live (even `--complete-graph` prints no
  `B-0`): with no version change anywhere, no merge entry fires
  auto-enable, so a seeded re-run has nothing to scan. Real reaches
  `[B-0]` through the **in-walk** `_slot_operator_unsatisfied_probe`
  (`depgraph.py:3447-3454` → `_slot_operator_unsatisfied_backtrack`),
  which no post-walk detector can reproduce. Pinned as
  `strict=True` xfail (`test_oracle_slot_operator_unsatisfied_rebuilds_the_stale_consumer`)
  and filed as a v2 item (candidate `#24f`: in-walk unsatisfied built
  slot-op probe + backtrack; needs #25 nomerge nodes to see the flawed
  parent in-graph). Case 2 (`--oneshot A` → `[A-2]`, no rebuild) MATCHES
  and is pinned passing.
- **No new Rust unit test**: the gate is CLI-layer branching in both
  languages with no new helper; coverage comes from the `CASES` row
  (shared-fixture `-uD @world` exercises the seeded re-run path on both
  sides -- output-neutral there, no stale consumer in shared fixtures)
  plus the flipped a522084 pin and the two unsat pins, all asserting
  Rust == Python empirically. The existing
  `complete_graph_auto_enable` unit tests cover the trigger.

Pins flipped: `test_oracle_non_slot_operator_update_selects_new_slot`
(second assertion → both `r`-tagged rows in real's `[A-2, B-0]` order +
block); `docs/023-oracle.md` a522084 row PARTIAL → MATCH;
`docs/backlog-tasks.md` #24 line notes S1 shipped.

S1 also closed two latent divergences the new gate exposed:
`test_oracle_boost_subslot_upgrade` and
`test_oracle_virtual_subslot_upgrade_avoids_missed_update` asserted
`libcmis`/`DBD-mysql` silence while quoting upstream mergelists that
merge them -- both now print the full upstream lists (`r`-tagged
provider + `rR` consumer + block). Pins updated to the oracle lists;
`docs/023-oracle.md` boost/virt rows corrected from "MATCH (versions)"
to full MATCH.

L0 stop-condition check: `TEST/logs/l0-20260911T234323Z` vs the S0
archive `l0-20260911T231643Z` -- parity 0.800 both, findings JSON
identical, raw `portuale/` outputs byte-identical (`diff -rq` clean).
Stronger than the stop condition allows (zero added rows: no stale `:=`
consumer among the L0 probes); no regression.

L0 baseline: archived under `TEST/findings/` by the S0 commit (raw
`l0-resolver.sh` output on `main`, pre-S1).
