# 025b — `_complete_graph` installed nomerge nodes: design note (#25, R3a)

Status: design, 2026-09-14, against `backlog/tier_2_e_5`. This is the
R3a slice of
[`backlog_tier_5_and_2_sliced.opus.md`](backlog_tier_5_and_2_sliced.opus.md)
§6; R3b–R3e implement it. Grounding: the F-B3/F-B4 findings in
[`025-tier2-closeout.deepseek.md`](025-tier2-closeout.deepseek.md) §11,
the mo-trace harness (`TEST/scripts/mo-trace/README.md`), and the real
sources named below. Owner decision **D4** applies: individual R3
slices may move L0 merge-order rows in either direction, provided the
net after R3e is not worse and every flipped probe is logged.

## 1. What #25 owns

Three visibly different divergences with one root cause — portuale's
scheduler graph is built *after* resolution from a different shape of
data than real's:

- **F-B3** (025 §11, `TEST/findings/l0.md` "## I"): the `-pe @system`
  tie-break. Node sets match (368 == 368) and the bias keys match, but
  the **pre-bias order** diverges at index 1 (`baselayout, findutils,
  patch, …` vs `baselayout, awk, bzip2, …`); `_merge_order_bias` is a
  stable sort, so real's `_create_graph` insertion order survives into
  the merge list, while portuale's DFS order does not. Downstream L0
  rows: `_system` #11, `_world` #14, `MULTI_emptytree-system` #13.
- **F-B4** (same section): a dependency initially satisfied by an
  *installed* package leaves **no edge** in real's graph
  (`nghttp2 -> systemd` absent; gedit #5, nautilus #8), and when a
  same-slot merge supersedes an installed node, real drops that node's
  in-edges. Portuale keeps an edge and waits for the merge. A static
  `build_digraph` rule cannot fix it: `MULTI_deep-update-world` (the B2
  order guard) needs the *opposite* edge for the same shape.
- **The original item**: real keeps every `@world`/`@system`-reachable
  installed package as a graph node (`_complete_graph`); portuale
  approximates them with reverse-dep atoms
  (`merge_order.rs::add_installed_dependency_closure`, a post-walk vdb
  closure with synthetic entries).

The information that separates the F-B4 cases is **the resolver's walk
order**, i.e. *which atom first forced the merge*. It cannot be
recomputed from the final candidate set. Hence: record it during
resolution and hand it to the scheduler graph.

## 2. Real's model

### 2.1 Walk order = LIFO stack, and that order is kept

`depgraph._create_graph` (`3rdparty/portage/lib/_emerge/depgraph.py:3254-3268`)
pops a **LIFO** stack:

```python
dep_stack = self._dynamic_config._dep_stack
while dep_stack or dep_disjunctive_stack:
    while dep_stack:
        dep = dep_stack.pop()
        if isinstance(dep, Package):
            self._add_pkg_deps(dep, ...)
            continue
        self._add_dep(dep, ...)
    if dep_disjunctive_stack:
        self._pop_disjunction(...)
```

Top-level args are pushed first; `_add_dep` (`:3349`) pushes the chosen
package as a `Package` entry; `_add_pkg` (`:3550`) walks that package's
own deps. Every node therefore has a well-defined insertion instant, and
`_add_pkg`'s `dep_stack.append`/`extend` order is the walk order. `||`
groups are deferred wholesale to `_dep_disjunctive_stack` and popped only
after the ordinary stack drains (`DepEdge::disjunctive` already mirrors
that deferral on portuale's side).

`_complete_graph` (`:8562`) later performs the mode-dependent deep walk
over the required sets (`@world`/`@system`/args); the nodes it adds are
inserted by the *same* stack discipline, so a single monotone sequence
covers both phases.

### 2.2 `DepPriority.satisfied`

`_emerge/DepPriority.py:10` carries `satisfied` alongside
`optional`/`buildtime`/`runtime`; `_emerge/DepPrioritySatisfiedRange.py`
turns it into rung membership: an unsatisfied runtime/buildtime edge is
HARD (`MEDIUM`, index 7), while **satisfied** runtime/buildtime edges are
MEDIUM_SOFT (index 6) and `runtime_post`/optional are softer still. The
scheduler's `drop_satisfied` ladders ignore the soft classes first, so an
edge whose atom matched an installed package at *add time* does not pin a
merge the way a not-yet-built dependency does.

Two properties matter for the port:

- `satisfied` is evaluated **when the edge is added** (`depgraph.
  _add_dep`'s `mypriority.satisfied` is set from the then-current
  installed set and the graph), not re-evaluated at serialization time.
  A later merge that replaces the installed instance does **not** turn an
  already-soft edge hard.
- `.satisfied` is only meaningful for the `DepPriority` class; the
  rebuilt-dependency classes (`DepPrioritySatisfiedRange` consumers) keep
  their own semantics.

### 2.3 Installed nodes and superseded in-edges

Installed packages are first-class `Package` nodes: an `_add_dep` that
resolves to an installed instance pushes it like any other, so it gets a
`DepPriority` per edge, an insertion position, and children from its own
recorded metadata (`pkg.built` → build deps `optional`, matching
portuale's `dep_edges_from_metadata(..., built = true)`). When a
same-slot merge is added later, real's collision path
(`_add_pkg` → `_remove_pkg`, `:4058`, and the same-slot handling around
`:2065-2080`) **removes the superseded installed node** (or its in-edges)
from the graph, so nothing downstream waits on a node that no longer
exists in the final plan. That is the F-B4 `nghttp2 -> systemd` drop; the
distinguishing datum is whether the merge was already in the graph when
the edge was added (then the edge points at the merge) or not (then the
edge was satisfied-by-installed and stays soft/absent).

## 3. Portuale today

- Resolver: `resolve_pretend_graph` (`rust/portage-repo/src/lib.rs`)
  produces `GraphEntry` (`:11668`) with `outcome`, `slot`/`sub_slot`,
  `deps: Vec<DepEdge>` and `required_by`. `DepEdge` (`merge_order.rs:88`)
  carries `priority`, `disjunctive`, `alt`, `key`.
- Scheduler graph: `merge_order::build_digraph` (`:1316`) DFS-walks the
  expanded top-level atoms, derives `children`/`parents`, and recomputes
  `satisfied` **from the current vdb** (`installed_candidates_by_cp`)
  at serialization time. `add_installed_dependency_closure`
  (`:1044`) post-walk fills installed `deps` and synthesises
  entries for installed dependencies to a fixpoint; `serialize_merge_order`
  then prunes the nomerge roots real prunes.
- Missing, relative to §2: no per-node insertion sequence; no
  recorded-at-add `satisfied` bit (only a live vdb probe); installed
  nodes are synthetic post-walk entries rather than walk-ordered nodes.

## 4. The data contract (what R3b–R3d add)

### 4.1 Insertion sequence (R3b)

Every entry the resolver creates gets a monotone `insertion: u64` (or the
entries vec *is* the walk order with a separate `top_level`/disjunction
marker). `build_digraph` must use it as the pre-bias node order instead
of the DFS order, and `Digraph.order` becomes the resolver's sequence.
Acceptance is the F-B3 probe: `MO_ORDER` on `-pe @system` equals real's
(`368 == 368`, same sequence); log every L0 probe that flips (D4).

### 4.2 Satisfied-at-add flag (R3c)

`DepEdge` gains `satisfied: bool`, recorded in `enqueue_dependencies`
where the edge is built, against the *installed set of that instant*
(the resolver already has `best_installed_for_atom`; the same call
answers it). `build_digraph` uses the recorded bit instead of the live
`installed_candidates_by_cp` probe, and the edge priority for a
satisfied edge drops to the MEDIUM_SOFT rung. When the same slot is
superseded by a merge in the same graph, the recorded bit stays soft
**and** the superseded installed node's in-edges are dropped (R3d);
`MULTI_deep-update-world` must keep its B2 portage/gentoolkit order,
which is the guard that this is not "just drop all installed edges".

### 4.3 Installed nodes first-class (R3d)

Installed packages reached by the walk become ordinary entries in the
resolver's own sequence (not a post-walk vdb closure), with:

- their own `deps` from the recorded vdb metadata (existing
  `dep_edges_from_metadata(..., built = true)` path),
- an insertion position from the walk,
- a supersede rule: a same-slot merge anywhere in the final graph
  removes that installed node's in-edges (and, if nothing reaches it,
  the node) the way `_remove_pkg` does.

`add_installed_dependency_closure` then either shrinks to a seeding
helper for the complete-mode required sets, or retires once the probes
agree (R3d's acceptance). Node/edge sets must match real's `--debug`
digraph dump on gtk:4, gedit and nautilus; `MO_NODES` equality on all
four probes (including `-pe @system`).

## 5. Acceptance probes (mo-trace)

The harness is `TEST/scripts/mo-trace/` (`MO_SEL` per-iteration state,
`MO_ORDER` post-prune/pre-bias order, `MO_NODES` node sets; real side via
`real-trace.py` inside the throwaway container; Rust-only by design).

| probe | command | gate |
|---|---|---|
| `-pe @system` order | real: container `emerge --pretend -e @system --debug`; portuale: `PORTUALE_MO_ORDER=1 … emerge -pe @system` | R3b: same sequence (368 == 368) |
| gedit | `emerge --pretend --update --deep --verbose-slot-rebuilds=y app-editors/gedit` (F-B4: `nghttp2` has only its `virtual/pkgconfig` edge) | R3c |
| nautilus | same shape, `#8` | R3c/R3d |
| gtk:4 | `TEST/scripts/mo-trace/ptl-trace.sh /tmp/gtk4 -- --pretend app-misc/gtk:4` + the container real counterpart | R3b/R3d |
| `MULTI_deep-update-world` | the L0 `MULTI_` probe in `TEST/atomlists/l0-resolve.txt` (B2 guard) | R3c: portage/gentoolkit order kept |

Operational detail and the exact `RT_SEL` patch flow: the harness README.
L0 re-runs: `TEST/run/l0-resolver.sh` before/after each slice; any probe
that flips is logged in the R3 commit body (D4), and the net after R3e
must not be worse than the pre-R3 baseline (`TEST/logs/l0-20260914T122346Z`:
clean 98, parity 0.817, unexplained 38, order 19).

## 6. Slice plan and risks

- **R3b** insertion sequence → `build_digraph` pre-bias order. No edge
  changes. Risk: the DFS order is load-bearing in the installed-closure
  seeding; R3b must keep the closure's synthetic entries ordered
  consistently until R3d.
- **R3c** satisfied-at-add. Risk: prematurely soft edges change merge
  order broadly; the `MULTI_deep-update-world` guard is the acceptance,
  and D4 covers L0 movement.
- **R3d** installed first-class + supersede. Risk: the closure's
  complete-mode seeding (`@system` ballast, `_ignore_optional` pacing)
  is what keeps `libgit2`/perl-cycle probes honest — R3d must carry that
  pacing into the walk-ordered model, not delete it.
- **R3e** close-out: full L0, `TEST/findings/l0.md` "## I", 025 §11
  F-B3/F-B4 → resolved or residue explained, `backlog-tasks.md`.

Documented uncertainties to re-check at R3b: whether `insertion` needs a
separate field or can be the entries index; whether the supersede rule
needs real's full `_remove_pkg` recursion or only in-edge removal for
the probes at hand. Both are settled by the first `MO_ORDER`/`MO_NODES`
runs, not by this note.
