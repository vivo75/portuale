# Backlog #24 — slot-operator rebuild undo path — agent plan

Status: proposed. Owner decisions required at Gate 0 (§6) before Slice
3 lands. Written 2026-09-12 against `main` @ `c7ad477` (post #23
merge). Merged from two independent drafts
(`024-slot-operator-plan.fable.md`, `024-slot-operator-plan.muse.md`):
the live reproduction, real-shape analysis and slice design come from
the first; the invariants/stop-condition discipline, xfail-first
pinning, flag gate, L0 triage slice and routing summary come from the
second. Two corrections were made while merging: the a522084 `B-0`
miss is **not** the undo path (§0.3), and real's undo has a ninth,
load-bearing rule (the dep-string comparison, §1.2) that the second
draft's guard list omitted.

**Read `AGENTS.md`, `docs/agent-context.md`,
`docs/023-backtracking_resolve.md` §1 (rules), `docs/023-oracle.md`
(method), `docs/history/scope-backlog-2026-09-05.md` lines 369-421 (the
investigation this item came from) and
`docs/history/slot-op-rebuild-cascade-plan.md` (what shipped in v1)
first.** This plan adds only what is specific to #24 and reuses #23's
shape (`ResolveCtx` / `BacktrackParams` / `run_pass` /
`collect_feedback` / `assemble_result` / `Backtracker`).

Real portage = the vendored `3rdparty/portage/lib/_emerge/depgraph.py`
unless stated. Every line number drifts; re-locate by function name
before trusting anything (AGENTS.md step 1).

Model tiers, same convention as the #22/#23 plans:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Fable 5.1 / Opus 5 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Opinion — what the backlog line gets right, what it gets wrong

The backlog line (`docs/backlog-tasks.md` #24): *add backtracking for
a rebuild that shifts another sub-slot, re-bind the rebuilt consumer's
own `:=` deps, and the `--changed-slot` interaction (one missing piece,
not three)*; #23 hands it the a522084 `B-0` miss. The 2026-09-05
investigation adds a fourth face, `IUSE_EFFECTIVE` in the built-dep
domain.

What holds after re-reading real and portuale side by side:

1. **"One missing piece" is right, and the piece has a name.** Real
   never synthesises a rebuild entry. Every slot-operator detector
   (`_slot_operator_update_backtrack` 2400, `_slot_change_backtrack`
   2361, `_slot_operator_unsatisfied_backtrack` 2881) writes the same
   thing: `backtrack_infos["config"]["slot_operator_replace_installed"]
   += {(root, slot_atom)}` and sets `_need_restart`. The next pass turns
   that set into a `SetArg` named `__auto_slot_operator_replace_installed__`
   with `force_reinstall=True` (`_gen_reinstall_sets` 5457-5480), so
   the consumer is **walked as a fresh ebuild node**: its tree
   `RDEPEND` with `:=` re-evaluated, its own `:=` consumers registered,
   its merge-order edges real. Cascade, re-bind and merge order all
   fall out of that one move. The undo (`_eliminate_rebuilds`
   3859-4000) then demotes any forced reinstall whose re-evaluated deps
   equal the installed ones; that is where `--changed-slot` has its one
   line (3898-3899).
2. **Portuale's rebuild is a post-pass entry synthesiser, not a graph
   node.** `assemble_result` calls `slot_operator_rebuild_entries`
   (`rust/portage-repo/src/lib.rs` ~11908) after the `Backtracker` has
   settled, and the entries it pushes (the `Reinstall {
   slot_operator_rebuild: true }` construction at ~12056) carry
   `deps: Vec::new()` and `required_by: []`. So the consumer's deps are
   never re-walked, its `:=` never re-bound, merge order falls back to
   array position, and there is nothing for an undo path to undo. The
   fix is to route the rebuild through the `Backtracker` the way #23
   routed slot-conflict masks. The in-walk hook already exists: the
   `reinstall_atoms` flip inside `run_pass` (~16045, `AlreadyInstalled`
   → `Reinstall`, after which `enqueue_dependencies` walks the tree
   ebuild) is exactly what real's pseudo-set does.

What is wrong or under-described:

3. **The a522084 `B-0` miss is not the undo path.** Reproduced live
   (§1.3): the same fixture with `--complete-graph` added produces
   real's `[A-2, B-0]` today. Under `--update --deep @world` the CLI
   skips the complete-mode pass because `--deep` is in force
   (`rust/portuale/src/pretend.rs` `want_complete`: `deep ==
   Deep::NotRequested && …`; Python mirror `not deep and …` ~21183),
   and with it `complete_seed_atoms` stays empty → `slot_op_reachable`
   is empty → the scan is suppressed wholesale. Real's `_complete_graph`
   (8562-8648) has **no** `--deep` condition. This is a bounded M-tier
   fix (Slice 1). The #23 oracle row and the
   `test_oracle_non_slot_operator_update_selects_new_slot` pin are
   corrected by Slice 1, not Slice 3.
4. **A second visible symptom the backlog line does not mention:**
   `emerge -p app-misc/A` on the same fixture prints `[B-0, A-2]`, the
   rebuilt consumer *before* the provider it is rebuilt against. Real's
   `test_slot_operator_rebuild.py` case 1 pins `[A-2, (B-0, C-0)]`.
   That is the `deps: Vec::new()` cut made visible and is the cleanest
   acceptance bar for Slice 3.
5. **Scope discipline: the "missed update" probe family is a different
   piece.** `_slot_operator_update_probe` (2576-2815, ~240 lines) +
   `_slot_operator_check_reverse_dependencies` (2472-2574, memoised
   per `__init__:868`) + `_downgrade_probe`/`_upgrade_available` decide
   *whether rebuilding a parent would let a child upgrade* (bugs
   584626, 528610, 652938, 743115). That is a selection-time probe over
   `_iter_similar_available`, not the undo path; `_downgrade_probe`
   inside it is #35's, and the family is the part real itself still
   gets wrong (Qt 6.9.3→6.10.1 is an upstream `xfail`, bugs
   964705/968228). It needs #25 (installed nomerge nodes) and #36
   (mask-aware selection) before it can be faithful. Recommendation:
   file it as separate items (§3 v2) and keep #24 to reconciliation +
   undo.

**Difficulty.** v1 as scoped here: **7/10**, on par with #22 and below
#23-C2. The probe family (v2): 8-9/10, the hardest open resolver
work after #23. **Bottom line:** #24 v1 is one M gate fix, one M
oracle slice, one F architectural slice (route the rebuild through
the `Backtracker`), one F undo slice (`_eliminate_rebuilds` +
`--changed-slot`), one F L0-triage slice, plus S docs. The F slices are
frontier work because they change `run_pass`/`BacktrackParams`/
`collect_feedback` in both languages with ~20 pinned contract outputs
moving; everything else is mid/small. A mid model can do Slice 3 only
with the spec below *and* the Slice-2 oracles already pinned; without
Slice 2 first, Slice 3 is F-only.

---

## 1. Ground truth (cite these, do not re-derive)

### 1.1 Real's machinery, by role

| Role | Function | Lines | Note |
|---|---|---|---|
| register | `_add_slot_operator_dep` | 4143-4149 | every `:=`/soname dep whose child landed in the graph, keyed by child `(root, slot_atom)`; removed with the node (4095-4103) |
| detect (post-walk) | `_slot_operator_trigger_reinstalls` | 3089-3132 | called from `_process_slot_conflicts` (2132) only when `_allow_backtracking`; unbuilt `:=` → `_slot_change_probe`; built `:S/SS=` with built parent → `_slot_operator_update_probe` (new-slot first under `rebuild_if_new_slot`, then same-slot); gated by `want_update_probe = dep.want_update or not dep.parent.installed` (bug 652938) |
| detect (in-walk) | `_add_dep` unsatisfied built slot-op | 3447-3454 | `_slot_operator_unsatisfied_probe` → backtrack (bug 439694) |
| detect (in-walk) | `_select_pkg_from_installed` | 8770-8790 | pulling the installed instance for a built slot-op atom when the child was already scheduled → probe instead of slot conflict |
| detect (conflict) | `_slot_conflict_backtrack_abi` | 2282-2315 | slot-conflict variant via `_slot_operator_update_probe_slot_conflict` (2453, autounmask levels) |
| feedback | `_slot_change_backtrack` / `_slot_operator_update_backtrack` / `_slot_operator_unsatisfied_backtrack` | 2361-2452, 2881-2933 | all write `config["slot_operator_replace_installed"]` (installed parent/child → `_replace_installed_atom` 3059-3087) and/or `config["slot_operator_mask_built"]` (non-installed binaries) + `_need_restart` |
| consume | `resolver/backtracking.py::_feedback_config` | 237-257 | `slot_operator_replace_installed.update`, `slot_operator_mask_built` → `runtime_pkg_mask`; `prune_rebuilds` clears both |
| apply | `_gen_reinstall_sets` | 5457-5480 | pseudo `SetArg` `__auto_slot_operator_replace_installed__`, `force_reinstall=True`, `reset_depth=False` |
| undo | `_eliminate_rebuilds` | 3859-4000 | §1.2 |
| undo (global) | `_resolve` `prune_rebuilds` | 5765-5779 | `_ENABLE_PRUNE_REBUILDS and _slot_operator_replace_installed and _get_missed_updates()` → clear the set, restart once (bug 743115) |
| restart gate | `_resolve` | 5700-5713 | a pass that grew either set returns `False` → `_backtrack_depgraph` re-runs |
| `--changed-slot` | `_changed_slot` | 3247-3252 | `_equiv_ebuild` slot/sub-slot vs pkg; the only contact with this family is 3898-3899 |
| display | `_show_abi_rebuild_info` / `_compute_abi_rebuild_info` | — | already ported (`abi_rebuilds`, `pretend.rs` ~11363 render, ~949 `r` marker) |
| `--debug` | "backtracking due to missed slot abi update:" | 2406-2416 | portuale has no such narration; cut (same ruling as #23) |
| `IUSE_EFFECTIVE` | `dbapi/__init__.py::_iuse_implicit_cnstr` | 238-276 | built-package implicit-IUSE tolerance; only bites a USE-conditional atom on an implicit flag against a scheduled consumer; named cut unless a fixture bites |

### 1.2 The undo path, rule by rule (`_eliminate_rebuilds` 3859-4000)

Skipped entirely when `"empty" in myparams` or any slot conflict is
live (bug 922038). For each `(root, atom)` in the replace set, for each
**non-installed** graph pkg matching it, the rebuild is **kept**
(`continue`) unless all of:

1. an installed instance of the same `slot_atom` exists with the
   **same cpv** (a real upgrade is never undone);
2. pkg not in `_reinstall_nodes` (`--newuse`/`--changed-use` wins);
3. **`--changed-slot` (3898-3899):** not (`changed_slot` and
   (`_changed_slot(pkg) or _changed_slot(installed_instance)`));
4. every parent atom in `_parent_atoms[pkg]` matches the *installed*
   instance;
5. non-`selective` only: no `AtomArg`/non-auto `SetArg` parent (the
   user did not ask for it);
6. installed and new `(slot, sub_slot)` equal;
7. built pkg only: `provides`/`requires` equal (soname; N/A here);
8. **the dep comparison (3935-3970):** `use_reduce` every `_dep_keys`
   string of the installed instance (uselist = new pkg's enabled USE)
   vs the new pkg's — for an ebuild, `pkgsettings.setcpv(pkg)` +
   `evaluate_slot_operator_equal_deps(pkgsettings, new_use,
   _graph_trees)` first, so every `:=` is **bound against the graph**;
   both sides `strip_libc_deps`. Unequal → keep.
9. Otherwise (`modified = True`): re-add the installed instance in the
   pkg's place with the same parent priorities, `_remove_pkg(pkg)`.
   `_resolve` then re-serialises (`altlist()`).

Rule order is load-bearing; rule 8 is why a522084's `B-0` is *kept* by
real (tree `app-misc/A:=` binds to `A:0/2=` against the graph,
installed says `A:0/1=`). A port that stops at rule 6 would undo it.

### 1.3 Portuale today (verified live, 2026-09-12)

Fixture = the a522084 shape as `_b1_root` builds it
(`tests/test_emerge_pretend_contract.py::test_oracle_non_slot_operator_update_selects_new_slot`):
world `app-misc/A`; vdb `A-1` (`0/1`, `PDEPEND=app-misc/B`), `B-0`
(`RDEPEND=app-misc/A:0/1=`); tree `A-1`, `A-2` (`0/2`), `B-0`
(`RDEPEND=app-misc/A:=`). `PORTAGE_CONFIGROOT=fixtures`.

| command | portuale | real (upstream test) | verdict |
|---|---|---|---|
| `-p -uD @world` | `[A-2]` | `[A-2, B-0]` (a522084) | **DIVERGENT — complete gate** |
| `-p -uD --complete-graph @world` | `[A-2, B-0]` + "causing rebuilds" | `[A-2, B-0]` | MATCH |
| `-p app-misc/A` | `[B-0, A-2]` | `[A-2, (B-0, C-0)]` (`test_slot_operator_rebuild` case 1, minus C) | **DIVERGENT — order** |

Code facts behind the two divergences:

- `pretend.rs` phase-2 gate: `want_complete = complete_graph || (deep
  == Deep::NotRequested && !nodeps && complete_graph_auto_enable(…))`;
  `complete_seed_atoms` is injected only into the `complete = true`
  pass (`run_resolve` closure). Python: ~21150-21190, same guard.
  `solver_bridge.rs:657` and Python `_required_set_reachable_cps`
  (16976) compute `slot_op_reachable` from those seeds regardless of
  `complete`.
- `slot_operator_rebuild_entries` (lib.rs ~11908; Python
  `_slot_operator_rebuild_entries` 8737, "cuts and all"): reads vdb
  `*DEPEND` of every reachable installed cp, finds `:S/SS=` atoms whose
  provider entry (`Upgrade`/`Downgrade`/`Reinstall`) lands at the same
  slot, different sub-slot, iterates to a fixpoint on the tree
  ebuild's `SLOT`, emits `Reinstall { slot_operator_rebuild: true }`
  entries (~12056) with empty `deps`/`required_by`. Called once from
  `assemble_result` (~18361), gated on
  `!ignore_built_slot_operator_deps && rebuild_if_new_slot`. Its doc
  comment's own "Cuts" list (~11902-11907) is the v1 cut inventory.
- Merge order: `GraphEntry::deps` doc (~11441) says synthetic
  slot-op/`--rebuild-if-*` entries "fall back to array position plus
  their `required_by` edges"; both are empty here.
- `BacktrackParams` (~15520) has no replace-installed set;
  `BacktrackFeedback` (Config / SlotConflict / MissingDep) has no
  slot-op kind; `params_equal` would need the new field. Python:
  `_bt_add`/`_bt_feedback_config`/`_params_equal` (~11375-11460).
- The in-walk force-reinstall hook exists: `run_pass` ~16045
  (`ctx.reinstall_atoms` → `PretendOutcome::Reinstall { …
  slot_operator_rebuild: false }`), after which `enqueue_dependencies`
  (~16585) walks the tree ebuild's deps from `read_md5_cache` metadata.
- `:=` binding against a graph exists only in the CLI crate:
  `rust/portuale/src/ebuild_phases.rs::bind_slot_operator` (~1132,
  binds against `$ROOT`'s vdb at merge time, real
  `evaluate_slot_operator_equal_deps`). `portage-repo` has none.
  Libc-dep stripping exists (`is_injected_libc`, cluster I slice 10).
  Built-atom normalisation (`with_slot("=")`) exists as
  `is_built_slot_op`/`reverse_dep_constraint_atom` (~11518-11545).
- `--changed-slot` ships standalone: `slot_changed` (lib.rs ~7444),
  wired at ~10304 and ~11073 as a `Reinstall` trigger.
- Flag precedent for a risky landing: `abort_path_enabled()`
  (lib.rs ~13742, `PORTUALE_ABORT_PATH`), from #19.
- Pins that will move in Slices 3/4 (list them in S0):
  `test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer`,
  `test_ignore_built_slot_operator_deps_suppresses_the_rebuild`,
  `test_slot_operator_rebuild_cascades_through_a_multi_level_chain`,
  `test_oracle_non_slot_operator_update_selects_new_slot`, the
  `slot_operator_rebuild_entries_*` unit tests (~29052-29300), every
  `CASES` row naming `slotbind*`/`casc*`/`subslot*`/`revdepslot*`/
  `changedslotpkg`, and the `--backtrack=0/30` /
  `--ignore-built-slot-operator-deps` / `rebuild_if_new_slot` rows.

---

## 2. Rules and invariants for every slice

All of `docs/023-backtracking_resolve.md` §1 applies verbatim. In
addition:

- **Dual-language from Slice 1.** There is no behaviour-neutral phase
  here; every slice lands Rust + `python/emerge_pretend_reference.py`
  in one commit, diffed empirically over `fixtures/` and the `_b1_root`
  cases, then pinned by contract tests (a parametrized `CASES` entry
  *and* a pinned-output test, plus a Rust unit test where a helper is
  added).
- **Oracle before pin.** No contract pin changes unless the commit body
  names the upstream test (`3rdparty/portage/lib/portage/tests/resolver/…`)
  whose `mergelist` justifies the new output, or the L0 probe that does.
  A pin that changes with no oracle behind it is a stop-and-surface
  event.
- **Do not regress the non-undo path.** Keep the `reachable` gate, the
  `New`-vs-replace provider rule, the tree-`SLOT` cascade and the
  sorted/deduped `abi_rebuilds` pairs. Before each slice, assert
  identical output on every existing slot-op fixture.
- **Determinism.** No timing, no set-iteration order (`BTreeSet` for
  the new params field).
- **L0 at default budget, findings triaged**, after Slice 1 and after
  Slices 3/4 (Slice 6). Archive raw portuale output before Slice 1
  (`TEST/run/l0-resolver.sh`) as #23's A0 did, and `diff -r`
  afterwards. Slice 1 may only add `[ebuild rR]` rows + "causing
  rebuilds" blocks under `-uD`; anything else is a defect. Regressions
  block the merge; adjudicated non-bugs go to
  `TEST/compare/known-divergences.yaml` with a one-line reason.
- **Fixtures, not real-tree data, for contract tests.** Upstream
  `ResolverPlayground` mergelists are the oracle; the real tree is the
  regression bed (Slice 6), never a test input.
- **Fixture hygiene:** `git add` new `fixtures/` directories before any
  `git clean -fdq fixtures/` (it deletes untracked fixtures). Compare
  failing contract-test *names* against a clean-`main` baseline, not
  counts (the suite has a known order-dependent isolation bug).
- **Perf gate:** `bench/` must not regress. Slice 1 option (a) can
  double resolve time on `-uD @world`; Slice 3 must keep the vdb scan
  behind the `reachable` gate and run it once per pass, not per node.
- **Cite.** Every behavioural claim about real carries `file:line`
  into the vendored checkout; comments carrying citations move with
  code, never shrink.
- **Stop conditions.** Each slice lists one. If hit, stop, write the
  finding into the slice notes, hand back to the owner. Do not work
  around.
- **No `git commit`/`push` unless asked** (AGENTS.md step 9).

---

## 3. Slices

### S0 — Baseline + inventory (S, M sanity-read, ½ h)

1. Full gate on `main`; archive L0 raw output.
2. `graft callers slot_operator_rebuild_entries --depth all` and
   `grep -n _slot_operator_rebuild_entries python/…`; write the
   call-site list into the S3 PR description.
3. Inventory table: every write site of replace-installed-shaped state
   (`scheduled`, `new_slot`, `abi_rebuilds`, the entry construction at
   ~12056, the call at ~18361) with scope (per-pass / cross-pass /
   assembly); every `continue` in the fixpoint loop in source order;
   the nine undo rules of §1.2 with a portuale-coverage column
   (ships / missing / non-goal).
4. List every contract pin from §1.3's last bullet with its current
   expected output.
5. Re-run the three commands of §1.3 and commit their output as
   `docs/024-oracle.md` §"baseline" (no code). **M** sanity-reads the
   coverage column once; a wrong "already covered" here poisons S3.

Stop condition: none (nothing changes).

### S1 — Complete-mode gate under `--deep` (M, F review, 1–2 h)

Goal: `-p -uD @world` on the a522084 shape prints `[A-2, B-0]`.

Two ways; pick by bench:

- **(b) Recommended:** when `--deep` is in force, keep skipping phase 2
  **but feed `complete_seed_atoms` to a phase-1 re-run** whenever
  `complete_graph_auto_enable(...)` fires on phase-1 entries. The deep
  walk already visited the reachable closure; the seeds only feed
  `slot_op_reachable`. Still a second resolve, but only on runs that
  change an installed package. If `bench/` shows that is too slow on
  L0's `-puD @world` (4.6 s baseline), compute `slot_op_reachable`
  once in the CLI and pass it through `ResolveRequest` into the first
  pass (one vdb walk, no re-resolve).
- **(a) Faithful:** drop the `deep == NotRequested` guard so phase 2
  runs like real (8562-8648). Simpler, doubles work under `-uD`. Only
  if (b) produces a divergence on L0.

Also check `complete_graph_auto_enable`'s third arm equals real's
`complete_if_new_slot = rebuild_if_new_slot` (8590-8592).

Tests: flip the a522084 pin (second assertion only); `CASES` row; add
the `test_slot_operator_unsatisfied.py` shapes (bug 439694: installed
`A-2`, `B-0` bound to `0/1`, `-uD @world` → `[B-0]`; and `emerge A
--oneshot` → `[A-2]`, no rebuild). Update `docs/023-oracle.md`'s
a522084 row (PARTIAL → MATCH, cause = complete gate) and the #24
backlog line.

Stop condition: L0 raw diff shows anything but added `rR` rows /
"causing rebuilds" blocks.

### S2 — Oracle fixtures for the whole family (F brief ½ h, M execution 2–3 h)

Branch touching only `fixtures/`, `tests/`, `docs/`. Method =
`docs/023-oracle.md`: upstream test → `_b1_root` case (+
`fixtures/repo/…` ebuilds + `metadata/md5-cache`), one
`test_oracle_slotop_*` each, Rust == Python asserted *now*, real's
mergelist as the expected value under `xfail(strict=True)` where
portuale diverges (same discipline as #19's 24 strict xfails). Table
in `docs/024-oracle.md` with MATCH/DIVERGENT/PARTIAL and the "expected
v1 status" column below (owner agrees it at Gate 0).

Start with a 10–20 line prior-art summary (`docs/history/slot-op-rebuild-cascade-plan.md`,
the slot-op entries in `what-this-proves.md`): what shipped, what was
cut, why the cuts were one piece.

| case | upstream file | mechanism | expected v1 status |
|---|---|---|---|
| a522084 | `test_solve_non_slot_operator_slot_conflicts.py` | complete gate | MATCH after S1 |
| rebuild-1 | `test_slot_operator_rebuild.py` case 1 (`emerge A`, `--dynamic-deps=n`) | order + `\|\|`-wrapped `:=` (bug 522652) | MATCH after S3 for order; `C-0` (the `\|\|` arm) may be PARTIAL — surface, don't chase |
| rebuild-2 | same, case 2 (`--usepkg`, binary `E-1` built against `F:0/1`) | `slot_operator_mask_built` (bug 652938) | v2 — DIVERGENT, documented |
| unsat | `test_slot_operator_unsatisfied.py` (bug 439694) | in-walk unsatisfied arm | MATCH after S1 |
| slotchange | `test_slot_change_without_revbump.py` cases 1, 4 | `_slot_change_probe` + `--changed-slot` (bug 456208) | MATCH after S5 (ebuild variant; binary half is v2) |
| regslotchange | `test_regular_slot_change_without_revbump.py` | same, main slot | MATCH after S5 |
| complete | `test_slot_operator_complete_graph.py` (bug 614390) | complete mode + cascade + undo | **S4 acceptance case** |
| revdeps | `test_slot_operator_reverse_deps.py` (bug 584626) | `check_reverse_dependencies` + `_upgrade_available` | v2 — DIVERGENT |
| parentdown | `test_slot_operator_update_probe_parent_downgrade.py` (bug 528610) | probe must **not** fire | must stay `[]` through v1 (regression guard) |
| missedupd | `test_slot_operator_missed_update.py` (bug 743115, `--backtrack 4`) | `prune_rebuilds` | v2 |
| autounmask / exclusive / runtime_pkg_mask / unsolved / bdeps / required_use | remaining `test_slot_operator_*.py` | mixed | translate, label; mostly v2 |
| conflict-rebuild | `test_slot_conflict_rebuild.py`, `test_slot_conflict_force_rebuild.py` | `_slot_conflict_backtrack_abi` | v2 |

Synthetic shapes (portuale-side, no upstream twin), also pinned:

- `slotundo-cascade`: a522084 plus the consumer's own tree `SLOT`
  bumped, so its rebuild lands at a new sub-slot and a second-level
  consumer is rebuilt (guards the existing `casc*` behaviour under the
  new shape).
- `slotundo-unnecessary`: a consumer the scan schedules whose
  graph-bound tree deps equal its vdb deps (provider bumped version,
  same `S/SS`; or the binding re-satisfied by the settled graph).
  Today: rebuilt. Real: undone. Must flip in S4.
- `slotundo-changed-slot`: `slotundo-unnecessary` plus the consumer's
  own `SLOT` independently stale; with `--changed-slot` the rebuild is
  **kept**. Flips in S5.
- `slotundo-rebind`: the scheduled consumer's tree `RDEPEND` gained a
  genuinely new dependency; after S3 that dependency appears in the
  merge list (real: yes, via the walked node).

`test_missed_update.py` (Qt) and
`testBacktrackInconsistentForcedRebuildWithBlocker` stay out
(upstream `xfail`; blocker + rebuild).

Stop condition: an upstream case that cannot be expressed with
`_b1_root` + fixtures (e.g. needs binpkgs) is labelled "not
translatable", not approximated.

### S3 — Route the rebuild through the `Backtracker` (F writes, second F reviews, 6–8 h)

The architectural slice. Net effect: a scheduled consumer becomes a
**walked graph node** on the next pass. Land behind a flag if G0.2
says so (`PORTUALE_SLOT_OP_GRAPH=0` → old synthesiser), removed in S6.

1. **State.** `slot_operator_replace_installed: BTreeSet<(String,
   String)>` on `BacktrackParams` (real keys `(root, slot_atom)`;
   portuale's one-instance-per-cp model makes `(cat, pkg)` enough —
   document the narrowing). Include it in `params_equal`. Python: same
   key in the params dict and `_params_equal`.
2. **Detection stays post-walk; its output changes.** Keep
   `slot_operator_rebuild_entries`'s *scan* (real's
   `_slot_operator_trigger_reinstalls` narrowed to "provider in the
   merge list at a different sub-slot") but return the set of consumer
   cps + `abi_rebuilds` pairs, **not** `GraphEntry`s. Move the call
   from `assemble_result` into `collect_feedback`: if the set minus
   `params.slot_operator_replace_installed` is non-empty, return
   `PassDecision::Feedback(BacktrackFeedback::Config { params: grown })`
   with the union — a config node, free of the mask budget, exactly
   real's `_feedback_config`. Otherwise fall through to the existing
   chain. Run it once per pass, behind the `reachable` gate.
3. **Apply in-walk.** In `run_pass` next to the `reinstall_atoms` flip
   (~16045): an `AlreadyInstalled { version }` whose cp is in
   `bp.slot_operator_replace_installed` becomes `Reinstall { version,
   slot_operator_rebuild: true, … }`. The walk then reaches
   `enqueue_dependencies` with the **tree ebuild's** metadata, which is
   the re-bind and the merge-order edge in one. Real's
   `reset_depth=False`/`force_reinstall=True`: the flip must not count
   as a top-level arg for `top_level_cps` or `--selective`.
4. **Cascade for free.** The consumer is now a merge-bound entry at its
   tree `SLOT`; the next pass's scan sees it as a provider with a
   possibly new sub-slot. The set only grows and is bounded by the
   installed cp count, so the loop terminates (real relies on the same
   monotonicity). Delete the internal fixpoint loop.
5. **Display.** `abi_rebuilds` still comes from the scan, surfaced via
   `PassResult` → `GraphResult`. The `[oldver]` bracket
   (`output.py:723-732`) now comes from the normal `Reinstall`
   renderer's slot comparison; verify the `cascmid-1.0 [1.0]` pin.
6. **Remove** the `GraphEntry` construction block and the
   `.extend(slot_op_rebuilds)` in `assemble_result`. Leave
   `rebuild_if_entries` (`--rebuild-if-*`, real `__auto_rebuild__`)
   alone; file a follow-up (G0.6).
7. **Remove nothing from the mask path.** If an S2 case regresses
   because the rebuild arm and a slot-conflict mask arm disagree,
   surface it: that is a ranking question, not a reason to keep the
   synthesiser.
8. **Python mirror** from the finished Rust diff (M may write it). The
   second F reviewer gets only the diff + §1 + this section.

Acceptance: `emerge -p app-misc/A` → `[A-2, B-0]`; `slotbind*`/`casc*`
pins keep their rows but may change **order** (justify each against the
upstream mergelist or the PDEPEND/RDEPEND edge that now exists);
`--json` shows non-empty `deps`/`required_by` on rebuilt rows;
`slotundo-rebind` flips; Rust == Python on all `fixtures/` + `_b1_root`
cases; `parentdown` still `[]`.

Traps:
- `resolved_slots`/`other_outcomes` dedup: the consumer may already
  have been visited as `AlreadyInstalled` earlier in the same pass; the
  flip must happen where the outcome is *first* decided, not by
  patching entries afterwards (or the deps never enqueue).
- #25: an installed consumer that is a pure `nomerge` node is not an
  entry at all in portuale; the scan over `slot_op_reachable` covers
  it, so the flip must *create* the entry, not find it.
- The stale `A:0/1=` atom from the consumer's *vdb* string must not
  form a slot conflict against `A-2` on the pass before the flip
  (today it doesn't — keep it so; real avoids it at 8770-8790).
- Do not touch `Deep`/`top_level` semantics for the flipped node.
- Perf: the scan reads vdb `*DEPEND` of every reachable installed cp;
  one pass more than today is fine, one per node is not.

Stop condition: the flip needs per-node graph history the
`Backtracker` does not carry (e.g. `_parent_atoms` across restarts).
That is a Phase-A-shaped refactor prerequisite, not an S3 detail.

### S4 — `_eliminate_rebuilds` undo path (F, 4–6 h)

Prereq: S3. Implements §1.2 as a post-walk step in `collect_feedback`
(after the S3 scan, before the slot-conflict chain; real runs it in
`_resolve` after `_process_slot_conflicts`).

1. **`:=` graph binder in `portage-repo`:**
   `bind_slot_operator_deps(depstr, pass_entries, vdb) -> String`: for
   each `cat/pkg:=`/`cat/pkg:S=` atom, bind to the slot/sub-slot of the
   package **as this pass leaves it** (merge-bound entry first, else
   installed). Real `_slot_operator.py::evaluate_slot_operator_equal_deps`.
   Reuse `ebuild_phases.rs::bind_slot_operator`'s token logic; do not
   call across crates. Unit-test it.
2. **Comparison:** for each cp in `params.slot_operator_replace_installed`
   whose entry is `Reinstall { slot_operator_rebuild: true }` at the
   installed version, rules 1–8 of §1.2 **in that order**. Rule 8 =
   `flat_dep_atoms` (use-reduced against the *new* USE) of the bound
   tree string vs the vdb string, both through `is_injected_libc`
   stripping. Rule 4 = every `required_by` atom of the entry (and
   every vdb reverse-dep atom `reverse_dependency_constraints` yields)
   matches the installed instance. Rule 5 = `ctx.selective` +
   `top_level_cps`. Rule 2 = `--newuse`/`--changed-use` `changed_flags`
   non-empty.
3. **Demote:** drop the cp from the set → `BacktrackFeedback::Config`
   with the shrunk params, one more pass. Latch the cp in a
   `slot_operator_undone: BTreeSet` on `BacktrackParams` so the S3 scan
   cannot re-add it (real never re-adds within a node because
   `_eliminate_rebuilds` does not set `_need_restart`; portuale's
   re-scan would, hence the latch).
4. **Skip conditions:** `ctx.empty`; any live slot conflict in the pass
   (bug 922038).
5. **`abi_rebuilds`:** a demoted consumer drops out of "causing
   rebuilds".
6. Rust unit tests: one per rule, on a fixture pair
   `dev-libs/slotundo{target,consumer}`.

Acceptance: `complete` (bug 614390) → MATCH; `slotundo-unnecessary`
flips; all S3 pins unchanged; `parentdown` still `[]`.

Stop condition: rule 8 needs a USE set for the *new* node that the pass
does not have at `collect_feedback` time. (It should: the entry's
`use_flags_display` / effective USE is computed in-walk.)

### S5 — `_slot_change_probe` + `--changed-slot` contact (M, F review, 2 h)

- `_slot_change_probe` (2317-2359): for an **unbuilt** `:=` dep
  (parent is an ebuild being merged) whose child is installed, if the
  child's tree ebuild at the same cpv has a different `(slot,
  sub_slot)` → schedule the child (bug 456208). Portuale has the
  metadata side (`slot_changed`); add it as a second detector feeding
  the same S3 set.
- `--changed-slot` (3898-3899): S4 rule 3, keep the rebuild when
  `ctx.changed_slot && (slot_changed(entry) || slot_changed(installed))`.
- Oracles: `slotchange` cases 1 and 4 (ebuild variant), `regslotchange`,
  `slotundo-changed-slot`. The binary half of `slotchange` case 1
  (`--usepkg`, `libarchive-3.1.1` binary at `SLOT=0` rejected) is
  `slot_operator_mask_built` → v2.

Sequence after S4 so the rule-3 line lands in place, not as a
temporary hook.

### S6 — Real-tree validation (L0) and triage (F triage, 1–2 h)

Rebuild the container if the base changed; run `TEST/run/l0-resolver.sh`
at default budget. Baseline = S0 archive. Expected: `rR` rows appear
under `-uD` probes (S1) and move to real's position (S3); the
merge-order timing cluster untouched; nothing else moves. For every
atom whose status changed in either direction, file a finding in
`TEST/findings/` with the diff, the rebuild/undo reason and a verdict
(fixed / regression / new known-divergence). Remove the S3 flag once
clean (or keep it one release if G0.2 says so). Run L1 only if
anything outside `--pretend` was touched (`git diff --stat` says no).

### S7 — Docs closure (S, ½ h)

`what-this-proves.md` append (one paragraph per slice, never rewrite),
`backlog-tasks.md` #24 → DONE with non-goals stated (v2 items,
`IUSE_EFFECTIVE`, `--debug` narration), `scope-backlog.md` §A entry
rewritten from four cuts to what shipped + what stayed cut,
`docs/024-oracle.md` final table with verdict line, `docs/023-oracle.md`
a522084 row corrected, memory note.

### v2 — carve out as separate backlog items (not #24)

File each with its upstream test and real function so a future slice
starts grounded:

- **#24b `_slot_operator_update_probe` + `_slot_operator_check_reverse_dependencies`**
  (2472-2815): "rebuild the parent so the child can upgrade"; bugs
  584626, 528610, 612772, 612874, 460304. Needs #25 nomerge nodes
  (parent atoms of installed packages), `_iter_similar_available`, and
  `_select_atoms_probe`. `_downgrade_probe` inside it stays #35's. F,
  8–12 h.
- **#24c `slot_operator_mask_built`** (bug 652938): a
  `MaskReason::SlotOperatorMaskBuilt` in `runtime_pkg_mask` for
  non-installed binaries; `prune_rebuilds` clears it. M once #24b's
  probe exists.
- **#24d `prune_rebuilds`** (5765-5779, bug 743115): replace set
  non-empty and `_get_missed_updates()` non-empty → clear the set,
  restart once. Needs a `missed_updates` accumulator. M.
- **#24e `_slot_conflict_backtrack_abi`** (2282-2315): slot-conflict
  variant with autounmask levels. M after #24b.
- **`IUSE_EFFECTIVE`** (`dbapi/__init__.py:238-276`): named cut unless
  a fixture bites; never partially patch atom matching.
- **`--rebuild-if-*` through the same path** (G0.6).

---

## 4. Difficulty and routing

| Slice | Work | Tier | Why |
|---|---|---|---|
| S0 | baseline, inventory, pin list | S (+ M sanity-read) | greps and tables, existing precedents |
| S1 | complete gate under `--deep` | M + F review | two-line guard in two languages; the (a)/(b) perf choice and the L0 diff need judgment |
| S2 | oracle fixtures + xfails | M (F brief) | pattern-following (#23-B1 precedent) |
| S3 | route rebuild through `Backtracker` | **F write + second F review** | `BacktrackParams`/`collect_feedback`/`run_pass` + mirror; ~20 pins move; dedup traps |
| S4 | `_eliminate_rebuilds` | **F** | new `:=` graph binder + nine ordered rules with a latch; easy to oscillate or over-undo |
| S5 | `_slot_change_probe` + `--changed-slot` | M + F review | small detector + one `continue` |
| S6 | L0 triage | **F** | honest adjudication, regression detection |
| S7 | docs | S | — |

Effort (frontier-hours of agent time, order of magnitude): S0 ½, S1
1–2, S2 2–3, S3 6–8 (+1 second review), S4 4–6, S5 2, S6 1–2, S7 ½.
Total F ≈ 13–19, M ≈ 6–8, S ≈ 1–2.

Do not give S3, S4 or S6 to a cheaper model to save cost: the known
failure mode — a schedule that matches fixtures while missing the
retract (or the walked node) on the real tree — is what the current
synthesiser already does, and only the spec plus a fresh-context F
review catches it.

---

## 5. Sequencing and gates

```
S0 ─ S1 ─┐
S2 ──────┴─ go? ─ S3 ─ S4 ─ S5 ─ S6 ─ S7
```

- S1 and S2 are independent of each other and of S3; S2 on a
  fixtures/tests/docs-only branch.
- "go?" = S1 merged, L0 diff clean, S2 table filled with the "expected
  v1 status" column agreed at Gate 0.
- **Go/no-go rule:** if S2 finds no divergence beyond a522084 (S1),
  order (S3) and `slotundo-unnecessary` (S4), S3/S4 still go (those
  *are* the item); S5 shrinks to the `--changed-slot` line. If S1
  alone closes every oracle case, park #24 with the fixtures as
  regression guards and file S3 as the "walked node" item.
- S4 needs S3's in-walk node; S5 after S4.

---

## 6. Gate 0 — owner decisions (answer in writing before S3)

| # | Question | Why it matters |
|---|---|---|
| G0.1 | Accept that the a522084 `B-0` miss closes in **S1** (complete gate) and the #23 oracle row is corrected accordingly? | Prevents S3 from being justified by a case it does not own. |
| G0.2 | S3/S4 land behind a flag (`PORTUALE_SLOT_OP_GRAPH=0` → old synthesiser, same pattern as #19's `PORTUALE_ABORT_PATH`), removed in S6 — or unconditionally? Recommended: flag, removed once L0 is clean. | Makes S3 L0-diffable both ways; costs a second code path in both languages for one slice. |
| G0.3 | S1 option (b) (seeds into a phase-1 re-run under `--deep`) vs (a) (always run phase 2)? Default (b) unless `bench/` says the re-resolve is free. | Perf on `-uD @world`, the L0 hot path. |
| G0.4 | `test_slot_operator_rebuild.py` case 1 order (`A-2` before `B-0`) = the S3 acceptance bar; `test_slot_operator_complete_graph.py` (bug 614390) = the S4 bar? | Bounds S3 to "walked node + order" and S4 to real's `_eliminate_rebuilds`, no `prune_rebuilds`. |
| G0.5 | File #24b–#24e (+ `IUSE_EFFECTIVE`) as separate items now rather than growing #24? Recommended: yes. | Keeps #24 closable; the probe family is upstream-`xfail` territory. |
| G0.6 | `--rebuild-if-*` (`rebuild_if_entries`) moved through the same path in S3, or left as a synthesiser with a follow-up? Default: leave. | Same shape, same order bug, not #24's line. |
| G0.7 | `--debug` "backtracking due to missed slot abi update" narration and the `Dependency resolution took…` line: deliberate cuts, as for #23? Recommended: cut. | Stops S3 gold-plating. |

---

## 7. Definition of done

- `docs/024-oracle.md` with a verdict line; every v1 case MATCH, Rust
  == Python, no v1 xfails left (v2 shapes stay strict-xfail with their
  new item numbers).
- `emerge -p app-misc/A` → `[A-2, B-0]`; `-p -uD @world` → `[A-2,
  B-0]`; `slotundo-unnecessary` not rebuilt; `slotundo-changed-slot`
  rebuilt under `--changed-slot`; `parentdown` `[]`.
- No synthetic `GraphEntry` construction left for slot-op rebuilds;
  rebuilt rows carry real `deps`/`required_by`.
- Contract suite green both languages, failing-test names equal to the
  clean-`main` baseline; Rust unit tests for the binder and each undo
  rule; `cargo fmt --check`, `cargo clippy --release --all-targets`
  zero warnings, `cargo test --release`, `pytest tests -q`; counts in
  the final commit message.
- L0: no regressions vs the S0 archive; `rR` rows at real's position;
  findings filed. `bench/` not regressed.
- Backlog hygiene per S7; `what-this-proves.md` entry with citations
  and a runnable live example.

---

## 8. Review checklist (attach to every slice PR)

- [ ] Gate: fmt / clippy zero-warn / `cargo test --release` / `pytest tests -q` green; failing-test *names* vs clean-`main` baseline
- [ ] Rust and Python diffed empirically on all `fixtures/` **and** every `_b1_root` case (not only pytest)
- [ ] Every changed pin names its upstream test or L0 probe in the commit body
- [ ] S1: L0 raw diff shows only added `rR` rows / "causing rebuilds" blocks; bench unchanged
- [ ] S3: rebuilt rows have non-empty `deps`/`required_by` in `--json`; no `GraphEntry` construction left in `slot_operator_rebuild_entries`; `params_equal` covers the new field; scan runs once per pass behind `reachable`
- [ ] S4: nine rules in real's order (any reordering justified with a counter-example fixture or a proof of none); latch prevents re-scheduling; `abi_rebuilds` drops demoted consumers; skipped under `--emptytree` and live slot conflicts
- [ ] Comments carrying real citations moved, not dropped (`git diff` on `//` lines nets ≈ 0 except new rules)
- [ ] Docs: `what-this-proves.md` appended, backlog lines updated, `docs/024-oracle.md` table updated, no prior paragraph rewritten
- [ ] Judgment calls surfaced, not defaulted (list them)
- [ ] New fixtures `git add`ed before any `git clean`
- [ ] No `git commit`/`push` unless the user asked
