# #366: Portage's graph-fill (walk) order (read 2026-10-15, Portage 3.0.82.2)

Sources (read-only, `/usr/lib/python3.14/site-packages`):
`_emerge/depgraph.py` `_create_graph` (:3254), `_dep_stack` push site
(`_add_pkg`, :3820-3830), `_add_pkg_deps` (:4151), `_queue_disjunctive_deps`
/ `_queue_disjunction` / `_pop_disjunction` (:4857-4922),
`_wrapped_add_pkg_dep_string` (:4446, `_select_atoms` call at :4493),
`_select_atoms_from_graph` (:5994), and `portage/dep/dep_check.py`
`dep_zapdeps` (:339-400 bins, :637-710 ranking).

## The walk

`_create_graph` drains two LIFO stacks. `dep_stack` holds `Package` and
`Dependency` items; `dep_disjunctive_stack` holds whole `||` (and virtual)
dep-strings queued per package per dep class. The inner loop pops
`dep_stack` to empty first (`dep_stack.pop()`, LIFO): a `Package` pop runs
`_add_pkg_deps` on it at once, a `Dependency` pop runs `_add_dep`
(selects the package, adds it to the digraph via `_add_pkg`, and pushes
the new `Package` onto `dep_stack`). Only when `dep_stack` is empty is
one disjunction popped (`_pop_disjunction`) and resolved through
`_add_pkg_dep_string` → `_select_atoms` → `dep_zapdeps`, whose chosen
children re-enter via `_add_dep` and refill `dep_stack`.

Consequences:

1. Depth-first, last-pushed first-popped. A package's plain (non-`||`,
   non-virtual) children are appended to `dep_stack` in written order, so
   they are *processed* in reverse written order; each one's subtree is
   fully walked before the next sibling pops.
2. Every plain dependency in the whole reachable cone is added to the
   digraph before ANY `||` is resolved: `_add_pkg_deps` only *queues*
   `||` strings (`_queue_disjunction`, one entry per package per dep
   class) and yields the plain atoms for immediate `_add_dep`. A package
   whose own `||` is queued still sits in the graph already (`_add_pkg`
   adds the package to the digraph when its `Dependency` is processed,
   before its deps are ever expanded).
3. Disjunctions resolve LIFO across packages and classes: queued in class
   order RDEPEND, IDEPEND, PDEPEND, DEPEND, BDEPEND (`_add_pkg_deps`
   `:4246-4295`), popped from the end, so a package's BDEPEND `||`
   resolves before its RDEPEND `||` (bug 639346 comment), and a child's
   queued `||` resolves before its parent's still-queued one.
4. `dep_zapdeps` sees the digraph *at that moment* (`graph_db` +
   `graph`, wired in at `:786-793`): a branch counts as in-graph only if
   `graph_db.match_pkgs(atom)` already names a package present in the
   digraph. Ranking per choice: in-graph first (`preferred_in_graph`),
   then installed (`preferred_installed` / `preferred_any_slot`),
   then the written order (leftmost wins inside `preferred_non_installed`).
   USE-violating choices sort after every USE-satisfied bin.
5. Arguments are appended to `dep_stack` in argv order and therefore start
   in reverse argv order; each argument's subtree completes before the
   next starts. Under `--emptytree` / complete mode, the system/world sets
   are appended only after the arguments' closure is done (`_complete_graph`
   flips `_select_atoms` to `_select_atoms_from_graph`, `:8662`), so they
   can never win an argument subtree's `||`.

## Which packages are in the graph when a given package's `||` is processed

- All packages added by processing every plain dependency chain that was
  pushed before the disjunction popped: the package itself, its ancestors,
  every earlier-processed sibling subtree (in reverse written order), and
  everything those pulled in — but no package reachable only through a
  disjunction that pops later (including the group's own taken child,
  which is selected *by* this resolution), and nothing from
  still-unprocessed arguments or the complete-mode system/world pass.
- `virtual/pager -pve`: the root's `||` is the only queued disjunction
  and the plain stack holds nothing else, so it resolves with exactly
  `{virtual/pager}` in the graph; with `-e` nothing counts as installed,
  so every branch lands in `preferred_non_installed` and the leftmost
  (`sys-apps/less`) wins. `sys-apps/util-linux` enters later via systemd
  (complete-mode/system reachability), after the choice is made.
- Fixture (a) `RDEPEND="A B"`, `A: || ( X Y )`, `B: Y`: root appends A,
  B; B pops first and adds Y; then A pops and queues its `||`; the
  disjunction resolves with `{R, A, B, Y}` present, so Y (in-graph) beats
  X (leftmost). Fixture (b) `RDEPEND="B A"`: root appends B, A; A pops
  first but only queues its `||`; B then pops and adds Y; same resolution
  set `{R, A, B, Y}`, same winner Y. Both orders agree — the pair pins
  the LIFO-plus-deferral discipline, not a winner change.
- A winner change needs the alternative's other parent to be added *after*
  the group resolves (the pager/systemd shape): e.g. `R: A`, `A: || (X Y)`,
  `X: B`, `B: Y` resolves with `{R, A}` present and takes X, while a
  plan-order-blind "selected anywhere" test would take Y via the later B.
