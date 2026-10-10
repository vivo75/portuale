# crates/portage-repo/src/merge_order/debug.rs
`--debug` merge-order dumps and node-selection traces.

## fn mo_sel_enabled
- `PORTUALE_MO_SEL=1`: emit one `MO_SEL ` line per `select_nodes` iteration to stderr -- the portuale half of the Portage-vs-portuale merge-order trace harness (`TEST/scripts/mo-trace/`). The Portage half is `real-trace.py`, which injects an `RT_SEL` line with the same fields into Portage's `_serialize_tasks`; `align-traces.py` walks the two streams in step and reports the first iteration whose state diverges. Never on by default, so a normal run is byte-identical (there is no other output change).

## fn outcome_version
- The version an entry is resolved at (the merge target for an upgrade/downgrade, the current version otherwise). `None` for `NoVisibleCandidate` and for a `Uninstall` removal (#72 B3), which is never a merge target.

## fn mo_sel_cpv
- Ports Portage's `Package`'s `cat/pkg-ver` label for the trace `pick=` field. Prefixed `m:` for a merge-bound node (Portage's `operation == "merge"`) or `n:` for a nomerge/installed one, so the aligner can see which kind of node each side removed when the frontiers differ -- the iter-1 gnuconfig/elt-patches batch vs Portage's installed-nomerge batch is exactly the difference this harness exists to localize.

## fn mo_sel_trace_line
- One `MO_SEL ` line (`PORTUALE_MO_SEL`) -- pure so the format is unit-pinned. Field order is the shared harness contract: `iter retlist alive asap prefer_asap drop_satisfied ig pick`; `asap` is the bracketed list of `asap_nodes` cpvs (a count was not enough: firefox's clang-runtime divergence is exactly one extra lingering asap node on portuale's side).

## fn debug_dump_graph
- Dumps the freshly-built digraph in Portage's own `digraph.debug_print()` format (`portage/util/digraph.py:349`), preceded by the `\ndigraph:\n\n` header -- to stderr, under `emerge --pretend --debug` (or the legacy `PORTUALE_DEBUG_MERGE_GRAPH` env var). This is how the module was validated and the fastest way to localise a merge-order divergence: dump both, diff the node sets, then the edge sets, then the priorities, then `.order` (per this module's header).
- Portage's format, per node in insertion order (`g.order` before `merge_order_bias` re-sorts it -- Portage's `debug_print` iterates `self.nodes`, not `.order`): `(cat/pkg-ver:slot/sub_slot::repo, <state>) depends on` / `(child..., <state>) (<max-priority>)` / `(leaf..., <state>) (no children)`.
- Divergences from Portage (see `resolver_trace`'s header): plain-text node labels; portuale's post-prune merge closure as the node set, with the top-level atoms appended as pseudo-`DependencyArg` nodes (`<atom> depends on` / `  <resolved-node> (soft)`) so a diff against Portage still lines up, rather than Portage's full `@world`/`@system` universe (proven unnecessary for ordering by the `_serialize_tasks` port).
- The env var predates the `--debug` wiring and went to stderr directly; kept working even without `--debug`.
- Pseudo-`DependencyArg` nodes for the top-level atoms, appended after the Portage graph (Portage interleaves them; portuale flattens set expansion before the resolver, so it only has the concrete atoms).

## fn debug_dump_cycle
- Ports Portage's `depgraph.py:9917-9930`'s `\nruntime cycle digraph (<n> nodes): \n\n` + `debug_print()` of the cycle's induced subgraph + `runtime cycle leaf: <pkg>\n\n` -- to stderr, under `--pretend --debug`, once per relaxed runtime cycle. Same node/edge formatting as `debug_dump_graph`.
- The last node in `.order` within the subgraph is the leaf Portage reports (Portage's `cycle_digraph.order[-1]`).

## fn debug_dump_graph_only
- Build the digraph and dump it -- for `topological_merge_order`'s single-package short-circuit, where `serialize_merge_order` (and thus the normal `debug_dump_graph` call) never runs.
