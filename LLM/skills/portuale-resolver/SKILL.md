---
name: portuale-resolver
description: Use when changing or debugging dependency resolution in portuale — merge order, backtracking, slot operators, virtuals, USE deps, blockers, @world updates, demotion and seeds, post-skip re-resolution, the pubgrub bridge, or a merge list that differs from Portage's.
---

# The resolver

Two engines, one contract: the **default walk**, whose output order is
`topological_merge_order`, and **`--solver=pubgrub`**, bridged to external
crates through `solver_bridge.rs`. Both are judged against the charter's
properties: every kept package has its dependencies, no blocker or
`REQUIRED_USE` is violated, no slot holds two packages, build dependencies come
before the package that needs them, and each package ends as new as the others
allow. Portage's merge list is the comparison, not the target: a different set
or order is a defect only if one of those properties fails. A difference that
is understood is recorded in the bed's `known-divergences.yaml` (a portuale
bug is fixed, never listed).

## Merge order is not a topological sort

This is the single most expensive misunderstanding in this area. Portage's
order comes from `_emerge/depgraph.py::_serialize_tasks`, which:

1. builds a `digraph` with a **dependency priority per edge**
   (`DepPriority`, `DepPriorityNormalRange`, `DepPrioritySatisfiedRange`);
2. decides leaf-ness while progressively **relaxing** which dependency kinds
   still constrain;
3. applies an **order bias** on nodes (`_merge_order_bias`);
4. has a special `asap_nodes` path;
5. on a cycle, does not topological-sort — it **selects** a cycle by a
   specific policy.

Two implementations can therefore produce the same graph and different orders,
both legal. The previous Kahn walk with a discovery-rank tie-break reproduced
the *set* and not the order — that history is why `merge_order.rs` exists
(`LLM/Paragone_solver_portage.md`).

Any change here is judged by **traces**, not by reading: capture the walk with
`PORTUALE_BT_TRACE=1`. For a refactor, diff before against after (it must be
empty). To learn why a list differs from Portage's, diff against Portage's
`--debug` (`pd.out`) and name the pass that differs, then judge the difference
by the charter — is the order legal, is every package the newest that fits —
not by equality. Order legality is also checked without a reference
(`output_invariants.py`: "a dependency merged after its owner").

## The dominant bug family: the silent drop

Roughly 40 core issues, ~285 visited and deduplicated
(`LLM/recurring-issues.md`). The shape is always the same: a real dependency is
treated as "not a dependency" — no entry, no `NoVisibleCandidate`, no warning.

The choke points were `resolve/pass/prepare_atom.rs` (an unparsable atom ended
the walk of that dependency), `resolve/queue.rs` (`enqueue_flat_deps`: enqueue
everything, drop on pop) and the unparsed-token handling in
`crates/portage-repo/src/globals.rs`.

**This family is now guarded by a program** (backlog #492), and the guard is the
rule to follow:

- A new dependency-token parse uses `portage_repo::parse_dep_atom(token,
  "<site>")`, which **counts** every unparsable token per site; counting is
  always on, debug builds also name the token on stderr.
- Where skipping is intended, keep `portage_dep::parse_atom` and add
  `// drop-ok: <reason>` on the same or the previous line. The source lint
  `every_dep_atom_parse_is_counted_or_justified`
  (`crates/portage-repo/src/tests/dep_drop_lint.rs`) fails otherwise.
- `PORTUALE_REPORT_UNPARSED_DEP_TOKENS=1` prints the total and the per-site
  counts; the L0 bed and the contract suite assert that total is zero
  (`check_unparsed_dep_tokens` in `output_invariants.py`).

Historical instances, for orientation only: slot ops `foo:0=` (`what-this-proves.until-2026-11-07.md:61-66`), USE deps
(`:74-77`), slot visited-set (`:396`), sub-slot feed (`:1229`), blockers `!!`
(`:1473`), `package.provided` (`:8504`), nested fragments (`:17303`),
`[use]`-unsat (`:8461`).

The invariant that governs every fix is **never silently wrong**
(`LLM/code-notes/portage-use-reduce/lib.md:77,113`; charter properties 1 and
5). A dependency portuale cannot honour must produce a visible outcome — an
entry, a candidate, or a message that names the cause and an action — not
silence. Prefer extending a guard like the one above over adding a one-off fix.

## The second family: a scope cut whose doc outlived it

A v1 cut is documented, later closed, and the paragraph is frozen because the
record is append-only. Traps of this shape accumulate in the module docs of
`portage-dep`, `portage-repo`, `ebuild_*` and in `what-this-proves.md`
(`LLM/recurring-issues.md` §2, ~102 + 53 narrowings). Before treating a
"documented cut" as current, check the backlog entry it points at.

## Mechanisms you will meet

| Mechanism | What to know |
|---|---|
| **Slot operators** (`:0=`, `:*`) | the rebuilt operator form and its bare form are different pins; a skip-block pin must render both |
| **Virtuals** | `virtual` expansion is per **version**, not per atom; nested and USE-conditional arms each change the shape |
| **Demoters** | a demotion is recorded at commit time; a *novel higher-in-slot demoter* selected after the skip is invisible to the commit-time void (#652) |
| **Seeds and skips** | `seed_queue` skip-if-demoter-masked; `select_expansion_arms` with `keep_demoted` |
| **Post-skip re-resolution** | the graph is re-resolved after a skip; the recheck gate (#655, #656) is what stops a void from re-firing |
| **Backtracking** | `backtrack: N/M` is part of the report; a `--debug` row therefore carries a count line Portage never prints |
| **Blockers** | a masked block is a *display* outcome as much as a resolution one |
| **`@world`** | a world update is a different entry path; `--update`, `--deep`, `--changed-deps` change which parameters the walk sees |
| **`package.provided`** | provides an atom with no package; the drop there is the classic silent failure |
| **Pubgrub bridge** | over-merge in the bridge, not the engine: bare targets became every slot, installed packages expanded as if `--deep`, build deps dropped, no `R` displayed |

## Parameter plumbing

`_emerge/create_depgraph_params.py` is where CLI flags become resolver
parameters; portuale's counterpart is the `--update`/`--deep`/bdeps parameter
set. `--usepkg` is special: Portage's `bdeps=auto` keys on the **presence** of
the flag, so `--usepkg=n` still counts (`LLM/2026-10-15-batch-plan.md`).

## Traps

- **Unifying two serializers by deletion.** Flat output and `--tree` use
  different logic (greedy leaf pop is off in tree). Never unify their
  predicates by removing code.
- **A memo key that misses a field** reads through to a stale result: the key
  must hash everything the body reads, including fields reached via helpers.
- **`Ok(None)` and "not configured" are different states.** Any reader that
  distinguishes them must be audited the same way when a synthetic base layer
  is introduced.
- **A heal that fires where Portage aborts** is a difference that needs a
  reason. It is acceptable only if the resulting plan satisfies the charter
  properties; do not assume it is a fix, and do not assume it is a bug.
- **Backtrack count is deliberately unasserted** (#572 precedent) — do not pin
  it.
- **Order changes without a trace diff** hide in "no output change" claims.

## Where to look

| Question | Source |
|---|---|
| The recurring families, with line citations | `LLM/recurring-issues.md` |
| Why the order differs from a topological sort | `LLM/Paragone_solver_portage.md` |
| The pubgrub bridge and its four defects | `LLM/04.34-pubgrub-parity.opus.md` |
| World updates, `--update`, demoters | `LLM/02.359-default-solver-update-parity.opus.md`, `LLM/2026-10-20-world-plan-preelaboration.md` |
| Per-item mechanisms and their evidence | `LLM/02.6*.opus.md`, `LLM/evidence/<date>-<item>/` |
| What Portage does, exact lines | skill `portage-source-map` |

## Provenance

The documents this skill was distilled from, and how many verified facts each
contributed: [`references/sources.md`](references/sources.md).
