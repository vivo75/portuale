# Plan: backlog #19 — DFS-partial merge-list truncation (the abort path)

Status: proposed. Owner decision required at Gate 0 before any code lands.
Scope: portuale `emerge --pretend` resolver, dual-language (Rust + Python
reference), contract-pinned, per `AGENTS.md`.

This document is written for agents. Read `AGENTS.md`, `docs/agent-context.md`
and `docs/scope-backlog.md` §A first. Every slice below follows the standing
rules: Rust and Python land together, fixtures live in `fixtures/`, behaviour
is pinned in `tests/`, `what-this-proves.md` gets its cited entry, and
`scope-backlog.md` is updated when an entry closes. Nothing here overrides
those rules.

---

## 0. Problem statement (re-derive, don't trust)

Real portage's `depgraph` walk is a recursive DFS. When it hits an unfixable
state it *abandons* the resolve. Observable effects (verify each against the
vendored `3rdparty/portage/lib/_emerge/depgraph.py` and record line numbers):

1. The merge list printed is the **partial** list — what the walk had
   accepted before the abort — not the full closure.
2. The `Total: N packages (…)` counters are cumulative over that partial list.
3. The error block (masked-dep disclosure, circular-dep report, etc.)
   follows the partial list.
4. Exit status is 1.

portuale today: single-pass BFS, "report, don't enforce" — full merge list,
notice appended, exit 0.

Three backlog entries in `scope-backlog.md` §A are parked on this one
mechanism:

- masked-dependency **abort** half (disclosure half shipped 2026-09-10);
- circular-dep **partial flat list + cumulative counters**;
- #19 itself, the DFS-partial truncation.

Therefore #19 is built as **one abort path with three consumers**, not as a
list-truncation hack.

**First task of Slice 1 is to locate and read the parked branch.** The item
text was compacted into `docs/history/scope-backlog-<date>.md`; `git branch -a`
and `git log --all --grep=DFS` / `--grep=truncat` will find the branch. Record
in the slice notes *why* it was parked. Do not delete or rebase it.

---

## Gate 0 — owner decisions (human, not agent)

Do not start Slice 2 until these are answered in writing (a short section
appended to `scope-backlog.md` §A is enough).

| # | Question | Why it matters |
|---|----------|----------------|
| G0.1 | Does portuale adopt real's **exit 1 on abort**, or keep the exit-0 convention with the partial list? | Every consumer flips on this. It is a product decision, not a parity detail. The existing "reported residuals stay informational, exit 0" convention for slot conflicts must be explicitly reconciled. |
| G0.2 | Is **byte-parity of the truncation point** a goal, or is "same membership, deterministic order" acceptable? | The truncation point is a function of real's DFS visit order. Matching it may require a DFS side-channel in a BFS walk. Deciding this up front prevents Slice 3 from open-ending. |
| G0.3 | Which abort shapes are in scope for v1? Proposed: (a) masked-only dependency, (b) `_serialize_tasks` unserializable cycle, (c) unsatisfiable atom mid-walk. | Bounds the fixture set and the L0 triage. |
| G0.4 | May the abort path be gated by a flag during rollout (e.g. `PORTUALE_ABORT_PATH=0` fallback), or must it land unconditionally? | Affects Slice 2 plumbing and the L0 comparison strategy. |

---

## Invariants for every slice

- **Do not touch merge order.** `merge_order.rs` (`_serialize_tasks` port,
  `SerializeFrontier`) shipped 2026-09-06..10 and is L0-validated. The abort
  path consumes its output; it must not change its output on the non-abort
  path. Add a test that asserts identical merge lists on every existing
  fixture before and after each slice.
- **Dual-language lockstep.** `emerge_pretend_reference.py` mirrors every
  behavioural change in the same commit. Rust-only shortcuts are rejected.
- **Fixtures, not real-tree data, for contract tests.** The real tree is the
  oracle (Slice 1) and the regression bed (Slice 6), never a test input.
- **Determinism.** No timing, no set-iteration order. If real's output is
  nondeterministic at some point (as with the `backtrack: N/M` timing line),
  document the deliberate cut rather than approximate it.
- **Cite.** Every behavioural claim about real portage in a commit message or
  `what-this-proves.md` entry carries `file:line` into the vendored checkout.
- **Stop conditions.** Each slice lists one. If hit, stop, write the finding
  to the slice notes, and hand back to the owner. Do not "work around".

---

## Slice 1 — Oracle and spec (no product code)

**Model tier:** frontier for 1.3–1.4; mid-tier for 1.1–1.2, 1.5.
**Output:** `docs/abort-path-spec.md` + fixtures + xfail contract tests.

### 1.1 Recover the parked branch
- Find it (`git branch -a`, `git log --all --oneline | grep -i -E 'dfs|truncat|abort'`).
- Write a 10–20 line summary: what it changed, what tests it added, why it
  was parked (quote the commit/notes). Put it at the top of the spec.

### 1.2 Build fixtures (one per abort shape in G0.3)
Under `fixtures/`, synthetic repo + vdb + profile, following the existing
`dev-libs/cyc4a`–`cyc4d` and `dev-libs/fucycle*` precedents:
- `dev-libs/abort-masked-*`: a target whose dep atom matches masked-only
  ebuilds, with at least two *other* deps that a DFS would visit **before**
  and **after** the masked one, so the partial list is distinguishable from
  the full list.
- `dev-libs/abort-cycle-*`: an unserializable cycle (hard `DEPEND` both ways,
  no `PDEPEND` escape) plus unrelated leaves before/after.
- `dev-libs/abort-unsat-*`: a versioned atom with no matching visible ebuild,
  same before/after arrangement.
Each fixture must also have a "sibling" variant where the failing dep is the
*last* thing visited, so partial == full membership and only exit code and
counters differ. This isolates the two behaviours.

### 1.3 Capture real portage output
In the `TEST/` container against real 3.0.82.2, run each fixture with
`emerge -pv`, `-pvt`, `-pv --columns`, and `--pretend --debug`. Save
stdout, stderr, and exit code under `fixtures/.../expected/real/`.
Record the exact portage version and the command line.

### 1.4 Read the source and write the spec
Read, and cite by line, the paths that produce the outputs captured in 1.3.
Starting points (verify; do not assume these are the right functions):
- `depgraph.py`: `_add_pkg` / `_add_dep` recursion (where the walk aborts
  and what state is retained), `altlist()`, `display()` /
  `_show_merge_list` (whatever renders the partial list), `display_problems()`,
  `_show_unsatisfied_dep`, `_show_circular_deps`.
- `_serialize_tasks`: the branch that gives up on a cycle and what `retlist`
  contains at that point (`_prepare_reduced_merge_list` is already ported —
  note how the partial list relates to it).
- `resolver/output.py`: how counters are computed from the list passed in.
- `actions.py` `action_build`: how the abort maps to exit status.

The spec must answer, for each shape, in plain prose:
- **Membership:** which nodes are in the partial list (accepted-before-abort?
  including or excluding the failing node's already-visited children?).
- **Order:** is it DFS acceptance order, or is the partial list still passed
  through `_serialize_tasks`?
- **Counters:** which of `Total / new / upgrades / … / Size` are affected.
- **Exit code** and where it is set.
- **Interaction with `--backtrack`:** does the abort happen inside the retry
  loop (after N backtracks) or on the first pass? This decides where the hook
  goes in portuale's `'backtrack` loop.

### 1.5 Pin as xfail contract tests
In `tests/`, one test per fixture × output mode, marked `xfail(strict=True)`
with the real-captured output as the expected value. Rust == Python is
asserted *now* (both currently produce the same wrong thing) so lockstep is
protected from day one.

**Stop condition:** if 1.4 finds that the partial list's membership depends on
something portuale does not model at all (e.g. real's `_dynamic_config`
reinstall bookkeeping), write it down and stop before Slice 2. G0.2 may need
revisiting.

---

## Slice 2 — Data model and plumbing (behaviour-neutral)

**Model tier:** mid-tier. Spec from Slice 1 is the input; no source reading
of real portage should be needed.

- Add an abort outcome to `GraphResult` on both sides. Suggested shape:
  ```
  enum ResolveOutcome { Complete, Aborted { reason: AbortReason, partial: Vec<NodeId> } }
  enum AbortReason { MaskedDep{…}, UnserializableCycle{…}, UnsatisfiedAtom{…} }
  ```
  Python mirror: a small dataclass in `emerge_pretend_reference.py`.
- Thread it `ResolveRequest → walk → GraphResult → pretend::run → exit code`,
  and through `mrg`'s `to_emerge_argv` path (nothing to do there if it
  literally reuses `pretend::run`; confirm and note it).
- Behind the G0.4 gate, add the exit-code mapping. With the gate off (or if
  G0.4 says "unconditional", with the outcome never produced yet) every
  existing test is byte-identical. Run the full contract suite + `cargo test`
  + clippy + fmt and paste the counts in the commit message.
- Add the "merge order unchanged" regression test from the invariants.

**Deliverable:** one commit, zero behavioural change, xfails still xfail.
**Stop condition:** any existing test changes output.

---

## Slice 3 — Partial-list membership and order

**Model tier:** frontier. This is the substantive slice.

Depending on G0.2:

**3a. "Same membership, deterministic order" (recommended first):**
- Define the accepted-before-abort set from portuale's own walk: the nodes
  the BFS had admitted to the graph when the abort condition fired.
- Compare with the membership the spec derived for real. Where BFS admits
  nodes real's DFS would not have reached yet, the difference is by
  construction; document it and check whether the *sibling* fixtures (failing
  dep last) already give full parity — they should.
- Order the partial list through the existing `serialize_merge_order` so it
  is at least a valid topological prefix.

**3b. "Byte-parity of the truncation point" (only if G0.2 says so):**
- Add a DFS visit-order side channel to the walk (a counter stamped on each
  node in real's `_add_pkg` recursion order) without changing which nodes are
  admitted. This must be a pure annotation; the merge-order regression test
  from Slice 2 is the guard.
- Truncate the accepted set at the stamp of the failing node.
- Expect this to need 2–3 iterations against the real captures; keep a
  divergence log in the slice notes.

In both variants, flip the relevant xfails to passing one fixture at a time.

**Stop condition:** 3b requires changing which nodes the BFS admits. That is
a resolver-architecture change and is out of scope here; hand back.

---

## Slice 4 — Rendering: partial flat list + cumulative counters

**Model tier:** cheap/mid. The `resolver/output.py` port already exists.

- Feed the partial list (Slice 3) into the existing output layer for `-p`,
  `-pv`, `-pt`, `--columns`.
- Counters computed over the partial list only; verify against the 1.3
  captures for every output mode.
- The circular-dep "cycle members re-display as their own flat list" logic
  shipped 2026-09-10 must now sit *after* the partial list; confirm order
  against the capture.
- `--json` provenance: add an `aborted` field with the reason; keep the
  partial list as the `merge_list` array. Pin it.

---

## Slice 5 — Wire the three consumers, flip exit codes, close entries

**Model tier:** mid-tier.

1. **Masked-dep abort:** the disclosure block (shipped) is now emitted as the
   `AbortReason::MaskedDep` error block after the partial list; exit per G0.1.
2. **Circular-dep partial list:** `_serialize_tasks`' give-up branch produces
   `AbortReason::UnserializableCycle`; the `large_cycle_count` trailer and
   suggestions follow as today.
3. **#19 proper:** any remaining abort site (unsat atom) routes through the
   same outcome.
4. Turn the remaining xfails into plain tests. `xfail(strict=True)` means a
   pass is loud — none may be left marked.
5. Docs: `what-this-proves.md` entry (cited), `scope-backlog.md` §A: remove
   the three parked items, add the deliberate cuts found in Slices 1/3 (e.g.
   BFS-vs-DFS membership difference if G0.2 chose 3a), Part 4 item 1 updated.
6. Reconcile the slot-conflict "residuals stay informational, exit 0"
   convention with G0.1 in the same commit — either it changes too, or the
   backlog says explicitly why not.

---

## Slice 6 — Real-tree validation (L0) and triage

**Model tier:** frontier for triage; the run itself is scripted.

- Rebuild the container image if the base changed; run `TEST/run/l0-resolver.sh`.
- Baseline before Slice 5 was 96/120 clean (2026-09-09), with the 24
  divergent being merge-order timing or "the parked backtracking-disclosure".
  Expected: the disclosure-related divergences clear; nothing else moves.
- For every atom whose status changed *in either direction*, write a finding
  in `TEST/findings/` with the diff, the abort reason (if any), and a verdict:
  fixed / regression / new known-divergence. Regressions block the merge.
- Adjudicated non-bugs go to `TEST/compare/known-divergences.yaml` with a
  one-line reason.
- Run L1 as well if anything in the non-`--pretend` path was touched (it
  should not have been; confirm with `git diff --stat`).

---

## Model routing summary

| Slice | Work | Tier | Why |
|-------|------|------|-----|
| 1.1, 1.2, 1.5 | branch archaeology, fixtures, xfail tests | mid | pattern-following, existing precedents |
| 1.3 | container capture | any / scripted | mechanical |
| 1.4 | read depgraph.py, write spec | **frontier** | ~1–2k lines of upstream, order-sensitive reasoning |
| 2 | plumbing | mid | fully specified, behaviour-neutral |
| 3 | membership/order | **frontier** | traversal-order reasoning against a BFS |
| 4 | rendering | cheap/mid | output layer already ported |
| 5 | wiring + docs | mid | mechanical once 3/4 hold |
| 6 | L0 triage | **frontier** | honest adjudication, regression detection |

Do not give a cheaper model Slices 1.4, 3 or 6 to "save cost": the known
failure mode — matching fixtures while diverging on the real tree — is what
parked the branch originally.

---

## Definition of done

- All three §A parked items removed from `scope-backlog.md`, with cuts listed.
- Contract suite: every abort fixture passes, Rust == Python, no xfails.
- `cargo fmt --check`, `cargo clippy --release --all-targets` zero warnings,
  `cargo test --release`, `pytest tests -q` all green; counts in the final
  commit message.
- L0: no regressions vs the 96/120 baseline; disclosure-related divergences
  reduced; findings filed.
- Merge-order regression test still green.
- `what-this-proves.md` entry with line citations into the vendored checkout.

