# crates/portage-repo/src/resolver_trace.rs
`emerge --pretend --debug` resolver trace.

## module
- Portage's `initialize_logger(logging.DEBUG)` resolution trace. Portage `_emerge/depgraph.py` is full of `writemsg_level(..., level=logging.DEBUG)` / plain `portage.writemsg(...)` calls that only fire under `--debug`. Together they are the single most useful debugging artifact Portage has, and the one this project has already exploited once (the 2026-09-06 `_serialize_tasks` port was validated by replaying Portage's dumped `digraph:`).
- Stream split, matching Portage exactly (Portage `writemsg_level`, `portage/util/__init__.py:119`: `level >= logging.WARNING` -> stderr, else stdout): `tr!` -> stdout (the DEBUG-level per-package narration: `Arg:`/`Atom:`, `Parent:`/`Depstring:`/`Priority:`/`Candidates:`, `Child:`/`Parent Dep:`/`Exiting...`, the rebuild summaries); `tr_err!` -> stderr (the `\ndigraph:\n\n` + `debug_print()` dump, the `runtime cycle digraph` dumps, and the per-atom `ebuild:`/`binary:`/`installed:` candidate list).
- Deliberate divergences from Portage (see the design note): plain-text node labels (no ANSI colour -- portuale is deterministic); portuale's post-prune merge closure as the node set, with top-level atoms printed as pseudo-arg nodes, rather than Portage's full `@world`/`@system` universe.

## fn out
- Portage `writemsg_level(..., level=logging.DEBUG)` (`level < WARNING` -> stdout).

## fn err
- Portage plain `portage.writemsg(...)`.

## fn dep_priority_str
- Portage `_emerge/DepPriority.py::DepPriority.__str__`. Used for a `digraph:` edge label (`priorities[-1]`) and every `Priority:` line.
- Portage has no `ignored` field modelled here (see `DepPriority`'s doc comment); `optional` is the widest portuale expresses.

## fn dep_priority_rank
- Portage `DepPriority.__int__` -- the hardness measure `digraph.add`'s `bisect.insort` orders a per-edge priority list by, so the label is the `max`. `buildtime_slot_op` 0, `buildtime` -1, `runtime_slot_op` -2, `runtime` -3, `runtime_post` -4, `optional` -5, none -6.

## fn max_priority
- Portage `priorities[-1]` after `bisect.insort`. `prios` is never empty at a Portage digraph edge.

## fn node_label
- Portage `_emerge/Package.py::__str__` (`_emerge/Package.py:568`), minus the ANSI colour Portage wraps the cpv in.
- Portage `build_id_str` -- only a merge-bound multi-instance binary carries one here (portuale doesn't track an installed package's vdb `BUILD_ID` on `GraphEntry`; that only shows for the rare `binpkg-multi-instance` vdb, a documented cut).

## fn is_nomerge
- Portage `pkg.operation == "nomerge"`, the same test `merge_order::build_digraph` makes for `Digraph::installed`.
- #72 B3: a `Uninstall` removal reports `true` as well. Portage's uninstall node has its own `operation`, but every *merge-list* consumer of this helper treats "not a merge" the same way (`is_nomerge` gates the merge-order trace's merge set and the `_serialize_tasks` port's selection), and portuale's removal must never be selected as a merge.

## fn use_str
- `use_flags_display` (bare-name-sorted `(flag, enabled)`, same on both sides) rendered the way Portage `pkg_use_display` opens its string.

## fn dump_atom_candidates
- Portage `_wrapped_select_pkg_highest_available_imp`'s candidate list (`depgraph.py:8347`) -- `f"{type_name + ':':>10} {cpv}::{repo}\n"` for every candidate that matched `atom`, in Portage's `dbs` order (ebuild, then binary, then installed), to stderr.
- Divergence from Portage: portuale lists ebuild and installed candidates only. Binary (`$PKGDIR` / binhost) candidates would need a `BinaryIndex` threaded here that this call site doesn't hold.

## struct ParentAtom
- #59 S1: Portage `_add_pkg`'s own per-package parent-atom list (`_add_parent_atom`, `depgraph.py:3583-3604`). Collected during the walk, where the child instance an atom actually resolved to is known -- including an `AlreadyInstalled` instance that a later merge-bound visit of the same cp shadows in `entries` (the #57 `paired-1.0` case).
- `unevaluated` is Portage's `unevaluated_atom`.

## fn installed_use_str
- `read_vdb_iuse` / `USE`, the vdb equivalent of `use_str` in bare-name-sorted order.

## fn child_use_str
- An installed keep renders its vdb-recorded USE via `installed_use_str` (Portage's `installed_use_str`), every other row renders the entry's own `use_flags_display` via `use_str`. #134: the primary printer used `use_str` unconditionally, so every installed `Child:` row printed `USE=""` (the resolver never builds display fields for a keep).

## fn renarration_order
- Backlog #277. Portage's `--debug` trace is a live LIFO narration: every `_add_pkg` call prints its `Child:`/`Parent Dep:` pair, including re-visits of an already-graphed node (the innermost chain of each slot-conflict node on the oldslot shape). Portuale's dump is post-hoc (one stanza per final entry), so a re-visit prints nothing at its use site -- only the first-visit (outer) chain appears. This planner recovers the missing inner chains without re-running the walk.
- At most `budget` edges per dump call -- the caller passes `entries.len()`, mirroring the existing guard style. Single-root local by construction: both inputs come from one pass's walk, which never crosses a root (Track X resolves each root in its own pass).
- Host stanza: the first entry with this (category, package) -- a deterministic approximation, documented at the call site.
- Emission order detail: stable -- the per-host walk sequence is preserved. The ring reads like Portage's expansion order.

## fn sort_by_dep_position
- Portage re-narrates in expansion order, while portuale's BFS visit order is an accident of queue dynamics. `deps` carries the host entry's `(atom, evaluated)` pairs in ebuild-text order; an edge matches on either form (queue texts are evaluated, dep records keep both).

## fn dump_resolution_walk
- Portage `_add_pkg` (`Child:`/`Parent Dep:`), `_add_pkg_deps` (`Parent:`/`Depstring:`/`Priority:`/`Candidates:`), `dep_check` (`Virtual Parent:`/`Virtual Depstring:`) and the `\nExiting... <pkg>\n` end marker (`depgraph.py:3568/4298/4747`, `dep/dep_check.py:225`).
- `parent_atoms` (#59 S1) carries the walk's own per-instance `Parent Dep:` data. Each entry's rows are looked up by its resolved instance; an installed child the walk created but `entries` no longer holds (shadowed by a same-slot merge) gets its own `Child:` block after the entries loop. Portage interleaves that block at the atom's LIFO position; appending keeps portuale's documented BFS-order divergence while emitting the same content.
- Divergence from Portage (stated once as a comment line at the top of the block): Portage interleaves these as a LIFO `_create_graph` walk; portuale emits them in one pass over the final `entries` (BFS-push order -- roots before their dependencies). The information is the same; the interleaving is not.
- Backlog #277 adds the bounded inner-ring re-narration: the chains Portage's live walk re-narrates on revisit (each slot-conflict node's innermost chain). At most `entries.len()` such pairs per dump, so the cost stays bounded per abort; DAG walks schedule none (no back edges), so their traces are byte-identical to before.
- #277 detail: a withheld (never merged) entry's true identity is the ebuild the walk withheld.
- #59 S1 detail: a BTreeMap keeps the block order deterministic while each key's rows stay in walk order. `paired-1.0` under `<dev-libs/paired-2.0` is the canonical shadowed-installed-child case.
- #277 detail: the same gap covers a *withheld* ebuild child (never merged, so no entry at all -- the oldslot shape's `prov-2.0`, dropped by the direct solve while its `Parent Dep:` rows survive). There is no vdb record to read for one, so the display triple is resolved from the candidate list instead of printing the `:0/0::__unknown__` placeholder fallback.
- #138: Portage's `--debug` prints `Depstring:` raw and `Candidates:` evaluated (`logs/l111-s0-20260921/real-rest-debug.log`) -- the evaluated form rides `DepEdge` beside the (already reduced-raw) token. Portage's true-raw first stanza has no counterpart here (see `DepEdge::evaluated`'s doc).
- #277 detail: the compact `Child:`/`Parent Dep:` pair Portage's live walk prints when it re-visits an already-graphed node (each slot-conflict node's innermost chain). Only back edges were scheduled (forward edges are covered by the child's own later stanza), in walk order.
- Portage adds both instances for shadowed children (the installed one and the merge); this block reproduces the one portuale's `entries` lost.
