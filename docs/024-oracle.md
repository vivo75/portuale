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
| required_use | `test_slot_operator_required_use.py` (bug 523048, → fail) | REQUIRED_USE-gated rebuild | **MATCH (S3)** -- the walked node runs the ordinary `REQUIRED_USE` check the synthesiser bypassed | done (no v2 item needed) |
| conflict-rebuild | `test_slot_conflict_rebuild.py` (bug 439688, `-uD --backtrack 4` → `[D-2, E-0]`) | conflict holds `A`, `D` shifts | MATCH (bug 922038 falls out) | done |
| conflict-mass | same file (bug 486580, 5 leaves) | `_slot_change_probe` (main-slot move, renamed `somass*`) + named-atom seeding gap (leaves outside CLI seeds) | strict-xfail | MATCH after S5+S3 (probe + walked node; seeds must cover the walk) |
| missed_update-Qt / blocker | `test_missed_update.py`, `testBacktrackInconsistentForcedRebuildWithBlocker` | — | OUT (upstream `xfail`; blocker + rebuild) | out of scope |
| slotundo-cascade | synthetic (a522084 + consumer tree-`SLOT` bump) | tree-`SLOT` cascade | MATCH | guards `casc*` for S3 |
| slotundo-unnecessary | synthetic (USE-disabled `soflag? ( := )` dep) | rule 8 USE-reduction | strict-xfail (scan reads raw vdb) | flips in S4 |
| slotundo-changed-slot | synthetic (consumer same-version SLOT move) | rule 6 (no flag) / rule 3 (flag) | no-flag MATCH (guard vs over-undo); flag strict-xfail (loses `r` + edge via standalone trigger) | S4 keeps guard; flag display flips in S5 |
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
