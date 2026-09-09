# `circular_dependency`-map backtracking + partial-graph display — design

Branch `feat/circular-dep-backtracking`. Follows on from
`docs/dfs-graph-backtracker-exploration.md` (Step 1 DFS walk = dead end,
Step 2 truncation post-pass = blocked).

## What real portage does (verified read, 2026-09-09)

### The `circular_dependency` backtrack parameter

- `_emerge/resolver/backtracking.py`: `BacktrackParameter.circular_dependency`
  is a `{Package: set(circular_child_pkg)}` map, seeded empty, copied
  forward across backtrack nodes, merged by `_feedback_config` when
  `infos["config"]["circular_dependency"]` is present.
- `depgraph.py:701` `self._dynamic_config._circular_dependency =
  backtrack_parameters.circular_dependency`; `:6070` passes it into
  `dep_check` as `mytrees["circular_dependency"]`.

### Where the map is *produced*

`depgraph.py:10262-10289`, inside `_serialize_tasks` (`altlist`), the
`not selected_nodes` dead-end branch:

```
self._dynamic_config._circular_deps_for_display = mygraph
if self._dynamic_config._allow_backtracking:
    cycles = mygraph.get_cycles(ignore_priority=...ignore_medium_soft)
    for cycle in cycles:
        for index, node in enumerate(cycle):
            if node in self._dynamic_config._circular_dependency:
                unsolved_cycle = True
            circular_child = cycle[-1] if index == 0 else cycle[index-1]
            circular_dependency.setdefault(node, set()).add(circular_child)
if unsolved_cycle or not allow_backtracking:
    self._dynamic_config._skip_restart = True
else:
    self._dynamic_config._need_restart = True
raise self._unknown_internal_error()
```

So: cycle detected → record every `(node → its predecessor in the
cycle)` edge → if any cycle node was *already* in the incoming map the
cycle is "unsolved" (`_skip_restart`, no more retries); otherwise
`_need_restart` and backtrack.

### Where the map is *consumed*

`portage/dep/dep_check.py:673-689`, inside `dep_zapdeps` per `||` group:

```
if circular_atom is None and circular_dependency is not None:
    for circular_child in chain(circular_dependency.get(parent, []),
                                circular_dependency.get(virt_parent, [])):
        for atom in atoms:
            if atom.blocker: continue
            if vardb.match(atom): continue      # installed ⇒ not circular
            if atom.match(circular_child):
                circular_atom = atom; break
...
if circular_atom is not None:
    other.append(this_choice)                  # demote below every real bin
else:
    ... preferred_in_graph / preferred_installed / ...
```

i.e. once `{go: {go}}` is in the map, `go`'s own
`BDEPEND=|| ( >=dev-lang/go-X dev-lang/go-bootstrap )` sees its first
branch's `>=dev-lang/go-X` matching `circular_child=go` → that choice
goes to `other` → the `go-bootstrap` branch wins.

### The `_backtrack_depgraph` loop (`depgraph.py:12175`)

```
while backtracker:
    backtrack_parameters = backtracker.get()
    mydepgraph = depgraph(..., backtrack_parameters=backtrack_parameters)
    success, favorites = mydepgraph.select_files(myfiles)
    if success or need_config_change() or not allow_backtracking or backtracked >= max_retries:
        break
    elif need_restart():
        backtracked += 1; backtracker.feedback(get_backtrack_infos())
    elif backtracker:
        backtracked += 1

if backtracked and not success and not need_display_problems():
    mydepgraph = depgraph(..., allow_backtracking=False,
                          backtrack_parameters=backtracker.get_best_run())
    success, favorites = mydepgraph.select_files(myfiles)
```

`need_config_change()` (`depgraph.py:11708`) returns True — **breaking
the loop** — when `_allow_backtracking and --autounmask-backtrack != y
and _have_autounmask_changes()`, and sets `_autounmask_backtrack_disabled
= True` (which drives the "terminated early" notice, already ported).

## The plasma-meta / podman trace

1. **Pass 1**, empty params. `_create_graph` DFS. An autounmask USE flip
   is recorded for an in-graph node (`kimageformats[avif]` &c). If
   `want_restart_for_use_change` is True for that flip (the flip changes
   the pkg's own `use_reduce`d dep set, or breaks a parent `[use]` dep),
   `_need_restart = True` and `_create_graph` **returns 0 at that point**
   — the DFS is abandoned mid-walk. `_resolve:5676`:
   `if not self._create_graph(): self._apply_parent_use_changes(); return 0`.
2. `_backtrack_depgraph`: `success` False; `need_config_change()` True
   (autounmask + backtrack≠y) → **break** with `backtracked == 0`.
3. `backtracked` is 0 ⇒ the `get_best_run()` re-run is skipped. The
   **pass-1 depgraph is displayed as-is** — its digraph holds only the
   nodes `_create_graph` added before it bailed = the DFS-discovery
   prefix up to the restart node.
4. `display_problems()` → autounmask block + "terminated early" notice +
   the partial merge list (`plasma-meta` 4, `podman` 8), exit 1.

**The circular `go` dep is not what truncates.** It is why real cannot
*complete* on a later pass (with `--autounmask-backtrack=y` the cycle
would need the `circular_dependency` map to break the `||`), but with the
default `backtrack=n` the loop never gets there. The truncation is purely
"`_create_graph` returns 0 the instant a `want_restart` autounmask flip
lands, and the partial digraph is displayed because backtracking is
off".

The earlier finding (`TEST/findings/l0.md` cluster A) tied the truncation
to `get_best_run` + the cycle; re-reading `_backtrack_depgraph` shows the
`backtracked == 0` break happens first, so `get_best_run` is not on this
path. `get_best_run` matters only when `backtracked > 0` **and**
`need_display_problems()` is False — a pure slot/mask backtrack that
exhausted retries, not the autounmask-disabled case.

## What portuale would need

Portuale's `backtracking_resolve` (`rust/portage-repo/src/lib.rs`)
rebuilds `entries` from scratch every `'backtrack` iteration; there is no
gated mid-walk abort and no persisted partial graph. To reproduce real:

1. **Gated DFS-abort in the walk.** When a `want_restart` autounmask flip
   is folded into `autounmask_use_config` *and*
   `!autounmask_backtrack_enabled`, stop draining the queue and return
   the `entries` accumulated so far instead of finishing the BFS. Needs:
   - a `want_restart` predicate = "this flip changed the flipped pkg's
     own `use_reduce_flat` dep set, or broke a parent `[use]` dep" —
     portuale already computes both sides at the flip sites (~13424,
     ~14085).
   - the walk order at abort time must equal real's DFS prefix, not
     portuale's BFS prefix. `merge_order::build_digraph` already replays
     `_create_graph`'s LIFO DFS over the resolved edges — but at abort
     time the graph is *incomplete*, so the edge set differs. Either
     switch the resolution walk to DFS (`PORTUALE_DFS_WALK`, 38
     order-pinned contract failures — exploration Step 1) or accept an
     approximate prefix (finding cluster A: "worse than the status quo").

2. **`circular_dependency`-map for standalone `||` re-selection.** Only
   needed for `--autounmask-backtrack=y` (rare) — portuale's cluster-D
   `circular_self` check (`lib.rs:14803`) already sends `go`'s circular
   `||` branch to `go-bootstrap` on pass 1 unconditionally, which is
   where real lands *after* one backtrack. For the default `backtrack=n`
   path this map is never consulted.

## Recommendation

The truncation's real trigger is simpler than the cluster-A finding
assumed (no `get_best_run`, no cycle gate), but faithfully reproducing
the *prefix* still needs either the DFS resolution walk (38 order-pinned
contract regressions, exploration Step 1 verdict: dead end) or an
approximate prefix (cluster-A verdict: worse than status quo — it
defeats `resolve-compare.py`'s size-gap suppression and fans back out to
per-package `missing`/`extra`).

`resolve-compare.py` already collapses both probes to a single benign
`truncated` finding. Portuale's fuller `plasma-meta` output is a
*complete* resolution real never reaches. Net: **not worth the
regression risk** for 2 already-handled L0 probes. Park the branch with
this doc; revisit only if the DFS walk is promoted for another reason.
