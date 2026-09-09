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
paper analysis. It needs no walk rewrite — just (3) + a post-pass.

### …but it has a hard prerequisite it can't meet (Step 2, 2026-09-09)

Prototyped the post-pass. **It never fires**, because step 2's
`!circular_deps.is_empty()` is false for both probes:

- Real `plasma-meta`/`podman` merge only `dev-lang/go-1.26.4` and report
  the `go → go (buildtime)` self-cycle.
- Portuale merges **both** `dev-lang/go` *and* `dev-lang/go-bootstrap`,
  no cycle — its cluster-D `||` fix (`or-dep-required-use-repro`,
  `2026-09-07`) marks the circular `>=dev-lang/go` branch of go's own
  `BDEPEND=|| ( >=dev-lang/go-X dev-lang/go-bootstrap )` as unavailable
  and takes `go-bootstrap`.

Real's actual mechanism: on pass 1 `dep_zapdeps` has no
`circular_dependency` map, so it picks the in-graph `>=dev-lang/go`
(`preferred_in_graph`) → cycle detected → backtrack records
`circular_dependency = {go: [go]}` → **pass 2** `dep_zapdeps` sends that
choice to `other` and picks `go-bootstrap`. For a *standalone*
`emerge -p dev-lang/go` this backtracking completes and both real and
portuale end at `go-bootstrap` (no cycle). For `plasma-meta`,
`--autounmask-backtrack=n` stops real after the first autounmask batch,
so pass 2 never happens and the cycle stays.

So reproducing real's `plasma-meta` needs portuale to **take the
circular `||` branch on pass 1 when the cp is already a graph node**
(conditionally undo cluster-D) **and** implement `circular_dependency`-
map-driven backtracking so a later pass switches to `go-bootstrap` when
backtracking is *allowed* to proceed. That is the deferred backtracking
work, not a post-pass.

The prototype (dead condition + `merge_order::discovery_order` helper +
`autounmask_flip_cps` accumulator) was reverted from `main`.

## Verdict on Step 1

**The flag-gated DFS walk is a dead end for the stated goal.** It is a
lateral move: same answers, reshuffled representation, at the cost of 38
order-pinned contract tests and zero merge-order-parity gain. The
`PORTUALE_DFS_WALK` toggle stays in the tree as a research instrument
(default off, no cost) but should not be promoted.

**Recommended next step (revised after Step 2):** the truncation
post-pass is *also* blocked — it depends on portuale detecting the same
`go` self-cycle real does, which portuale's cluster-D `||` fix
deliberately breaks. The remaining paths all lead through
`circular_dependency`-map backtracking (real `dep_zapdeps` +
`_backtrack_depgraph`):

- **full**: `circular_dependency` map + pass-2 `||` re-selection +
  `--autounmask-backtrack=n` early stop + partial-graph display. This is
  the real feature; ~1–2 weeks; medium regression risk.
- **do nothing**: `resolve-compare.py` already collapses the two probes
  to one `truncated` finding each. Portuale's fuller `plasma-meta`
  output (a complete resolution real never reached) is arguably *more*
  useful to a user than real's stuck 4-package partial.

`get_best_run` proper remains a separate, larger effort whose real
prerequisites (a digraph object + abort-on-unrecoverable-failure) are
independent of queue-drain order.
