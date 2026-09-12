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
| rebuild-1 | `test_slot_operator_rebuild.py` case 1 (`emerge A`, `--dynamic-deps=n`; bug 522652) | order + `\|\|`-wrapped `:=` | **MATCH (S3)** -- `[A-2, B-0, C-0]` | done |
| rebuild-2 | same, case 2 (`--usepkg`, binary `E-1` at `F:0/1`) | `slot_operator_mask_built` (bug 652938) | NOT TRANSLATABLE (needs a binpkg with built `F:0/1=` metadata + `--usepkg` mask-built path; no ad-hoc binpkg machinery in `_b1_root` tests) | v2 `#24c` |
| unsat-439694 | `test_slot_operator_unsatisfied.py` | in-walk unsatisfied arm | case 2 MATCH, case 1 strict-xfail (S1) | case 1 → v2 `#24f` (new item, S1 finding) |
| slotchange-1/4 | `test_slot_change_without_revbump.py` cases 1 (`--oneshot`, ebuild variant), 4 (`-uD --changed-slot`) | `_slot_change_probe` + `--changed-slot` (bug 456208) | case 1 strict-xfail → **MATCH (S5)**; case 4 MATCH (sets+markers via standalone trigger; S5 owns routing); case 2 (`--noreplace` → `[]`) MATCH | cases 1+4 MATCH after S5 (ebuild variant; binary halves → v2 `#24c`) |
| regslotchange | `test_regular_slot_change_without_revbump.py` (`soslotconsumer --oneshot --usepkg`) | same, main slot (renamed: `dev-libs/boost` taken by 023 oracle) | strict-xfail → **MATCH (S5)** | MATCH after S5 (ebuild variant) || complete | `test_slot_operator_complete_graph.py` (bug 614390; `=socmeta-2 socc --backtrack 9`) | complete mode + cascade + undo | strict-xfail (named `socc` out-selects meta's `=socc-1` pin -- #36 overlap) | **S4 acceptance** |
| revdeps | `test_slot_operator_reverse_deps.py` (bug 584626; + ignore-built variant) | selection + scan (no probe needed on this shape) | MATCH both | done (probe family stays v2 `#24b`) |
| revdeps-libgit2 | same file (bug 717140, `-uD` → `[]`) | must-not-downgrade guard | MATCH (guard) | stays `[]` through v1 |
| parentdown | `test_slot_operator_update_probe_parent_downgrade.py` (bug 528610, `-uD` → `[]`) | probe must NOT fire | MATCH (guard) | stays `[]` through v1 |
| missedupd | `test_slot_operator_missed_update.py` (bug 743115, `--backtrack 4`; compacted `soflag` for `python_targets_*`) | `prune_rebuilds` | strict-xfail (upgrades `sosetuptools` instead of holding the rebuild) | v2 `#24d` |
| autounmask | `test_slot_operator_autounmask.py::testSubSlot` ignore-built case (`icu --oneshot`, → `[icu-49]`) | flag contact | MATCH | done (keyword/autounmask/binpkg cases → v2) |
| exclusive | `test_slot_operator_exclusive_slots.py` (bugs 612772, 612874) | slot upgrades + depclean uninstalls | NOT TRANSLATABLE (mergelists carry `[uninstall]`/`!slot` removal ops; `--pretend` has no removal display) | v2 (needs depclean semantics) |
| runtime_pkg_mask | `test_slot_operator_runtime_pkg_mask.py` (`=socmeta-2 --backtrack 14`) | runtime mask + rebuild | MATCH (ambiguous order) | done |
| unsolved | `test_slot_operator_unsolved.py` (ruby cycle, USE-gated solutions) | cycle + probe interplay | NOT TRANSLATABLE without approximating (USE-gated `circular_dependency_solutions`; an approx pin is worse than none) | v2 |
| bdeps | `test_slot_operator_bdeps.py` (`-uD`, + `--usepkg --with-bdeps=y` ebuild-fallback) | `BDEPEND` `:=` rebuild | MATCH both | done (binary-rejection half → v2 `#24c`) |
| required_use | `test_slot_operator_required_use.py` (bug 523048, → fail) | REQUIRED_USE-gated rebuild | **MATCH (S3)** -- the walked node runs the ordinary `REQUIRED_USE` check the synthesiser bypassed | done (no v2 item needed) |
| conflict-rebuild | `test_slot_conflict_rebuild.py` (bug 439688, `-uD --backtrack 4` → `[D-2, E-0]`) | conflict holds `A`, `D` shifts | MATCH (bug 922038 falls out) | done |
| conflict-mass | same file (bug 486580, 5 leaves) | ~~`_slot_change_probe` (main-slot move, renamed `somass*`)~~ **`_slot_operator_update_probe` (v2 `#24b`)** + named-atom seeding gap (leaves outside CLI seeds) | strict-xfail | **stays strict-xfail** (v2 `#24b`: verified live, the leaves' *built* `somassb:1/1=` deps need the update probe, not `_slot_change_probe`; S5 does not fire) |
| missed_update-Qt / blocker | `test_missed_update.py`, `testBacktrackInconsistentForcedRebuildWithBlocker` | — | OUT (upstream `xfail`; blocker + rebuild) | out of scope |
| slotundo-cascade | synthetic (a522084 + consumer tree-`SLOT` bump) | tree-`SLOT` cascade | MATCH | guards `casc*` for S3 |
| slotundo-unnecessary | synthetic (USE-disabled `soflag? ( := )` dep) | rule 8 USE-reduction | strict-xfail (scan reads raw vdb) | flips in S4 |
| slotundo-changed-slot | synthetic (consumer same-version SLOT move) | rule 6 (no flag) / rule 3 (flag) | no-flag MATCH (guard vs over-undo); flag **MATCH (S5)** after correction -- real runs no undo and shows no `r`/edge here, see the S2 correction below | S4 keeps guard; flag display was mis-expected, S5 pins real's actual shape |
| slotundo-rebind | synthetic (consumer gains `sounewdep`) | walked node edges | **MATCH (S3)** -- one walked `reinstall` row, no `already_installed` duplicate | done |

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
  ebuild shape by itself. **Corrected in S5**: the follow-on claim that
  the flag's observable is `r` + edge ("restored via the replace set")
  was wrong -- verified live against the vendored portage that with the
  flag real's `_eliminate_rebuilds` does not even run on the
  `slotundo-changed-slot` shape (`_forced_rebuilds` empty: the
  consumer's own slot move makes it a plain reinstall, so the S3 scan
  never puts it in the auto-set, no `r` marker, no block). Portuale's
  pre-S5 output already matched; S5 pinned it. The flag's demote-vs-keep
  flip still needs binaries (v2 `#24c`).
- **conflict-mass is the update-probe family, not S5's**
  (S5, verified live with `ResolverPlayground` on the upstream
  `testSlotConflictMassRebuild` shape): every leaf fires
  `_slot_operator_update_backtrack` against its recorded *built*
  `somassb:1/1=` dep -- `_slot_change_probe` only ever sees *unbuilt*
  `:=`/`:S=` deps, and the S5 probe does not fire on this shape. The
  mass case therefore stays strict-xfail under its original v2 owner
  (`#24b`), not "MATCH after S5+S3".
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

## S3 — route the rebuild through the `Backtracker` (2026-09-12)

The architectural slice. A slot-operator rebuild is no longer a
`GraphEntry` synthesised after the search settles; it is a **walked
graph node**, exactly as in real.

**Shape (real citations).** `slot_operator_rebuild_entries` is split
into `slot_operator_rebuild_scan` (`lib.rs`, Python
`_slot_operator_rebuild_scan`), which returns only the consumer
`(cat, pkg)` set plus the `(provider_cpv, consumer_cpv)` display pairs.
It runs in `collect_feedback` / the Python decision chain, last in the
chain, on the settled graph -- real calls
`_slot_operator_trigger_reinstalls` from `_process_slot_conflicts`
(`depgraph.py:2131-2132`) after the walk. A grown set becomes
`BacktrackFeedback::Config` (real
`backtrack_infos["config"]["slot_operator_replace_installed"]`,
`depgraph.py:2400`/`2361`/`2881` → `resolver/backtracking.py::
_feedback_config` 237-257) carried on
`BacktrackParams::slot_operator_replace_installed` (`BTreeSet`, in
`params_equal`). The next pass seeds the walk with a bare `cat/pkg`
atom per member -- real's pseudo `SetArg`
`@__auto_slot_operator_replace_installed__` (`_gen_reinstall_sets`
5457-5480), appended *after* the user's args (`select_files` 5430),
`owner = None` (the set arg has no package parent) and `depth = 1`
(real's `reset_depth=False`: no `--deep=N` depth interaction, and not a
top-level argument, which `depth == 0` means everywhere in the walk) --
and flips that package's `AlreadyInstalled` outcome to
`Reinstall { slot_operator_rebuild: true }` at the same point
`--reinstall-atoms` flips (real's `force_reinstall=True`). Keyed on the
`cat/pkg`, so whichever visit comes first -- the seed or an ordinary
dependency edge -- is the one that flips; patching the entry afterwards
would leave its deps unenqueued. The cascade falls out for free (the
rebuilt consumer is a provider at its *tree* sub-slot on the next
pass), so the internal fixpoint is gone. Narrowing, documented on the
field: real keys `(root, slot_atom)` via `_replace_installed_atom`
(3059-3087); portuale resolves one instance per `cat/pkg`.

**Verdicts (live, `_b1_root`, Rust == Python byte-for-byte on every
probe):**

| case | before S3 | after S3 |
|---|---|---|
| rebuild-1 (bug 522652) | `[B-0, C-0, A-2]` -- array position | `[A-2, B-0, C-0]` = real's `[A-2, (B-0, C-0)]` |
| slotundo-rebind | `already_installed` + `reinstall` duplicate `--json` rows | one walked `reinstall` row |
| required_use (bug 523048) | merged `soreqb-0` | `!!! … has unmet requirements` + rc 1, real's shape |
| a522084, cascade, slotbind, bdeps, revdeps, autounmask, conflict-rebuild, runtime_pkg_mask, parentdown, libgit2 guard | — | unchanged |

**Three pins moved, each with its oracle:**

1. `test_oracle_slotop_rebuild_order`, `test_oracle_slotop_undo_rebind`
   -- strict-xfail → passing (upstream `test_slot_operator_rebuild.py`
   case 1; the S2 synthetic).
2. `test_oracle_slotop_required_use` -- strict-xfail → passing
   (upstream `test_slot_operator_required_use.py`). Not planned: the
   walked node simply runs the `REQUIRED_USE` check the synthesiser
   never consulted. The planned v2 item is therefore **not needed**.
3. `test_oracle_slotop_slotchange_case4_changedslot`
   (`app-arch/libarchive-3.1.1`),
   `test_changed_slot_reinstalls_a_package_whose_vdb_slot_differs_from_the_current_ebuild`
   and `test_changed_deps_and_changed_slot_combine_in_one_reinstall_line`
   (`dev-libs/changedslotpkg-1.0`) -- all three now render the
   `[oldver]` bracket. Real `output.py::_get_installed_best` 721-727: a
   reinstall of an installed cpv is the `vardb.cpv_exists(pkg.cpv)` arm
   (`replace = True`) and carries `myoldbest = [installed_version]`
   exactly when the installed instance's `(slot, sub_slot)` differs
   from the one being merged -- which both shapes are (installed `0/0`
   vs tree `SLOT="0/13"` / `SLOT="0/2"`); `convert_myoldbest` does not
   suppress a same-version bracket. The post-pass synthesiser already
   applied that rule to its own entries; S3 gives it to walked
   reinstalls, where real applies it. Real's third disjunct (`not
   quiet_repo_display and repo differs`) stays out, as it did there.

   The same bracket made one real-renderer branch reachable for the
   first time and it was missing, so it was ported with the pin:
   `convert_myoldbest`'s non-`new_slot` arm appends the *old*
   instance's sub-slot on a second disjunct the entry's own
   `_append_slot` does not have -- `old.slot == pkg.slot and
   old.sub_slot != pkg.sub_slot`. `decorate_version` /
   `_decorate_version` gained a `force_sub_slot` argument, passed only
   from the `oldbest` caller, so `emerge -pv --changed-slot
   dev-libs/changedslotpkg` prints real's
   `[1.0:0/0::testrepo]` rather than `[1.0:0::testrepo]`. Pinned in the
   slot-only `--changed-slot` test.

**New behaviour, deliberate (`--backtrack=0` / `--nodeps`):** the
rebuild is off. Real gates `_slot_operator_trigger_reinstalls` on
`_allow_backtracking` (`depgraph.py:2131`), which `_backtrack_depgraph`
sets from `allow_backtracking = max_retries > 0` (12190) -- with no
search there is no restart to apply the replace set on, so real
schedules nothing and `_forced_rebuilds` stays empty (no "causing
rebuilds" block either). The pre-S3 synthesiser ran regardless of the
budget. Pinned in
`test_slot_operator_rebuild_is_a_walked_node_and_off_at_backtrack_zero`.

**Reachability widened (the S2 mass-rebuild finding):** `slot_op_reachable`
is now seeded from `complete_seed_atoms ∪ atoms`. Real `_complete_graph`
starts its required-set walk from
`args = self._dynamic_config._initial_arg_list[:]` (8677) and *appends*
the `@world`/`@selected`/`@system` set args (8723-8731), so a
directly-requested atom is a seed too. An empty `complete_seed_atoms`
still means "not complete mode" and gates the whole scan off -- the args
alone never enable it.

Judgment calls surfaced (not defaulted):

- **Gate G0.2 answered "unconditional"** by the owner: no
  `PORTUALE_SLOT_OP_GRAPH` flag; the synthesiser is deleted from the
  default path. S6's L0 triage compares against the S0 archive rather
  than toggling a flag.
- **The `--solver=pubgrub` / `--solver=resolvo` bridges keep the
  synthesiser.** `solver_bridge.rs` has no `Backtracker`, so it cannot
  re-drive a pass with the consumer seeded; `slot_operator_rebuild_entries`
  survives there as a documented legacy wrapper around the scan (it
  re-runs the scan to a fixpoint to keep the cascade). The default
  `--solver=portage` path has no synthesiser left.
- **`required_by` on a rebuilt row stays empty**, and `--tree` does not
  indent it. That is faithful, not a gap: real's parent is the internal
  auto set arg, and `_eliminate_rebuilds` rule 5 exists precisely to
  exclude it from the "the user asked for it" test. The merge-order
  evidence is the row's own `deps` (asserted in the Rust unit test) and
  the provider-first order (asserted in three contract pins).
- **`conflict-mass` (bug 486580) stays strict-xfail.** S3 removed the
  seeding half of its blocker (named atoms now seed reachability), but
  the shape still needs S5's `_slot_change_probe` -- a main-slot move
  `1 → 2/2` is not a sub-slot shift, so the scan never fires. Reason
  text left as-is; it already names S5 first.

S3 does **not** implement the undo (`_eliminate_rebuilds`): every
consumer the scan schedules is still kept. That is S4, and the two
`slotundo-unnecessary` / `complete` xfails stay strict.

## S4 — `_eliminate_rebuilds` undo path (2026-09-12)

The retraction half of the family: a consumer the S3 scan schedules is
now demoted when real's `_eliminate_rebuilds` (`depgraph.py:3859-4000`)
would demote it. The nine rules of plan §1.2, in real's order, live in
`portage-repo::slot_operator_eliminate_rebuilds` (+ Python
`_slot_operator_eliminate_rebuilds`), called from `collect_feedback` on
the settle-eligible pass *after* the S3 scan did not grow the set (real
runs it in `_resolve` after `_process_slot_conflicts`). Rule 8 needs
real's graph-aware `:=`/`:S=` binder, ported as
`bind_slot_operator_deps` / `_bind_slot_operator_deps` (real
`portage/dep/_slot_operator.py::_eval_deps` over
`evaluate_slot_operator_equal_deps`'s `_graph_trees`, whose vartree is
the `PackageTrackerDbapiWrapper`, `depgraph.py:744-775` -- merge-bound
entry first, else the installed vdb). Demotion drops the cp from
`BacktrackParams::slot_operator_replace_installed`, latches it in the
new `slot_operator_undone` (`in params_equal`), and returns budget-free
`Config` feedback; the S3 scan takes the latch, so the still-stale vdb
binding cannot re-add it. Skip conditions: `--emptytree` (`ctx.empty`)
and any live slot conflict (real bug 922038). Rule 7 (`provides`/
`requires` for built packages) is not reachable in the ebuild-only v1
(a binary entry keeps its rebuild; v2 `#24c`); rule 3 (`--changed-slot`)
is S5's line, and rule 6 already subsumes it for ebuilds (S2
correction).

### S4 verdicts

| case | before S4 | after S4 |
|---|---|---|
| slotundo-unnecessary | strict-xfail: `[rR] sounneed`, `[r U] souprov`, block | MATCH -- `[U] souprov-2.0 [1.0]`, no `sounneed` row, no block, `abi_rebuilds: []` |
| slotundo-changed-slot (no flag) | MATCH (kept) | MATCH -- rule 6 still keeps the slot-moved consumer (guard against over-undo) |
| complete (bug 614390) | strict-xfail | **strict-xfail -- selection, not undo** (finding below) |
| a522084 `B-0` (bug 522084) | MATCH | MATCH -- rule 8 keeps it (tree `A:=` binds to `A:0/2=`, vdb says `A:0/1=`) |
| rebuild-1 (bug 522652), cascade, slotbind, bdeps, revdeps, parentdown, libgit2 guard, conflict-rebuild, runtime_pkg_mask, autounmask | -- | unchanged (rule 8/6 keep every genuine ABI rebuild) |

### Finding: `complete` is blocked by selection, not by the undo

The S2 table called `test_slot_operator_complete_graph.py` (bug 614390)
the "S4 acceptance case"; live after S4 the undo is *not* what the case
needs. The walk resolves the top-level bare `dev-libs/socc` to `socc-2`;
meta-pkg's later `=socc-1` dep then resolves as `AlreadyInstalled` for
the already-scheduled slot, and that fast path never consults
`resolved_slots` -- real's `_add_pkg` slot-parent check
(`depgraph.py:2160-2185`) turns it into a slot conflict, and the
solvable-conflict feedback enforces `{dev-libs/socc, =socc-1}` →
`socc-1`. The undo rules themselves are right on this shape: rule 5
keeps `socc-1`'s `AtomArg` rebuild, rule 8 keeps `socd-1`/`socb-2`'s
graph-bound `socfoo:=` (`0/2=` vs the vdb's `0/1=`). Pinned as
strict-xfail with the selection reason; the installed-side slot check is
the backlog #36 (mask-aware selection) overlap S2 already named.

Judgment calls surfaced (not defaulted):

- **The triggering provider loses its `r` when the only edge is
  demoted.** Portuale's `force_reinstall` marker comes from the surviving
  `abi_rebuilds` pairs (`pretend.rs` ~10671), and `_compute_abi_rebuild_info`
  drops the edge of a demoted parent exactly the same way (its
  replacement parent is the reinstated installed node, not a
  merge-bound one, so the pair is skipped) -- but real's `r` on the
  provider additionally comes from the trigger having put the
  provider's replacement atom into the auto set. Upstream
  `ResolverPlayground` pins mergelists only, so there is no oracle
  either way; portuale's model is "no surviving rebuild edge, no
  forced-reinstall marker" and the pinned test documents it.
- **Rule 4's atom matching ignores `[use]` deps** (the candidate string
  carries no USE state), the same documented direction
  `reverse_dep_constraint_atom` already takes -- can only keep a
  rebuild, never demote one.
- **Rule 8 compares flat atom sets, not real's structured per-key
  lists** (order/redundant-bracket differences read equal), and strips
  libc atoms after flattening rather than at the outer level only --
  both narrower-than-real demotion risks, matching the plan's own
  `flat_dep_atoms` instruction.

Rust unit tests: `bind_slot_operator_deps_binds_against_entries_then_installed`
and `slot_operator_eliminate_rebuilds_applies_the_eight_rules_in_order`
(rules 0,1,2,4,5,6,7,8 + non-slot-op entries + the latch) on the shared
`fixtures/repo/dev-libs/souprov` pair. Contract test
`test_oracle_slotop_undo_unnecessary` flipped xfail → pinned exact
output (`--json` `abi_rebuilds: []` included).

## S5 — `_slot_change_probe` + `--changed-slot` contact (2026-09-12)

The last detector of the family: real `_slot_change_probe`
(`depgraph.py:2317-2359`), the first arm of
`_slot_operator_trigger_reinstalls` (3103-3107). For an **unbuilt**
slot-operator dep (`:=` / `:S=`, real's `not (atom.soname or
atom.slot_operator_built)`) whose parent is an ebuild scheduled for
merge and whose child the graph resolved to an installed instance, the
tree ebuild at the child's own cpv may carry a different
`(slot, sub_slot)` than the vdb record -- a slot move without a revbump
(bug 456208). Real sees the dep through `_slot_operator_deps`
(registered when the parent's dep was walked); portuale re-reads the
parent entry's tree metadata and use-reduces it against the node's
enabled USE (`flat_dep_atoms`), then requires the pass to have resolved
the child to the installed instance (`AlreadyInstalled`, real's
`dep.child.built`), and calls the existing `slot_changed` for the
tree-ebuild-at-cpv lookup. A hit joins the S3 replace set, exactly
real's `_slot_change_backtrack` (2361-2399) writing
`slot_operator_replace_installed` + `_need_restart`.

Placement: `slot_operator_slot_change_probe` /
`_slot_operator_slot_change_probe` (`rust/portage-repo/src/lib.rs`,
`python/emerge_pretend_reference.py`), called from the tail of
`slot_operator_rebuild_scan`. Deliberately **outside** the `reachable`
gate: real's first trigger arm is not complete-mode gated, and the two
new oracle cases (`--oneshot`, no sets at all) have an empty
`slot_op_reachable` -- the vdb half of the scan stays gated exactly as
before. The `collect_feedback` gates (`--backtrack=0`,
`--ignore-built-slot-operator-deps`, `--rebuild-if-new-slot=n`) still
gate the whole scan including the probe; that is a documented narrowing
vs real (real gaps only the *update-probe* new-slot arm on
`rebuild_if_new_slot`, at 3123) with no oracle in v1.

`--changed-slot` (real 3898-3899) is rule 3 in
`slot_operator_eliminate_rebuilds` now, in real's position between rule
2 and rule 4. As the S2 correction says, it is behaviour-neutral for
ebuilds (rule 6 keeps every consumer whose tree ebuild moved), so the
observable S5 flip is the probe's, not the undo's.

**Verdicts (live, `_b1_root`, Rust == Python byte-for-byte):**

| case | before S5 | after S5 |
|---|---|---|
| slotchange-1 (bug 456208) | `[R] ark-4.10.0` only | `[rR] libarchive-3.1.1 [3.1.1]`, `[R] ark-4.10.0`, no block |
| regslotchange | `[N] soslotconsumer` only | `[rR] soslotlib-1.52.0 [1.52.0]`, `[N] soslotconsumer-4.0.0.2`, no block |
| slotchange-4 (`-uD --changed-slot @world`) | MATCH | unchanged (the child is the standalone `--changed-slot` reinstall, not `AlreadyInstalled` -- real's `dep.child.built` fails) |
| slotundo-changed-slot (no flag / flag) | MATCH / xfail with the wrong `r`+edge expectation | both MATCH real: no flag keeps `[r U]`+`[rR]`+block; flag is a plain `[U]`+`[R]`, no block (real runs no undo there) |
| a522084, rebuild-1, cascade, slotbind, bdeps, revdeps, parentdown, libgit2 guard, conflict-rebuild, runtime_pkg_mask, autounmask, required_use, undo-unnecessary, undo-rebind | -- | unchanged |
| conflict-mass | strict-xfail | **strict-xfail** (v2 `#24b` -- now correctly attributed; S5's probe does not fire) |

Pins moved:

1. `test_oracle_slotop_slotchange_case1`, `test_oracle_slotop_regslotchange`
   -- strict-xfail → passing, pinned to real's exact merge lines (the
   scheduled child carries `r`, the parent does not, and real's
   `_compute_abi_rebuild_info` yields no pair for the child direction,
   so no "causing rebuilds" block).
2. `test_oracle_slotop_undo_changed_slot_flag` -- strict-xfail → passing
   with the *real-verified* expectation: `[ebuild U] souprov-2.0`,
   `[ebuild R] souneedslot-1.0`, no `r`, no block. The S2 expectation of
   `r`+edge was wrong (see the S2 correction above).
3. `test_oracle_slotop_conflict_mass_rebuild` stays strict-xfail with a
   corrected reason naming `_slot_operator_update_probe` (v2 `#24b`).

Rust unit test: `slot_operator_slot_change_probe_schedules_a_moved_installed_child`
-- the fixture `kde-base/ark` -> installed `app-arch/libarchive-3.1.1`
shape (vdb `0`, tree `0/13`) schedules the child; a merge-bound child
entry (the graph chose an upgrade), the `slot_operator_undone` latch,
and a child whose tree ebuild did not move (`souprov`, tree `0/1` vs
vdb `0/1`) all stay unscheduled. The undo-rules unit test gained its
rule-3 assertions (positive: `souneedslot` kept with and without the
flag; negative: `sounneed`'s rule-8 demotion stands with the flag on,
because its tree ebuild did not move).

Judgment calls surfaced (not defaulted):

- **The S2 flag-half expectation was wrong, not portuale.** Verified
  live with `ResolverPlayground`: with `--changed-slot`, real's
  `_eliminate_rebuilds` never runs on `slotundo-changed-slot` and
  `_compute_abi_rebuild_info` is empty; the consumer is rebuilt by the
  standalone `_changed_slot` selection. Pinning the old `r`+edge
  expectation would have been a synthetic divergence.
- **conflict-mass re-attributed** to v2 `#24b` after the live trace
  showed `_slot_operator_update_backtrack` firing per leaf; the plan's
  "MATCH after S5+S3" cell is superseded.
- **`--ignore-built-slot-operator-deps` / `--rebuild-if-new-slot=n`
  still gate the unbuilt probe**, unlike real (real's
  `ignore_built_slot_operator_deps` is a dep-string parse-time strip in
  `_add_pkg_deps` 6033-6038, and the probe arm is not gated by either).
  Kept to avoid moving the pre-existing gated pins; no v1 oracle covers
  the combination.

## S6 — L0 real-tree validation and triage (2026-09-12)

Run `TEST/logs/l0-20260912T120635Z/` (branch @ S5 `69ed4f7` + the
review follow-ups `a7d0ef1`, docs/comment-only), same pinned tree as the
S0 archive (`gentoo` @ `11c58b7a`, `buildovl` @ `3b1df681`, `porttest` @
`aef17684`, profile `default/linux/amd64/23.0/systemd`, portage
`3.0.82.2`): **120 probes, 96 clean, parity 0.800**, `UNEXPLAINED: 46`
-- byte-identical topline and `l0-report.json` to both the S0 archive
(`l0-20260911T231643Z`, pre-S1) and S1's run
(`l0-20260911T234323Z`).

- `portuale/` raw outputs `diff -rq` clean vs both archives: stronger
  than the stop condition, which allowed added `rR` rows under `-uD`
  probes (S1) moving to real's position (S3). No probe in the corpus has
  a stale `:=` consumer, so neither the scan, the probe nor the undo
  fires there -- zero added rows, zero moved rows, zero regressions, and
  zero findings to adjudicate (`known-divergences.yaml` stays empty; the
  46 findings are the recorded clusters).
- `real/` raw outputs differ only in the `Dependency resolution took`
  timing line (every differing file checked); the merge-order timing
  cluster is untouched.
- No S3 rollback flag to remove: G0.2 was answered "unconditional".
- L1 not run: `git diff 5d2e874..a7d0ef1 --stat` names only
  `rust/portuale/src/pretend.rs` and `rust/portage-repo/src/lib.rs`, no
  merge/unmerge/package/fetch/phase code.

Finding log: `TEST/findings/l0.md` §"Slice run: backlog #24 S1–S5
slot-operator rebuild (2026-09-12)".
