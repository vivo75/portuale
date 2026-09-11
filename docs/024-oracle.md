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

### S2 oracle table (filled by S2; headers only in S0)

| case | upstream file | mechanism | S0 status | expected v1 status |
|---|---|---|---|---|
| *(S2 fills this)* | | | | |

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
