# S0 #643 — the consumer's `||` falls through because the downgrade guard demotes the expanded virtual arm

Date: 2026-11-02. Branch `backlog/643-nested-virtual` (portuale; pmtest untouched).
Oracle: Portage 3.0.83's own resolver (`portage/tests/resolver/ResolverPlayground.py`,
imported from this tree's `3rdparty/portage/lib`), p03 fixture of
`LLM/evidence/2026-10-28-572/probe572_p03.py`. No product code, no fixture or
pmtest changes in this task.

## 1. Oracle confirmation (Step 1)

`python3 LLM/evidence/2026-10-28-572/probe572_p03.py`:

```
SUCCESS True
MERGELIST ['dev-perl/Sub-Name-0.280.0-r1', 'dev-lang/perl-5.44.0', 'dev-perl/Try-Tiny-0.320.0']
UNSAT frozenset()
```

Holds exactly as the plan states: Sub-Name + the perl upgrade + Try-Tiny, the
installed virtual nomerge.

## 2. Trace (Step 2)

`probe643_trace.py` = `probe572_p03.py` plus `dep_zapdeps` / `_expand_new_virtuals` /
`depgraph._add_dep` monkeypatches in the style of `probe572_trace4.py`. The
`dep_zapdeps` patch additionally replays the per-arm bin inputs
(`dep_check.py:432-541` availability + downgrade probes, `:599-736` bin ladder)
so each arm's bin is read off, not guessed. Full output is the probe's stdout;
the decisive three records (colour stripped):

**2a. Expansion of Try-Tiny's RDEPEND** (`dep_check.py:894-912` calls
`_expand_new_virtuals` inside `dep_check`, before `dep_zapdeps` at `:928`):

```
EXPAND-IN   ['||', Atom('>=virtual/perl-Scalar-List-Utils-1.400.0'), Atom('dev-perl/Sub-Name')]
EXPAND-OUT  [['||', [Atom('=dev-lang/perl-5.42*'), Atom('=virtual/perl-Scalar-List-Utils-1.680.100_rc')], Atom('dev-perl/Sub-Name')]]
```

The virtual arm becomes ONE conjunction arm: the nested `dep_check` choice over
the virtual's RDEPEND (`[=dev-lang/perl-5.42*]` — the only available arm, see 2c)
plus the exact-version virtual pin `=virtual/perl-SLU-1.680.100_rc` appended
(`dep_check.py:257-264`). The nested `dep_check` runs with `parent = Try-Tiny`
(the merge node) and `virt_parent = installed virtual` (`dep_check.py:237` sets
`mytrees["virt_parent"]`; the trace prints both).

**2b. The consumer-level choice** (parent Try-Tiny, the evaluation that drops the
virtual arm):

```
ZAPDEPS-IN  ['||', [=dev-lang/perl-5.42*, =virtual/perl-SLU-1.680.100_rc], dev-perl/Sub-Name]
ZAPDEPS-REDUCED ['||', [False, False], False]
ZAPDEPS-OUT ... -> [dev-perl/Sub-Name]
  ARM [=dev-lang/perl-5.42*, =virtual/perl-SLU-1.680.100_rc] | reduced: [False, False]
    atoms_after_recurse: ['=dev-lang/perl-5.42*', '=virtual/perl-SLU-1.680.100_rc']
    =dev-lang/perl-5.42*: avail = installed 5.42.2; avail_slot = dev-lang/perl:0;
      highest_in_slot = 5.44.0 (ebuild scheduled for merge, IN GRAPH)
      -> installed_downgrade=True  =>  bin `other` (dep_check.py:634-635)
  ARM dev-perl/Sub-Name | reduced: False
    avail = Sub-Name-0.280.0-r1; no downgrade; not in graph
    => bin `preferred_non_installed` (dep_check.py:694-703)
```

**2c. The nested choice is NOT empty — the guard demotes but still returns.**
Every evaluation of the virtual's own `||` (parents: installed virtual, merge
virtual, and Try-Tiny-with-virt_parent) prints:

```
ZAPDEPS-OUT ['||', =dev-lang/perl-5.42*, ~SLU-1.680.100_rc] -> [=dev-lang/perl-5.42*]
  =dev-lang/perl-5.42*: installed_downgrade=True => bin `other`
  ~SLU-1.680.100_rc: no portdb match => all_available=False => bin `other`
```

`=dev-lang/perl-5.42*` is returned despite the demotion because the first
`allow_masked=False` sweep (`dep_check.py:812-816`) returns the first
`all_available` choice in bin order — demotion to `other` does not make an arm
unavailable. `~SLU-1.680.100_rc` matches no tree package (tree has 1.630.0 and
1.680.0) and is skipped on that sweep.

**2d. Node creation.** Exactly one matching `_add_dep` in the whole run
(`depgraph.py:3364`):

```
ADD_DEP atom: dev-perl/Sub-Name | parent: Try-Tiny-0.320.0 (merge) | child: Sub-Name-0.280.0-r1 (merge)
```

The virtual is never `_add_dep`'d: no `_add_pkg_deps` walk of the virtual
selects anything, and the mergelist has no virtual entry.

## 3. The winning mechanism (a): the `installed_downgrade` guard, one level up

Exactly one of the three candidates: **the downgrade guard** (bug #531656,
`dep_check.py:531-541`), firing at the *consumer's* `||` over the *expanded*
conjunction — the same guard as #572, applied one level up.

Chain, each link traced above: (i) `_expand_new_virtuals` rewrites the virtual
arm to the conjunction `[inner choice + exact-virt pin]` (2a, `:257-290`);
(ii) the conjunction's only available sub-arm `=perl-5.42*` compares within
SLOT `0` — `avail_slot` is built slot-only as `Atom(cp + ":" + avail_pkg.slot)`
(`:532`; `Package.slot` is `"0"`, the subslot is separate), so the co-scheduled
perl-5.44.0 is `highest_in_slot`, is in the graph, and `downgrade_probe` does
not want the downgrade → `installed_downgrade=True` → the whole conjunction
lands in bin `other`, the last bin (`:634-635`); (iii) Sub-Name is available,
un-demoted and not in the graph → `preferred_non_installed`, bin index 1
(`:694-703`); (iv) only the winner becomes a node, parented to the consumer
(2d).

Rejected alternatives, with the trace line that kills each:

- *Not "dead nested expansion".* The nested expansion is not dead: its
  surviving arm is available and IS returned as the inner choice (2c,
  `:812-816`). What kills the virtual arm is the guard demoting that surviving
  arm at the outer level (2b).
- *Not "installed-preference".* No `preferred_*` bin is involved on the losing
  side: the virtual arm is a conjunction, never classified as installed (the
  `all_installed` fast path at `:600-616` needs `all_available` first, and the
  `graph_db is None` branch at `:618-633` does not apply — `graph_db` is set).
  The winner beats it by bin index (1 vs 8), not by installed status.

## 4. Portuale-side mechanism for Task 2 (b)

Inline `_expand_new_virtuals` port at consumer flatten time; no new queue
carrier, no seed-feed change. Exact sites (all paths under
`crates/portage-repo/src/resolve/`):

1. **Expand** in `pass/enqueue_deps.rs::enqueue_entry_deps`
   (`EnqueueEntryDepsArgs`, `:772`), immediately before the
   `split_off_disjunctions(&tokens, use_flags)` call (`:833`): for each `||`
   group arm naming an installed new-style virtual (pure-`||` RDEPEND per the
   existing `rev_dep_or.rs::virtual_rdepend_is_expandable`, `:641`;
   single-tree-version per `virtual_expansion_seeds`, `:681-693`; USE
   conditionals of the virtual's RDEPEND evaluated with the virtual's own
   effective USE as today at `:701-712`), replace the arm with the conjunction
   `[nested-choice atoms..., =virtual/<exact-installed-version>]` as ONE arm of
   the consumer's group. New function, e.g.
   `rev_dep_or.rs::expand_new_style_virtual_arm(ExpandNewStyleVirtualArmArgs<'_>) -> Option<Vec<String>>`,
   reusing `installed_meta.rs::live_metadata_for_installed` (`:51`),
   `or_preference.rs::split_or_groups` (`:544`), and a keep-demoted variant of
   the nested choice below. `DeferredDisjunction.owner` stays the consumer
   (the trace's `_add_dep` parent is Try-Tiny, 2d); no `DeferredSite` variant,
   no `installed_walked` insert (that erases the Q19 rows — #572 REPORT §"Third
   and fourth candidates").
2. **Nested choice** = Portage's inner `dep_zapdeps` return, which KEEPS the
   demoted-but-available arm: a variant of
   `rev_dep_or.rs::select_expansion_arms(SelectExpansionArmsArgs)` (`:506`)
   that returns the first available arm even when
   `or_preference.rs::alternative_downgrade_demoted` (`:27`) fires for it
   (today `:537-550` skips demoted arms and `:558-560` returns empty — that
   filter is the p02 shape; p03 needs the demoted arm kept, per 2c), then
   appends the exact-virt pin atom.
3. **Outer choice unchanged**: `disjunction_stack.rs::pop_disjunction`
   (`:204`) → `resolve_main_group` (`:454`) → the existing `or_preference`
   ranking, which already ports the guard — the conjunction arm demotes via
   `alternative_downgrade_demoted` exactly as traced (2b), the fall-through arm
   wins, and `enqueue_chosen_atoms` (`:667`) queues it as a normal node with
   the consumer as parent (2d). `rev_dep_or.rs::virtual_expansion_seeds`
   (`:656`) is untouched: its installed-satisfied skip (`:738`) correctly
   fires no seed on p03 (the nested survivor IS installed-satisfied) — the
   fall-through is handled inline, which is why the feed "cannot cover this by
   construction" (plan §9).
4. **Constraints the trace pins down**: installed-satisfied arms stay
   parent atoms/pins (Q19 — the conjunction's virt pin `=virtual/...` must
   never become a queue item); the nested choice and the outer choice both
   observe the graph with the co-scheduled upgrade present, so both choices
   happen at pop time against the drained graph, never frozen at enqueue time
   (Portage evaluates both inside one `_add_pkg_deps` walk).

## 5. Rejected shapes (c)

No product shape was tried in this task (S0 is evidence-only per the brief), so
there is no new measured product divergence to record. The three #572 REPORT
candidates stay rejected for p03, now with the trace saying why each one's
carrier contradicts the mechanism in §3:

- *Candidate 1 (installed-walk gate).* Portage never walks the virtual for this
  choice: every decisive evaluation has `parent = Try-Tiny`, and no
  `_add_pkg_deps(virtual)` evaluation selects Sub-Name. Product divergence
  already measured on p02 (Q19 cell moves; a first variant broke a second pin).
- *Candidate 2 (splice into the consumer, consumer-attributed).* The expansion
  is attributed to the virtual (`atom_graph` edges `dep_check.py:266-270`,
  `virt_parent` set at `:237`); the consumer-attributed variant measured the
  Q19 skip rows collapsing (1 row vs 2). For p03 it additionally destroys the
  conjunction structure the guard demotes as a unit (2b).
- *Candidates 3/4 (queue the expansion as hard deps owned by the virtual).*
  Measured inversion stands (prov Upgrade + `NoVisibleCandidate`, 0 skip rows
  vs baseline 2). The trace explains it: the nested survivor is demoted at
  both levels yet still returned as the inner choice (2c) — queueing it as a
  hard dep contradicts the choice, while Portage queues nothing except the
  outer winner (2d, exactly one `_add_dep`).

## 6. Files, commands, disposition

- Created: `LLM/evidence/2026-11-02-643/probe643_trace.py` (oracle + three
  monkeypatches), `LLM/evidence/2026-11-02-643/REPORT.md` (this file).
- Commands: `python3 LLM/evidence/2026-10-28-572/probe572_p03.py` (oracle
  holds, §1); `python3 LLM/evidence/2026-11-02-643/probe643_trace.py`
  (trace, §2; SUCCESS + expected MERGELIST + UNSAT frozenset()).
- No product code, no pmtest/fixture changes, no corpus impact. Nothing to
  verify beyond the two probe runs (both green, byte-exact expected output).
- Self-review: the bin assignments in §2b/2c are replays of `dep_check.py`
  logic in the probe, not Portage's own bin variables — risk: replay drift.
  Mitigated: the replay follows the cited lines atom-for-atom (availability
  via `match_pkgs(without_use)`, guard via `match_pkgs(avail_slot)` +
  in-graph test), and its outputs agree with Portage's own OUT on every
  printed evaluation (12/12 `ZAPDEPS-OUT` lines consistent with the replayed
  bins: the demoted/ unavailable arms lose, the available ones are returned).
  The one structural claim Task 2 depends on — "inner choice keeps the
  demoted arm" — is Portage's own OUT line, not the replay. Concern: none
  blocking; flagging the replay-vs-native caveat for the Task 2 implementer
  (the mechanism does not depend on it).
