# `emerge --pretend --debug`: real portage's resolver trace

**Status: implemented (2026-09-07).** `emerge --pretend --debug` (and
`-pd`) now emits a resolver trace on both streams, dual-language
(`rust/portage-repo/src/resolver_trace.rs` + the mirror in
`python/emerge_pretend_reference.py`), gated on the process-global
`portage_repo::set_resolver_debug()` that `pretend.rs` sets from
`--debug && --pretend`. `--debug` still also means `PORTAGE_DEBUG=1` in
any ebuild phase (real `main.py:1235` does both).

What it emits, matching real's own stream split
(`writemsg_level(level=DEBUG)` → stdout, plain `writemsg` → stderr):

- **stdout:** `\n      Arg:`/`     Atom:` per top-level atom; a
  per-package `Child:`/`Parent Dep:` / `Parent:`/`Depstring:`/`Priority:`
  /`Candidates:` / `Virtual Parent:`/`Virtual Depstring:` / `Exiting...`
  narration; the `forced reinstall atoms:` / `slot operator
  dependencies:` / `forced rebuilds:` summaries after the merge list.
- **stderr:** the per-atom `   ebuild:` / `installed:` candidate list;
  `\ndigraph:\n\n` + `debug_print()` of the merge digraph; `runtime
  cycle digraph (N nodes):` dumps.

**Deliberate divergences from real** (the goal is duplicated
functionality/information, not a byte-identical copy — and the contract
suite pins Rust == Python on both streams, not portuale == real):

- **plain-text node labels** — `(cat/pkg-ver:slot/sub_slot::repo,
  state)` with no ANSI colour (portuale is deterministic; colour is a
  documented cut elsewhere too). Diffs against real cleanly after an
  escape-strip.
- **node set** — portuale's post-prune merge closure, with the
  top-level atoms printed as pseudo-`DependencyArg` nodes, not real's
  full `@world`/`@system` universe (the `_serialize_tasks` port proved
  the extra installed-universe nodes unnecessary for ordering).
- **narration interleaving** — the `Child:`/`Parent:` blocks are emitted
  in one pass over the final `entries` (portuale BFS-push order, roots
  before dependencies), on the successful backtracking pass only. Real
  interleaves them as a LIFO `_create_graph` walk. Same information,
  different order — which is exactly why the `digraph:` dump (emitted
  from `.order`) is the authoritative diff surface.
- **candidate list** — ebuild + installed only; a binary (`$PKGDIR` /
  binhost) candidate would need a `BinaryIndex` the resolver's per-atom
  call site doesn't hold.
- `slot operator dependencies:` lists only the `:=` edges that actually
  forced a rebuild this run (portuale's `abi_rebuilds`), not real's
  every-installed-`:=`-edge dump (real's `_slot_operator_deps`).

The rest of this file is the original design note — kept for the message
inventory and upstream source cross-reference.

---

## What portuale's `--debug` is today

`--debug` / `-d` currently means exactly one thing
(`pretend.rs`'s own option parse, `emerge_options.rs:247`): set
`PORTAGE_DEBUG=1` in the phase environment, so the embedded bash runs
`set -x` in every ebuild phase. That is a faithful port of *half* of
real `main.py:1235`, and it is only observable during a real build —
`emerge --pretend --debug` is byte-identical to `emerge --pretend`,
which the option's own doc comment already records.

The `ebuild` applet's `--debug` is the same thing
(`ebuild.rs:128`), as is `--regen`'s (`regen.rs:62`).

## What real's `--debug` additionally does

Real `main.py` also calls `initialize_logger(logging.DEBUG)`, and
`depgraph` is full of `writemsg_level(..., level=logging.DEBUG)` calls
that only fire under it. The result is a complete trace of the
dependency resolution — **the single most useful debugging artifact real
portage has**, and the one this project has already exploited once: the
2026-09-06 `_serialize_tasks` port was validated by replaying real's own
dumped digraph through a prototype of the algorithm before writing any
of it (see `what-this-proves.md`, "Merge-list order: the full
`_serialize_tasks` port"). Everything below is currently obtained by
running *real* portage next to portuale; having portuale emit the same
shapes is what makes the two directly diffable.

All of it goes to **stderr** (`portage.writemsg` defaults to
`sys.stderr`), so stdout stays the clean merge list. That matters for
the contract suite: a trace on stderr cannot perturb the stdout
comparison, but the suite *does* compare stderr too, so the Rust and
Python sides must emit it identically.

Regenerate a reference dump with:

```bash
emerge -p --debug app-portage/eix > /tmp/real-debug.log 2>&1
```

### Message inventory

In roughly the order they appear, with the upstream source of each
(line numbers are portage 3.0.82.2):

| Shape | Emitted by |
|---|---|
| `\n      Arg: <arg>\n     Atom: <atom>\n` | `depgraph.py:5521` — `_resolve`, once per top-level arg atom |
| `<type>:` right-aligned to width 10 including the colon, then ` <cpv>::<repo>` — so `   ebuild:`, `   binary:`, `installed:` | `depgraph.py:8347-8352` — `_wrapped_select_pkg_highest_available_imp`, every candidate that matched |
| `\nChild:         <pkg> USE="…" <USE_EXPAND>` + `Parent Dep:    <atom>[ (<unevaluated>)] required by <parent>` | `depgraph.py:3568-3603` — `_add_pkg` |
| `Re-used Child:` / `Replace Child:` / `Slot Conflict:`, same 15-column label | `depgraph.py:3680`, `:3705`, `:3722` |
| `\nParent:    <pkg>\nDepstring: <raw dep string>\nPriority:  <DepPriority>\n` | `depgraph.py:4298-4310` — `_add_pkg_deps`, once per dep key, **unevaluated** |
| the same three lines, then `Candidates: [<selected atoms>]` | `depgraph.py:4479-4514` — `_wrapped_add_pkg_dep_string`, after `_select_atoms` (the depstring here is the USE-**evaluated** one, so the two blocks differ) |
| `\nCandidates: <virt cpv>: [<atoms>]` | `depgraph.py:4631` — the indirect-virtual-dep pass |
| `Virtual Parent:      <pkg>` + `Virtual Depstring:   <RDEPEND>` | `portage/dep/dep_check.py:225-233` |
| `\nExiting... <pkg>\n` | `depgraph.py:4747` — end of one package's dep walk |
| `forced reinstall atoms:` | `depgraph.py:1013` |
| `slot operator dependencies:` | `depgraph.py:1166` |
| `forced rebuilds:` | `depgraph.py:1192` |
| `\ndigraph:\n\n` + `digraph.debug_print()` + `\n` | `depgraph.py:9460-9464` — the whole graph, in `.order`, immediately before `_serialize_tasks` schedules it |
| `\nruntime cycle digraph (<n> nodes):\n\n` + a dump, then `runtime cycle leaf: <pkg>\n\n` | `depgraph.py:9917-9930` |
| `enabling 'complete' depgraph mode due to uninstall task(s):` | `depgraph.py:10345-10360` |

`digraph.debug_print()` itself (`portage/util/digraph.py:349`) is:

```
<node> depends on
  <child> (<priority>)
  <child> (<priority>)
<node> (no children)
```

iterating `self.nodes`, which is insertion-ordered and therefore *is*
`.order`; the per-edge label is `priorities[-1]`, i.e. the **maximum**
priority on that edge, not the whole list.

## Why port it

1. **It is the ground truth for merge order and graph membership.** The
   only two resolver gaps left (`_complete_graph` membership, and the
   `asap_nodes` libc seeding) are both "portuale's graph differs from
   real's". Diffing two dumps in the order *node sets → edge sets →
   per-edge priorities → `.order`* localises such a gap in one pass.
   `merge_order.rs`'s own header says this, and it is how the
   `_serialize_tasks` port was landed.
2. **It is already 90% derivable from data portuale holds.** Every
   message above except the `Virtual Parent:` pair corresponds to
   something portuale already computes: `GraphEntry::deps` carries the
   per-key `DepPriority`, `list_candidates` produces the candidate list,
   `merge_order::build_digraph` produces `.order` and the typed edges.
3. **The half-measure already exists and should be folded in.**
   `PORTUALE_DEBUG_MERGE_GRAPH=1` (see `running-it.md`) prints the
   digraph in an ad-hoc `NODE`/`EDGE` shape. Stage 1 below replaces it
   with real's own format, which removes the need to hand-translate
   between the two while diffing.

## Implementation plan

### Plumbing

Follow the established env-free process-global pattern (the one
`--useoldpkg-atoms`, `--package-moves`, `--quickpkg-direct` and
`--binpkg-changed-deps` use): a `static RESOLVER_DEBUG: AtomicBool` in
`portage-repo` plus `set_resolver_debug()` / `resolver_debug()`, set from
`pretend.rs`'s existing `--debug` arm. No new CLI flag — real has one
flag that does both things, and portuale's `--debug` should keep meaning
`PORTAGE_DEBUG=1` *and* now also the trace.

Add one helper next to it — `fn trace(args: fmt::Arguments)`, writing to
stderr only when the flag is set — so no call site repeats the guard.
Mirror as a module-level `_resolver_debug` bool plus `_trace()` in
`emerge_pretend_reference.py`.

Every stage below is independently shippable and independently
contract-testable (a fixture invocation with `--debug`, asserting
stderr).

### Stage 1 — the `digraph:` dump (do this first)

Highest value per line of code, and the only stage that needs no new
data: `merge_order::debug_dump_graph` already walks exactly the right
structure. Change it to emit real's format instead of `NODE`/`EDGE`:

* node label = real `Package.__str__`:
  `(<cat>/<pkg>-<ver>:<slot>/<sub_slot>::<repo>, <state>)` where
  `<state>` is `installed` for a nomerge node and
  `<type_name> scheduled for merge` (`ebuild`/`binary`) for a merge-bound
  one. `GraphEntry` already has `slot`, `sub_slot`, `repo_name` and
  `source`.
* edge label = the **maximum** priority on the edge, using real's
  `DepPriority.__str__` ladder: `buildtime_slot_op` > `buildtime` >
  `runtime_slot_op` > `runtime` > `runtime_post` > `optional` > `soft`,
  with `ignored` winning outright. `DepPriority` already carries every
  field; add a `Display` impl and an `Ord`-by-`__int__` helper so the
  "maximum" is real's own, not an ad-hoc one.
* iterate `Digraph::order` — which, since the port, genuinely is real's
  `.order` (built by `build_digraph`'s own two-stack walk, independent of
  the resolver's BFS).

**Known, documentable difference:** real's graph also contains
`DependencyArg` nodes (`net-libs/rest`, `@world`, `@profile`,
`@selected`, `@system`) with `(soft)` edges to their members, plus the
installed universe those set args reach — 1854 nodes where portuale has
462. Portuale has no arg nodes at all (set expansion is flattened before
the resolver runs) and its node set is real's post-prune
merge-bound closure. Print the top-level atoms as pseudo-arg nodes so a
diff lines up, or state the difference at the top of the dump; do not
try to synthesise the extra 1400 installed nodes, which the
`_serialize_tasks` port proved are not needed.

### Stage 2 — the per-dep-key blocks

`Parent:` / `Depstring:` / `Priority:` at both sites, and `Candidates:`
at the second. Portuale's `enqueue_dependencies` and the main
New/Upgrade walk already iterate the same keys in the same order with
the same priorities (`merge_order::key_priority`), so this is two trace
calls per key plus the evaluated-atom list `use_reduce` already returns.
Reuse `DepPriority`'s `Display` from stage 1 for the `Priority:` line.

### Stage 3 — `Arg:` / `Atom:` and the candidate list

`Arg:`/`Atom:` goes in the top-level atom loop. The
`   ebuild:`/`   binary:`/`installed:` list is every candidate that
survived masking, which is exactly what `list_candidates` plus the
binary/installed lookups already produce — print them in real's order
(the `dbs` order: ebuild, binary, installed) with the width-10
right-aligned label.

### Stage 4 — `Child:` / `Parent Dep:` / `Exiting...`

Straightforward, but note the **ordering caveat**: portuale's resolver is
a BFS over a queue, real's `_create_graph` is a LIFO stack, so this trace
will not interleave the way real's does even when the resulting graph is
identical. That is expected and should be stated where it is emitted —
it is also why stage 1 is the more valuable one, since the `digraph:`
dump is emitted from `build_digraph`'s own `.order` walk and *does*
match.

### Stage 5 — the rebuild/slot-op summaries

`forced reinstall atoms:`, `slot operator dependencies:` and `forced
rebuilds:` are all sets portuale already computes
(`slot_operator_rebuild_entries`'s `abi_rebuilds`, `rebuild_if_entries`,
and the `--reinstall-atoms` matcher). Cheap; do it whenever one of those
areas is next touched.

### Stage 6 — `Virtual Parent:` / `Virtual Depstring:` (optional)

The only stage needing something portuale does not model: real's
`dep_check` recurses *into* a new-style virtual's own `RDEPEND` as a
distinct step, and traces that recursion. Portuale treats a `virtual/*`
package as an ordinary entry whose metadata is walked like any other, so
there is no separate recursion to trace. Either synthesise the pair when
walking an entry whose category is `virtual`, or leave it as a
documented cut — it carries no information the ordinary
`Parent:`/`Depstring:` block for that same virtual does not.

## Verifying a stage

```bash
emerge -p --debug <atom>                  > /tmp/real.log  2>&1
rust/target/release/portuale emerge -p --debug <atom> > /tmp/port.log 2>&1
```

then diff the relevant section. For the digraph, diff in this order —
each step rules out a whole class of cause before the next:

1. **node sets** (a membership divergence — e.g. the outstanding
   `media-libs/libdisplay-info` one);
2. **edge sets** (a `||`-branch or USE-evaluation divergence);
3. **per-edge priorities** (a dep-key or slot-operator classification
   divergence);
4. **`.order`** (a discovery-walk divergence — the
   `_dep_disjunctive_stack` deferral is the subtle one here).

Also add contract cases so the two implementations cannot drift: a
fixture `emerge --pretend --debug <atom>` asserting the exact stderr,
per stage.
