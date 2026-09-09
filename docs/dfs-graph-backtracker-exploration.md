# DFS-ordered graph + `get_best_run` — exploration notes

Branch `explore/dfs-graph-backtracker`. Goal: reproduce real portage's
**partial merge list** when a resolve is abandoned (autounmask changes +
an unresolvable circular dependency → `plasma-meta` shows 4 packages,
`podman` 8, where portuale shows the full 487 / 43).

## Step 1 — flag-gated DFS resolution walk (`PORTUALE_DFS_WALK=1`)

`portage_repo::set_dfs_walk` (env-free process-global, same pattern as
`RESOLVER_DEBUG`). The `'queue` loop in `backtracking_resolve` drains its
`VecDeque` LIFO (`pop_back`) instead of FIFO (`pop_front`).
`enqueue_flat_deps` already pushes a package's deps in forward declared
order, so `pop_back` expands the last-declared sibling's subtree first —
exactly real's `_dep_stack` (`depgraph.py::_create_graph`).

### What it changed

| probes | result |
|---|---|
| 109 / 117 L0 atomlist entries | **byte-identical** BFS ↔ DFS |
| firefox, thunderbird, gimp | same package **set**, merge **order** shifts a few lines |
| gnome-shell, nautilus | order shifts more (~60 lines); parity vs real essentially unchanged (nautilus 200→198, gimp 184→184, firefox 168→168, thunderbird 172→172) |
| plasma-meta | 487 → 484 (DFS drops `dev-lang/yasm`, `media-libs/libaom`, `media-libs/libavif` — pulled via the `kimageformats[avif]` autounmask node) |
| contract suite (`PORTUALE_DFS_WALK=1`) | **38 fail / 972 pass** |

### The 38 contract failures are all order-of-representation, not wrong answers

Same package sets, same versions, same USE — only the *order* in which
things are walked/reported changes:

| family | count | why |
|---|---|---|
| slot conflicts (`slot_conflict*`, `unsolvable_slot_conflict*`, `--backtrack=0/30`, `--verbose-conflicts`, `--json`/`--tree` multi-slot) | ~22 | which instance is "A"/"existing" and which parent is reported first both follow processing order |
| autounmask backward cascade / breakage | ~7 | the "already-resolved slot re-check" fires when a `[flag]` dep is hit for an already-graphed pkg — order-dependent |
| `--debug` resolver trace (`stage1_digraph_dump`, `stage3_candidate_list`, `stages_2_4_walk`) | 4 | the trace narrates the walk in walk order |
| `package.provided` plural WARNING, REQUIRED_USE "two violations collected", `\|\|` yields-to-next | 5 | order of two independent atoms / branch selection under backtracking feedback |

None is a resolution error — DFS just reorders a correct answer, and
these tests pin the order.

### Key finding: the resolution-walk order is decoupled from the merge list

`merge_order::build_digraph` **already replays `_create_graph`'s DFS
stack traversal** (LIFO `dep_stack` + deferred `_dep_disjunctive_stack`)
over the resolved `entries` + `GraphEntry::deps` edges — see the
`// Real _create_graph` comment block (~line 553). So the final merge
list order is derived from the graph *structure*, not from the order
`entries` was appended in. Switching the resolution walk to DFS does
**not** move cluster I (merge-order parity).

The DFS walk matters only for:

1. **The partial graph on an aborted resolve** — the `get_best_run`
   target. Moot today: portuale never aborts (records the failure,
   keeps walking).
2. Edge cases where the *order of encountering a conflict* changes
   backtracking feedback → a different final resolution. 8 L0 probes,
   all already-divergent, net-neutral.

## Consequence for the plan

The "byte-identical DFS walk" is a near-no-op and is **not the
foundation** — portuale already has the DFS traversal it needs
(`merge_order.rs`). The two pieces that actually matter:

- **abort-on-unrecoverable-failure**: real `_create_graph` `return 0`
  unwinds the DFS the moment a required dep can't be satisfied and
  autounmask can't fix it. Portuale's design is "report, don't enforce"
  — it would need a gated abort path.
- **`get_best_run`**: track the graph across backtrack iterations, keep
  the deepest terminal one, display its partial `altlist()`.

### A shorter path to the truncation, using what exists

Earlier analysis (see `TEST/findings/l0.md` cluster A) established that
real truncates *only* when autounmask changes coincide with an
**unresolvable circular dependency** (both cases are the `dev-lang/go`
self-cycle), and that real's partial set == the DFS-discovery prefix up
to the first `want_restart_for_use_change` autounmask node. Since
`merge_order` already computes that DFS order:

1. run the full BFS as today;
2. detect `!circular_deps.is_empty() && <autounmask changes> &&
   !autounmask_backtrack_enabled`;
3. record, during the walk, which `(cat,pkg)` had an autounmask USE flip
   that changed its own `use_reduce`'d dep set (`want_restart`);
4. in `merge_order`'s DFS `g.order`, find the first such node; truncate
   `entries` (and the merge list) to the prefix.

This matched `plasma-meta` (4) and `podman` (8) **exactly** in the
paper analysis. It needs no walk rewrite — just (3) + a post-pass. The
risk earlier flagged ("approximation defeats the comparator's size-gap
suppression") does not apply if the prefix is exact.

**Open question:** is `want_restart_for_use_change`'s "the flip changed
the dep set" the right cut point, or does real also stop at a flip that
only breaks a *parent's* `[use]` dep? (real `want_restart` checks both.)

## Verdict on Step 1

**The flag-gated DFS walk is a dead end for the stated goal.** It is a
lateral move: same answers, reshuffled representation, at the cost of 38
order-pinned contract tests and zero merge-order-parity gain. The
`PORTUALE_DFS_WALK` toggle stays in the tree as a research instrument
(default off, no cost) but should not be promoted.

**Recommended next step:** abandon the walk rewrite. Implement the
targeted truncation post-pass (steps 1–4 above) on `main` as a normal
slice — it is bounded, needs no DFS walk, and hits `plasma-meta`/`podman`
exactly. If a genuine `get_best_run` is ever wanted it is a separate,
much larger effort whose prerequisite (a real digraph object + abort
semantics) is independent of queue-drain order.
