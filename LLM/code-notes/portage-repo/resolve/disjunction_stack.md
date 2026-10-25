# crates/portage-repo/src/resolve/disjunction_stack.rs
Deferred `||` resolution for the default solver (02.359 S5, backlog #369).

## fn split_off_disjunctions
- Portage: `_emerge/depgraph.py`, `_queue_disjunctive_deps` (the walk over the
  `use_reduce`d struct: `||` lists appended to `disjunctions`, everything else
  yielded) and `_add_pkg_deps` (the `use_reduce` call just above it, which is
  why conditionals evaluate at split time here, not at pop time).
- The owner's effective USE is frozen at expansion: Portage evaluates
  `use_reduce` at `_add_pkg_deps` time and defers only the *choice*. A later
  in-pass autounmask flip re-runs the whole walk (driver), so the frozen USE
  never goes stale within a pass.
- Nested `||` inside a deferred alternative stays inside the stored group and
  resolves at pop time through the same `use_reduce_flat_disjunctive` call,
  i.e. immediately rather than re-deferring the way Portage's
  `_add_pkg_dep_string` re-queue would. No fixture or bed cell shapes this;
  noted, not handled.
- `virtual/*` atoms are NOT deferred (Portage appends them to the same stack
  entry): S0 (`pg359-vapp -pv`, byte-identical 4 rows) shows no divergence,
  so #397 stays unfiled per the plan condition.

## struct DeferredDisjunction
- Portage: `_queue_disjunction` (one `(pkg, dep_root, dep_priority,
  dep_struct)` entry per dep string, the whole group list resolved together)
  and `_pop_disjunction` (LIFO `.pop()`, resolved via `_add_pkg_dep_string`
  against the drained graph).
- Group order inside an entry is declaration order; entries pop newest first.
- `depth` is the entry's own depth on the main site (`enqueue_flat_deps`
  queues at `depth + 1`) and the children's depth outright on the installed
  site (`enqueue_plain_tokens` takes `child_depth`).
- The `--autounmask-use` flip path pushes a `Flip` site since #398 (was
  deliberately not a site, residue #398): see below.

## fn pop_disjunction
- Portage: `_create_graph` (`while dep_stack or dep_disjunctive_stack` with
  the inner drain first). The `run_pass` loop calls this only when
  `pop_queue_item` reports both `queue` and `held_blocks` empty — the
  portuale equivalent of "the whole `dep_stack` is empty". Held seed blocks
  restore first (Portage never pops a disjunction while plain seeds are still
  stacked).
- Group-free expansions push nothing, and their plains flatten through the
  unchanged closures (never invoked on `||`-free input), so the walk without
  `||` is exactly as found. `or_group_universe` and the reverse-order tricks
  in `enqueue_flat_deps` are kept by owner decision, not removed.
- Multi-`PackageArg` runs differ from Portage in one ordering corner:
  Portage drains both stacks per `PackageArg` (`_resolve`), portuale drains
  all seed blocks before the first disjunction pops. Row sets agree; order
  could differ. No cell shapes this today.

## enum DeferredSite::Flip (backlog #398)
- Portage: `_queue_disjunctive_deps` runs for every `_add_pkg_deps` expansion
  including a USE-flipped re-expansion — a restart pass expands the flipped
  package with the flip in force, so its `||` groups defer like any other.
  portuale re-expands in-walk instead, and chose the branch at flip time
  (`pg398-t`: `X` picked with neither alternative visible; Portage merges
  only `Y`). The flip now splits plains (queued in-walk, unchanged) from
  groups (pushed here) via `split_off_disjunctions` on the flipped USE.
- Pop-time resolution reuses the shared target-scope core with the backtrack
  loop's own circular map (no virtual join: a flipped owner is never given
  the Main site's pullers join, like the old expansion-time flatten).
- Pop-time enqueue mirrors the flipped pipeline: genuinely-new filter against
  pop-time visited/queued, `delta_atom_cleanly_reuses_resolved_slot`, pullers
  into `slot_pullers` plus the parent edge into `required_by_map` (Portage
  adds the parent atom even for a previously added package), `enqueue_flat_deps`
  for the kept atoms only, and merge-order edges for kept plus already-seen
  targets (Portage's drained-graph choice still orders the merge — without the
  edge `pg398` sorts `a` before `Y`). The slot-key lookup with a version check
  finds the edge target; indices are not stored (`entries.remove`/`retain`
  shift them mid-pass).
- Malformed input falls back to today's path: the splitter returns its input
  unsplit, so there are no groups and everything flattens in-walk as before.

## gate threading (#571)
- `DisjunctionPreferenceArgs` carries the gate into `alternative_downgrade_demoted` / `downgrade_probe` / `visible_tree_matches`.
