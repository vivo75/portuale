# crates/portage-repo/src/merge_order/priority.rs
Dependency priorities, the edges they label and the ignore rungs.

## struct DepPriority
- Ports Portage's `_emerge/DepPriority.py::DepPriority` -- the per-edge dependency classification that drives every `ignore_priority` decision in `_serialize_tasks`.
- `cross` (a dependency crossing into a different `ROOT`) is always `false` here: portuale resolves one root at a time, so Portage's `self._cross(pkg.root)` is constantly false and the two `runtime_slot_op and not priority.cross` guards below collapse to plain `runtime_slot_op`. `ignored` is likewise not modelled -- Portage only ever sets it for `--root-deps=rdeps` / `--with-bdeps=n` on a built package, both of which portuale expresses by dropping the `DEPEND`/`BDEPEND` keys from the walk entirely (see `dep_edges_from_metadata`'s callers), which is what Portage's own `edepend["DEPEND"] = ""` does one line later.
- `satisfied`: Portage's `mypriority.satisfied = inst_pkg` -- an already-installed package matches this atom, so the edge can be relaxed to break a cycle (`DepPrioritySatisfiedRange`). Filled in by `build_digraph`, which is the first point at which every entry's resolved slot is known.

## struct DepEdge
- One resolved dependency of a `GraphEntry`, in Portage's `_add_pkg_dep_string`'s own `deps`-tuple key order (`RDEPEND`, `IDEPEND`, `PDEPEND`, `DEPEND`, `BDEPEND` -- `depgraph.py:4253-4289`).
- The atom string is kept alongside the resolved `category`/`package` so `build_digraph` can decide `DepPriority::satisfied` against the installed vdb once the whole graph is known.
- `evaluated`: the token with its use-deps evaluated against the parent's USE (`portage_dep::evaluate_atom_conditionals`), for Portage's `--debug` `Candidates:` line (#138; oracle `logs/l111-s0-20260921/real-rest-debug.log`). Note the token itself is already reduced-raw, not true-raw: `dep_edges_from_metadata` runs `use_reduce_structured` first, which evaluates `flag?()` conditionals away -- so Portage's first, true-raw `Depstring:` stanza (`Depstring: plasma? ( ... )`) has no portuale counterpart (known remainder, unnumbered display gap). Falls back to the token when evaluation returns `None`.
- `disjunctive`: Ports Portage's `depgraph._queue_disjunctive_deps`: this atom sits inside a `|| ( … )` group, or is itself a `virtual/*` atom (which Portage's `dep_check` expands into one). Portage does not walk these inline -- it collects each dep key's whole disjunctive set into one bundle on `_dep_disjunctive_stack`, which `_create_graph` only pops once the ordinary `_dep_stack` has drained completely. That deferral is load-bearing for `.order`, and therefore for merge order: it is why a `virtual/*` dependency lands hundreds of positions later in Portage's graph than the sibling atoms declared next to it.
- `alt`: for a `disjunctive` atom that came from an actual `|| ( … )` group: `(group, branch)` -- `group` counts the `||` groups in this dep key, `branch` the alternative within one group. `build_digraph` keeps one branch per group (`dep_zapdeps`' `choice_bins`: all-in-graph, then all-installed, then first) and suppresses the rest. `None` for an inline atom or a bare deferred `virtual/*`.
- `key`: which of Portage's own `deps`-tuple keys this came from (0=`RDEPEND`, 1=`IDEPEND`, 2=`PDEPEND`, 3=`DEPEND`, 4=`BDEPEND`). Disjunctive bundles are queued one per key, so the walk needs the key boundaries.

## fn split_disjunctive
- Ports Portage's `depgraph._queue_disjunctive_deps`, run over one USE-reduced dep key: returns `(inline, disjunctive)` atom lists.
- Portage walks the `opconvert=True` reduced struct and, for each element: a `|| ( … )` group goes wholesale into the deferred bundle; a plain all-of group recurses (contributing to that same bundle); a bare atom is deferred too when its category is `virtual` ("Eventually this will check for PROPERTIES=virtual"), and yielded inline otherwise.
- Portuale keeps every branch of a `||` group rather than resolving one up front; `build_digraph` then picks `dep_zapdeps`' first-satisfiable branch (see its own doc). Each disjunctive atom is tagged with the `(group, branch)` of the `|| ( … )` alternative it belongs to -- `group` counts the distinct `||` groups reached in this key, `branch` the alternative within one group (a bare atom is its own single-atom branch; a nested `( … )` is a multi-atom branch). A `virtual/*` atom outside any `||` is deferred too (like Portage) with `group`/`branch` `u32::MAX` so `build_digraph` never groups it with a real alternative.
- Depth of the innermost enclosing `|| ( … )`, if any: everything below one is disjunctive, however deeply nested.
- Depth at which the current `branch`'s nested `( … )` opened, if any; the branch stays fixed for every atom until that group's `)`.
- A nested `( … )` alternative directly inside the `||`.
- A bare atom directly in the `||` is its own branch.

## fn key_priority
- The dependency keys Portage walks, paired with the `DepPriority` each contributes -- Portage's `depgraph.py:4253-4289`'s own `deps` tuple. `optional` is `pkg.built` for the two build-time keys (a built package's build deps are informational only); `IDEPEND` is a `runtime` priority like `RDEPEND` (Portage also sets `installtime`, which no `ignore_priority` predicate reads).

## fn dep_edges_from_metadata
- `GraphEntry::deps`: flattens `metadata`'s own dep-key strings, one key at a time in `keys`' given order (so cross-key token order can't leak in), against `use_flags`. `use_reduce_flat` transparently drops `||`/`(`/`)` markers, listing every branch's atoms in written order -- the other branches simply never correspond to a real entry, so the digraph's own lookup skips them for free.
- One `DepEdge` per distinct `(atom, category, package, priority)`: Portage records a list of priorities per digraph edge (`digraph.add`'s `bisect.insort`), and `leaf_nodes`/`child_nodes` need every one of them, so an atom named by both `RDEPEND` and `DEPEND` contributes two edges here rather than being deduped to the first. The `atom` is part of the dedup key (not just `cat/pkg`) so the distinct branches of a `|| ( >=foo-2:2 >=foo-1:1 )` group both survive -- `build_digraph` matches each atom against the resolved entries individually, so the branch that doesn't resolve is dropped there, not here.
- `built` is Portage's `pkg.built` -- true for a binary candidate and for an installed package -- and only affects whether the build-time keys are marked `optional`. Callers that drop `DEPEND`/`BDEPEND` from `keys` entirely (Portage's `edepend["DEPEND"] = ""` for a built package under `--with-bdeps=n`) never reach that branch.
- The structured reduction keeps the `||`/`( … )` boundaries `split_disjunctive` needs; Portage reduces the same string with `opconvert=True` before handing it to `_queue_disjunctive_deps`.
- Ports Portage's `_wrapped_add_pkg_dep_string`: a `:=`/`:slot=` atom promotes its own key's priority to the slot-operator variant, which the `ignore_priority` ladder refuses to relax as readily as a plain one.

## enum Ignore
- One `ignore_priority` filter: a rung of `NORMAL` (`N*`) or `SATISFIED` (`S*`). An enum, not a `fn` pointer, so a filter has one identity whatever the codegen (function addresses are not unique across codegen units).

## section: ignore predicates
- `_emerge/DepPriorityNormalRange.py` and `_emerge/DepPrioritySatisfiedRange.py`, one function per rung.

## fn ignore_name
- Portage's `ignore_priority.__name__` for the `_serialize_tasks` trace (`PORTUALE_MO_SEL`), so a portuale `MO_SEL` line lines up field-for-field with the `RT_SEL` line `TEST/scripts/mo-trace/real-trace.py` injects into Portage's loop. `None` is Portage's `ignore_priority = None`.

## struct PriorityRange / static NORMAL / static SATISFIED
- Ports Portage's two `DepPriority*Range` classes: an `ignore_priority` ladder indexed `NONE=0 .. MEDIUM`, plus the named rungs `_serialize_tasks` reaches for directly.

## impl Display for DepPriority
- Ports Portage's `_emerge/DepPriority.py::DepPriority.__str__` -- the single highest classification. `Priority:` lines and `digraph:` edge labels both use it.
