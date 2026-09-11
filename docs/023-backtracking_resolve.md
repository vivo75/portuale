# Refactor `backtracking_resolve`, then close backlog #23 — agent plan

Written 2026-09-11 against `main` @ `eef8494` (post #19/#22 merge). Two
phases in strict order: **Phase A** — a behaviour-neutral Rust-only
refactor of `rust/portage-repo/src/lib.rs::backtracking_resolve`;
**Phase B** — an oracle + backlog-hygiene slice that can run in parallel
with A; **Phase C** — backlog #23 proper, last.

**Read `AGENTS.md` and `docs/agent-context.md` first.** This file adds
only what is specific to this work. Every line number below drifts;
re-locate by function name before trusting anything (AGENTS.md step 1).

Model tiers used throughout:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Fable 5.1 / Opus 5 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" means a frontier model reads the full diff before the user
is asked to commit, regardless of who wrote it.

---

## 0. Review of the previous analysis — what stands, what changes

What was said before (chat, 2026-09-11) and holds after a re-read:

1. **The backlog one-liner under-describes #23.** The real function is
   `_slot_confict_backtrack` (upstream typo, one `l` missing) in
   `lib/_emerge/depgraph.py`. It does not "choose a version to mask";
   it produces a *ranked list of alternative mask choices* that
   `backtracking.py::_feedback_slot_conflict` turns into sibling
   `_BacktrackNode`s explored depth-first. `_slot_conflict_backtrack_abi`
   (the spelling the backlog uses) is the slot-operator rebuild path —
   that is #24, not #23.
2. **portuale's shape is the real blocker.** `backtracking_resolve` is a
   linear loop with one bundled mask trial (`MaskPhase` +
   `mask_trial_spent`). Ranking alternatives is pointless without a
   place to put the second-ranked one. #23 is therefore "node stack
   first, ranking second".
3. **The refactor is worth doing on its own** and the seam already
   exists: of the 18 cross-pass `let mut`s, 15 are written only in the
   post-walk decision block; the `'queue` walk writes just three
   (`autounmask_use_config`, `autounmask_use_change_records`,
   `autounmask_use_broke`), at two near-duplicate sites
   (`'parent_flip` block; already-resolved-slot re-check).

Corrections and sharpenings:

4. **Real masks children, never parents, in the slot-conflict path.**
   `_slot_confict_backtrack` only ever masks packages *in the conflicted
   slot* (plus `_iter_similar_available` siblings of them). Parent
   masking happens one node later via `_feedback_missing_dep` when the
   parent's atom no longer matches anything. portuale's trial masks
   the highest child **and** every puller with a lower alternative in
   one shot because it has no second node. So #23 is not "richer
   choice" but **"replace the bundled trial with real's two-mechanism
   search"**. Removing the puller-masking is a behaviour change with
   pinned tests behind it — it must be done only once the node stack
   (C2) makes the missing-dep path reach the same result, and every
   pin change must be checked against the oracle (B1), not re-pinned.
5. **Depth accounting differs.** Real's `--backtrack=N` bounds
   `mask_steps` (config-change nodes don't count). portuale's
   `backtrack_iteration` counts every retry including autounmask
   growth passes. Phase A keeps this as-is (neutral); C2 aligns it and
   this *will* move the `--backtrack=0/30` contract pins — list them
   before starting, verify each against the oracle.
6. **`get_best_run` matters for output.** When real exhausts its
   search it re-runs with the deepest terminal node that has *config
   changes but no masks*, so users never see a "masked by backtracking"
   graph unless nothing better exists. portuale's `Reverting` pass is
   a one-step approximation of this. C2 must reproduce
   `get_best_run`'s selection rule, not just "one clean pass".
7. **`_check_runtime_pkg_mask` (bug 375573)** — a node is discarded if
   every parent of a slot-conflict mask is itself masked. Not in
   portuale today. Belongs to C2.
8. **Oracle first is still right — but per the user's ordering it
   gates Phase C, not Phase A.** B1 can run in parallel with A; C does
   not start until B1 says there is a real-tree divergence worth
   fixing. If B1 finds none, stop after Phase A and park #23 with the
   oracle fixtures committed as regression guards.
9. **`--debug` "backtracking due to slot conflict:" narration** —
   real prints it; portuale's `resolver_trace.rs` does not. Not pinned,
   not required for parity of the merge list. Explicit non-goal of
   this plan; note it in #23's closure paragraph.

Numbers in this doc (line counts, variable counts, write-site counts)
come from `grep`/`awk` over `main`, not from compiling. They are
right to within a few; the *write-site* finding (point 3) was
confirmed by reading both sites.

---

## 1. Rules that apply to every slice

- **Phase A is Rust-only.** `agent-context.md` "Team structure": do not
  force the two languages into structural lockstep. The Python mirror
  (`python/emerge_pretend_reference.py::resolve_pretend_graph`) does
  **not** move in Phase A. Do reuse the same *phase names* in comments
  on the Python side (one-line `# phase: run_pass` style markers) so an
  agent can find the matching region — that is a doc-only change.
- **Phase C is dual-language.** Every behaviour change lands in Rust
  and Python in one commit, verified empirically by running both
  against `fixtures/` and diffing, then by `tests/test_emerge_pretend_contract.py`.
- **The full gate, every slice:** `cargo fmt --check`,
  `cargo clippy --release --all-targets` (zero warnings),
  `cargo test --release`, `python3 -m pytest tests -q`.
  Phase A additionally requires the **L0 byte-diff** (§1.1).
  Phase C additionally requires an L0 run with findings triaged.
- **Comments move with code.** ~977 of the function's ~2870 lines are
  comments carrying real-portage citations. An LLM refactor that drops
  or shortens them fails review. Reviewer: `git diff --stat` on comment
  lines should net to ≈ 0 in Phase A.
- **No `git commit`/`push` unless the user asks** (AGENTS.md step 9).
- **Surface judgment calls, don't default.** Especially: any pinned
  test that changes in Phase A (should be zero), and any pin change in
  Phase C not explained by an oracle fixture.
- **Docs per slice:** append to `docs/what-this-proves.md` (never
  rewrite prior paragraphs); update `docs/backlog-tasks.md` /
  `docs/scope-backlog.md` when a slice closes or changes an item.

### 1.1 The Phase A acceptance gate — "byte-identical everywhere"

Because Phase A changes no behaviour, the bar is stricter than green
tests:

1. Zero changes to pinned outputs in `tests/test_emerge_pretend_contract.py`.
2. `--debug` resolver-trace pins (`stage1_digraph_dump`,
   `stage3_candidate_list`, `stages_2_4_walk`) unchanged — they narrate
   the walk in walk order and catch silent reordering.
3. `bench/` gate does not regress (thin LTO + `codegen-units=1` are
   already set; extra function boundaries should inline).
4. **L0 raw-output byte diff:** before starting A1, run
   `TEST/run/l0-resolver.sh` on `main` and archive the raw per-probe
   portuale output (not just the parity score). After each A-slice,
   re-run and `diff -r`. Any byte of difference is a defect in the
   slice. (Real's side of the diff is irrelevant here — this compares
   portuale-before to portuale-after.)
5. The two fixture-driven Rust helpers `graph_result_real` and
   `graph_result_real_backtrack` in the `lib.rs` test module keep
   passing unchanged.

---

## 2. Phase A — refactor `backtracking_resolve` (Rust only)

Target shape (real `_backtrack_depgraph` ↔ `_create_graph` split):

```
fn backtracking_resolve(req) -> Result<GraphResult>
    let ctx    = ResolveCtx::new(req)?          // immutable per-call settings
    let mut bp = BacktrackParams::default()      // cross-pass state, Clone
    loop {
        let pass = run_pass(&ctx, &bp)?          // one full BFS walk
        match next_step(&ctx, &mut bp, &pass) {
            Step::Retry  => continue,
            Step::Settle => return assemble_result(&ctx, &bp, pass),
        }
    }
```

Do **not** start from the inner per-item body — that is where the
borrow-checker pain is (≈29 local closures, the `'parent_flip` labeled
block, ≈15 unlabeled `continue`s) and it does not create the seam #23
needs.

### A0 — Baseline capture and inventory (S)

- Run the full gate on `main`; archive L0 raw output (§1.1 item 4).
- Produce `docs/023-refactor-inventory.md`: for every `let`/`let mut`
  at function scope and at `'backtrack`-loop scope, a table with
  name, type, scope (call / cross-pass / per-pass), and the line
  ranges that *write* it (grep `\b<name>\b\s*(=|\.(insert|push|entry|extend|clear|retain|get_mut|remove))`).
  This is the map every later slice works from.
- Also list every `continue 'backtrack` site (8 today) with the
  condition guarding it, in source order — that order is the
  `next_step` decision chain and must be preserved exactly.
- **Model:** S for the greps and table; **M** sanity-reads the table
  once (a wrong scope classification here poisons A1–A3).
- **Acceptance:** doc exists, gate green (nothing changed).

### A1 — Introduce `ResolveCtx` and `BacktrackParams` (M, F review)

- `ResolveCtx`: every read-only value computed before the loop
  (`repos`, `root`, `vdb`, option booleans, `local_binpkg`,
  `backtrack_max`, `top_level`, `atoms`, …). Move the declarations into
  a struct constructor; replace uses with `ctx.field`. Pure rename.
- `BacktrackParams` (`#[derive(Clone, Debug)]`): the 18 cross-pass
  `let mut`s from A0's table. Keep `MaskPhase` inside it for now.
  Include a `## Real counterpart` doc comment mapping fields to
  `backtracking.py::BacktrackParameter` (`slot_constraints` ≈
  `runtime_pkg_mask` + slot-conflict constraints; `autounmask_use_config`
  ≈ `needed_use_config_changes`; the `mask_*`/`Reverting` trio has no
  real counterpart and is marked "portuale-only, removed in C2").
- No logic moves. `continue 'backtrack` sites untouched.
- **Traps:** a field that is *reset* each pass in today's code but
  declared outside the loop belongs to per-pass state, not
  `BacktrackParams` — A0's table decides. Don't let the model "tidy"
  the accumulator names.
- **Model:** M writes; **F review** focused on the scope table vs the
  struct split.
- **Acceptance:** §1.1 in full.

### A2 — Extract `next_step` and `assemble_result` (M, F review)

- `next_step(&ctx, &mut bp, &pass) -> Step`: move the post-walk chain
  in **exact source order**: (1) judge pending mask trial, (2)
  solvable-conflict `slot_constraints` growth, (3) unsolvable-conflict
  mask trial, (4) `_autounmask_breakage`, (5) autounmask growth
  re-run, (6) `_feedback_missing_dep`, (7) reverse-dep
  (`_slot_operator_check_reverse_dependencies`) retry, (8) anything
  else A0 listed. Each `continue 'backtrack` becomes `return Step::Retry`.
- `PassResult` (per-pass outputs): `entries`, `slot_conflicts`,
  `slot_want`, `slot_pullers`, `masked_deps`, `circular_deps`,
  `nvc_dep_atoms`, `missing_dep_trigger`, `autounmask_grew`,
  `dropped_pins` inputs, etc. — every per-pass value the decision
  chain or assembly reads. A0's table says which.
- `assemble_result`: the tail from the `for entry in &mut entries`
  post-processing through `return Ok(GraphResult{..})`.
- The walk stays inline in the loop body for this slice.
- **Traps:** (a) `nvc_count` is a closure over `entries` — make it a
  free fn. (b) `backtrack_config: Option<Config>` is rebuilt from
  `req.config` — keep that in `next_step`, and keep the
  `config = backtrack_config.as_ref().unwrap_or(&req.config)` shadow at
  the top of the pass. (c) Step (1) *precedes* the `slot_conflicts`
  extension by `build_residual_slot_conflicts` in today's code? — check
  A0's order; if residual conflicts are computed after the decision
  chain, they belong in assembly, not `PassResult`. Surface if unclear.
- **Model:** M writes; **F review** on decision-chain order.
- **Acceptance:** §1.1 in full. Expect the `-p --backtrack=0`, `=30`,
  `slot_conflict*`, `unsolvable_slot_conflict*`, autounmask-breakage
  pins to be the sensitive ones — run them first.

### A3 — Extract `run_pass` with a pass-local overlay (F)

The only Phase A slice where behaviour can change silently.

- Move the per-pass `let mut`s (24: `visited_atoms`, `resolved_slots`,
  `other_outcomes`, `root_deps_build_seen`, `slot_want`,
  `slot_pullers`, `entries`, `queue`, …) into `PassState`, and the
  `'queue` loop into `run_pass(&ctx, &bp) -> Result<PassResult>`.
- **The overlay.** The walk writes three cross-pass values in-pass and
  *reads them back within the same pass*: the `'parent_flip` block
  checks `autounmask_use_config` for an opposite flip already recorded
  (→ `autounmask_use_broke`) and whether the flip is new; the
  already-resolved-slot re-check does the same. Implement
  `PassState.use_overlay: HashMap<(cat,pkg), HashMap<flag,bool>>` plus
  `use_change_overlay: Vec<AutounmaskChange>` and `use_broke: bool`;
  every lookup consults **overlay then `bp`**, every write goes to the
  overlay. `run_pass` returns them in `PassResult`; `next_step` step (5)
  merges them into `bp` exactly where today's code already has them
  (today they are *already* in `autounmask_use_config` by then — the
  merge must be a no-op union with the same dedup rule
  `(atom, token)` for records, `bucket.get(f) != Some(want)` for flags).
- Consider (surface, don't decide) collapsing the two write sites into
  one `fold_use_flip(state, bp, key, flips, atom_form, dep_chain) -> Folded{Newly, Broke, Same}`
  helper. It is the natural place, but the two sites differ in one
  detail (the parent-flip site `break`s on conflict *before* inserting
  anything; the slot-reuse site inserts non-conflicting flags and
  flags the conflict). Preserve both behaviours or prove equivalence
  with a fixture.
- `run_pass` takes `&bp`, never `&mut bp`. That is the invariant this
  slice exists to establish; `clippy` will enforce it.
- **Traps:** `resolver_debug() && backtrack_iteration == 0` — the
  iteration counter is read in-walk for the debug trace; put it in
  `ctx`-like read-only form for the pass. `autounmask_grew` semantics
  ("newly added `(cp, flag)`") must be computed against `bp` ∪ overlay.
- **Model:** **F writes and a second F instance reviews** (fresh
  context, given only the diff + A0 table + this section). The
  autounmask-breakage pins pass either way if the overlay is
  slightly wrong in a way no fixture exercises — the reviewer's job is
  to construct the counter-example fixture or argue there is none.
- **Acceptance:** §1.1 in full, plus one new fixture if the reviewer
  found a gap.

### A4 — Split the per-item walk body (M)

- `process_item(&ctx, &bp, &mut state, item) -> ItemFlow` with phases
  as separate fns: blocker split → visited/dedup → candidate selection
  (incl. binary/usepkg) → `'parent_flip` → changed-deps report →
  slot reuse & conflict record → metadata/USE/REQUIRED_USE → autounmask
  license/keyword → entry push → dependency expansion & enqueue.
  Unlabeled `continue`s become early `return ItemFlow::Next`;
  `continue 'queue` from inside `'parent_flip` becomes a return value.
- Borrow strategy: `PassState` with independently borrowed fields,
  passed as `&mut PassState`; closures that capture many locals become
  fns taking `&PassState`. Do not introduce `Rc<RefCell>`.
- **Model:** M; **F review** only on the `'parent_flip` and slot-reuse
  extraction. This slice is mostly mechanical once A3 is in.
- **Acceptance:** §1.1 in full.

### A5 — Docs closure for Phase A (S)

- Append one paragraph to `what-this-proves.md`: the new shape, the
  invariant (`run_pass` is `&bp`), pointer to the inventory doc.
- Add the `# phase:` markers in `emerge_pretend_reference.py::resolve_pretend_graph`
  (comment-only).
- Update `docs/operation-diagrams.md` if it names `backtracking_resolve`
  internals.
- **Model:** S. **Acceptance:** gate green, diff is comments/docs only.

---

## 3. Phase B — oracle + backlog hygiene (parallel with A)

### B1 — Real-portage oracle fixtures for slot-conflict backtracking (F brief, M execution)

Goal: know *before* Phase C whether portuale's merge lists diverge
from real on the cases `_slot_confict_backtrack` was written for.

- Sources: upstream `lib/portage/tests/resolver/`:
  `test_slot_conflict_mask_update.py`, `test_missed_update.py`,
  `test_slot_conflict_update_virt.py` (bug 692746),
  `test_backtracking.py`, `test_aggressive_backtrack_downgrade.py`,
  `test_slot_conflict_update.py`, `test_solve_non_slot_operator_slot_conflicts.py`.
  Translate each `ResolverPlayground` ebuild/installed set into
  `fixtures/repo/…` (+ `metadata/md5-cache/…`) and `fixtures/vdb/…`,
  checking name collisions with existing `dev-libs/slotconf*`/`bt*`
  fixtures first.
- Record real's expected merge list from the upstream test's
  `ResolverPlaygroundTestCase(..., mergelist=[...])` — that *is* the
  oracle, no container run needed for this step.
- Run portuale on each; produce `docs/023-oracle.md` with a table:
  case, real mergelist, portuale mergelist, match Y/N, which real
  mechanism the case exercises (child mask / similar-pkg grouping /
  375573 discard / get_best_run / mask_steps bound).
- Also grep `TEST/findings/l0.md` and re-run
  `TEST/run/l0-resolver.sh` looking specifically for probes whose
  divergence is in a slot-conflicted package's *version choice*; today
  the file attributes no cluster to this. Record the answer either way.
- **Go/no-go rule:** if every oracle case matches and L0 shows nothing
  attributable, **Phase C is parked** — commit the fixtures as
  regression guards, update #23 to "no known divergence; parked with
  oracle", stop. Do not do C for parity's sake.
- **Model:** F writes the translation brief (which cases, which
  mechanism each isolates) — ~1 page; **M** translates fixtures and
  runs them; **S** fills the table. F reads the table.
- **Acceptance:** `docs/023-oracle.md` with a verdict line; fixtures
  pass `pytest` as *cases* (pinned to portuale's *current* output,
  clearly labelled `# oracle-divergent, see 023-oracle.md` where they
  differ from real).

### B2 — Fix the #23 backlog line and carve out inherited items (S)

- `docs/backlog-tasks.md` #23: correct the function name to
  `_slot_confict_backtrack` (upstream spelling), file
  `lib/_emerge/depgraph.py` + `lib/_emerge/resolver/backtracking.py`,
  note that `_slot_conflict_backtrack_abi` is #24's.
- Split the inherited `conflict_downgrade`/`installed_downgrade`
  guards (`dep_check.py::dep_zapdeps`, bug 531656) into a new Tier-2
  item #35 "downgrade_probe + live graph_db for dep_zapdeps" so #23's
  closure is not blocked on them.
- Reword #23's one-liner to the C1–C4 shape below.
- **Model:** S. **Acceptance:** doc diff only.

---

## 4. Phase C — backlog #23 (dual-language, after A and B1's go)

Precondition: A1–A3 merged (A4 optional), B1 verdict = "go", B2 done.
Python mirror changes are mandatory in every C slice.

### C1 — `BacktrackParams` ↔ real `BacktrackParameter` alignment (M, F review)

- Split `slot_constraints` into `slot_constraints` (solvable-conflict
  atom sets, portuale-specific) and `runtime_pkg_mask: HashMap<cpv, MaskReason>`
  with `MaskReason::{SlotConflict(parents), MissingDependency(...), …}`
  matching real's `runtime_pkg_mask` dict shape. Today `!=cpv`
  negatives live inside `slot_constraints` — move them.
- Add `mask_steps: u32` and `depth: u32` alongside
  `backtrack_iteration`; do **not** change which one gates retries yet.
- Python: mirror the split in `resolve_pretend_graph`'s state dicts.
- **Acceptance:** zero pin changes (still neutral). §1.1 L0 byte-diff.
- **Model:** M; F review on the `MaskReason` shape (it is the data C2
  and C3 build on).

### C2 — Replace `MaskPhase` with a `Backtracker` node stack (F)

The real work.

- New `struct Backtracker { nodes: Vec<Node>, unexplored: Vec<Node>, current: usize, max_depth }`
  with `Node { params: BacktrackParams, depth, mask_steps, terminal }`,
  porting `backtracking.py` verbatim in semantics: `_add` (dedup by
  `params` equality, `mask_steps <= max_depth`, `_check_runtime_pkg_mask`
  bug 375573), `get` (DFS pop), `feedback` (config vs slot-conflict vs
  missing-dep, "at most one of the latter two per restart",
  first-conflict-only), `backtracked`, `get_best_run`.
- Driver: `backtracking_resolve` becomes real's `_backtrack_depgraph`:
  `while let Some(params) = bt.get() { pass = run_pass(&ctx,&params); if success → return; bt.feedback(pass.backtrack_infos) }`,
  then if exhausted `run_pass(&ctx, &bt.get_best_run())` and report.
  `next_step`'s steps (2),(4),(5),(7) become *config* feedback
  (`depth += 1`, `mask_steps` unchanged, `terminal` per real); (6)
  becomes `_feedback_missing_dep`; (3) becomes `_feedback_slot_conflict`
  **but for this slice still emits exactly one child = today's bundled
  mask set**, so outputs stay pinned. (1) and `Reverting` are deleted —
  `get_best_run` replaces them.
- `--backtrack=N` now bounds `mask_steps`. **This moves pins** for
  `--backtrack=0`/`=30` and for cases where autounmask growth passes
  previously ate the budget. Enumerate them before coding; for each,
  the new output must match either a B1 oracle fixture or a hand-run
  of real portage in the `TEST/` container. A pin that changes with no
  oracle behind it is a stop-and-surface event.
- Python: same `Backtracker` class, same driver.
- **Model:** **F writes**, **second F reviews** with the upstream
  `backtracking.py` open side by side. **M** may write the Python
  mirror from the finished Rust diff. **S** runs the gate and diffs
  fixtures across languages.
- **Acceptance:** gate green; every pin change justified in the commit
  body by an oracle case id; L0 run with no new divergence cluster;
  `what-this-proves.md` paragraph with a live-verified example.

### C3 — Real mask-choice generation (M, F review)

- Port `_slot_confict_backtrack` into a free fn
  `slot_conflict_mask_choices(ctx, pass, conflict) -> Vec<Vec<(cpv, parent_atoms)>>`:
  existing node always included (692746), sort conflict pkgs by version
  desc, per-candidate `conflict_atoms` = all-parents minus those matching
  it, stable sort by `len(conflict_atoms)`, return the full ranked list
  (real feeds *all* choices as siblings; DFS pops the last-added first).
- `_feedback_slot_conflict` now emits one node per choice. **Remove
  the puller-masking** from the slot-conflict path; verify with B1
  fixtures that `_feedback_missing_dep` on the next node reaches the
  parent-downgrade real produces. If a B1 case regresses because
  portuale's missing-dep feedback is narrower than real's, surface it
  — that is a separate gap, not a reason to keep puller-masking.
- `_iter_similar_available` grouping is **not** in this slice (C4).
- Python mirror in lockstep.
- **Model:** M writes (the ranking is ~80 lines with a precise spec);
  **F review** on the puller-masking removal and on every pin change.
- **Acceptance:** as C2; B1 table re-generated, matches should go up,
  none may go down.

### C4 — `_iter_similar_available` grouping + closure (M, then S)

- Port `_iter_similar_available` (same `cp`, same slot atom,
  visible, `autounmask_level` gating as portuale models it) and the
  `similar_pkgs` grouping loop so missed-update siblings are masked in
  the same node. Python mirror.
- Re-run B1 table and L0; update `docs/023-oracle.md` verdicts.
- Closure docs: `backlog-tasks.md` #23 → DONE with the non-goals stated
  (`--debug` narration; "masked by backtracking" listing in
  `_show_unsatisfied_dep` if still unported — check; #35 downgrade
  guards); `scope-backlog.md` Part 4 item 1 reworded; `what-this-proves.md`.
- **Model:** M for the port; S for docs; F reads the final oracle table.
- **Acceptance:** gate green; oracle table all-match or each mismatch
  filed as a named backlog item.

---

## 5. Sequencing and gates

```
A0 ─ A1 ─ A2 ─ A3 ─ (A4) ─ A5
                 │
B1 ──────────────┴─ go? ─ C1 ─ C2 ─ C3 ─ C4
B2 ───────────────────────┘
```

- A0–A5 and B1/B2 are independent; run B1 on a branch that does not
  touch `lib.rs` (fixtures + docs only) so it merges cleanly.
- C1 needs A3 (the `&bp` invariant) — without it the overlay and the
  node stack fight.
- `explore/dfs-graph-backtracker` (31 lines in `lib.rs`, DFS queue
  order under a flag) will conflict with A3; rebase it after A3 or
  fold the flag into `ResolveCtx` during A1.
- #19's remaining slices 4–6 (rendering, error-block wiring) touch
  `assemble_result`'s territory. Land A2 before those, or schedule
  them after A5.

Effort (frontier-hours of agent time, order-of-magnitude): A0 ½, A1 1,
A2 2, A3 3–4, A4 3, A5 ½, B1 2–3, B2 ¼, C1 2, C2 6–10, C3 3, C4 2.

---

## 6. Review checklist (attach to every slice's PR description)

- [ ] Gate: fmt / clippy zero-warn / cargo test / pytest — all green
- [ ] Phase A only: L0 raw byte-diff empty; `--debug` trace pins untouched
- [ ] Comment lines net ≈ 0 in Phase A (`git diff | grep -c '^[-+]\s*//'` both sides)
- [ ] Every `continue 'backtrack` / `Step::Retry` still in A0's order
- [ ] `run_pass` signature takes `&BacktrackParams` (A3 onward)
- [ ] Phase C: every changed pin names its oracle case in the commit body
- [ ] Phase C: Rust and Python diffed empirically on all `fixtures/` (not only pytest)
- [ ] Docs: `what-this-proves.md` appended, backlog lines updated, no prior paragraph rewritten
- [ ] Judgment calls surfaced, not defaulted (list them)

