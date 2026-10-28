# S0 #572 (from #567) — why the fall-through `||` arm never reaches the graph

Date: 2026-10-28. Branch `slice/572-or-reverify` (portuale + pmtest).
No product code kept from this task; the candidate change is recorded and rejected
below. Oracle: Portage 3.0.83's own resolver (`portage/tests/resolver/ResolverPlayground.py`,
imported from this tree's `3rdparty/portage/lib`) plus the `p572*` fixtures.

## Reproduction

Fixture family (pmtest `fixtures/`, `p572*`), mirroring the #567 S0 shapes:

| fixture | mirrors |
|---|---|
| `dev-libs/p572prov-5.42` (tree) + `-5.44` (tree), installed `5.42`, `SLOT="0/5.42"` / `"0/5.44"` | `dev-lang/perl-5.42.2 → 5.44.0` |
| `virtual/p572virt-2.0` (tree + vdb), `RDEPEND="|| ( =dev-libs/p572prov-5.42* =dev-libs/p572prov-5.40* ~dev-libs/p572enc-3.210 )"` | `virtual/perl-Encode-3.210.0-r2` |
| `dev-libs/p572enc-3.210` (tree only) | `perl-core/Encode-3.210.0` |
| `dev-libs/p572leaf-1.0` (tree only), `RDEPEND=">=virtual/p572virt-2.0"` | `dev-perl/Encode-Locale` (requested, not installed) |

Request (the S0 p02 shape, no `--update`/`--deep`):
`emerge -p =dev-libs/p572prov-5.44 dev-libs/p572leaf`.

* Portage (playground, `probe572.py`): `['perl-core/Encode-3.210.0', 'dev-lang/perl-5.44.0', 'dev-perl/Encode-Locale-1.5']`
  — i.e. Encode plus the perl upgrade plus the requested leaf.
* portuale (current `main`, `tests_p572.rs`): Encode absent; the virtual stays
  `AlreadyInstalled` and the leaf merges — the #567 `[missing]` rows.

Shape fidelity checks that must hold or the reproduction is worthless (all four
were run, `probe572_plain.py`, `probe572_plain2.py`, `probe572_mixed.py`):

* A *plain* dep on the installed virtual (no `||`) is **not** added by Portage.
* A plain atom beside the `||` suppresses the expansion entirely (the virtual is
  no longer a pure new-style virtual for `dep_check`), so the fixture must keep
  the virtual's RDEPEND `||`-only.

## Mechanism (Portage side, traced)

`probe572_trace4.py` monkeypatches `portage.dep.dep_check.dep_zapdeps` and prints
each `||` evaluation with the arm's vardb/portdb view:

```
ZAPDEPS-IN  ['||', Atom('=dev-lang/perl-5.42*'), Atom('=dev-lang/perl-5.40*'), Atom('~perl-core/Encode-3.210.0')]
ZAPDEPS-OUT [...] -> [Atom('~perl-core/Encode-3.210.0')]
   arm =dev-lang/perl-5.42*      | vardb: ['dev-lang/perl-5.42.2']  | portdb: ['dev-lang/perl-5.42.2']
   arm =dev-lang/perl-5.40*      | vardb: []                        | portdb: []
   arm ~perl-core/Encode-3.210.0 | vardb: []                        | portdb: ['perl-core/Encode-3.210.0']
```

1. `dep_check()` runs `_expand_new_virtuals` (`3rdparty/portage/lib/portage/dep/dep_check.py:37-…`)
   on the **consumer's** dependency string before `dep_zapdeps`. An atom naming a
   new-style virtual is expanded so the virtual's own `RDEPEND` becomes part of
   the consumer's requirements (bug #141118). The virtual atom itself stays.
2. So the virtual's `||` is evaluated *in the consumer's graph*, after the
   argument atom `=dev-lang/perl-5.44.0` is already a merge node.
3. `dep_zapdeps`'s downgrade guard (bug #531656 — `installed_downgrade`,
   `dep_check.py:531-542`) demotes the `=dev-lang/perl-5.42*` arm: its best
   candidate (`5.42.2`) is lower than the version the graph already holds in
   that slot (`5.44.0`) and `downgrade_probe` does not want it, so the choice
   lands in `other` (last bin).
4. `~perl-core/Encode-3.210.0` is `preferred_non_installed` (bin 1) and wins;
   `_add_dep` (`depgraph.py:3364`) adds `perl-core/Encode` as a node, shown as a
   child of the `[nomerge]` virtual.

## Mechanism (portuale side, measured)

Same fixture, instrumented (`alternative_downgrade_demoted` +
`use_reduce_flat_disjunctive` ranks + `pop_disjunction` site):

```
DBG rank ["=dev-libs/p572prov-5.42*"]  -> Other
DBG rank ["=dev-libs/p572prov-5.40*"]  -> OtherInstalledAnySlot
DBG rank ["~dev-libs/p572enc-3.210"]   -> Available
DBG pop site_installed=true owner=("virtual","p572virt") chosen=["~dev-libs/p572enc-3.210"]
```

The guard already exists and fires (`or_preference::alternative_downgrade_demoted`,
port of bug #531656); the fall-through arm is chosen. The gap is **reachability**:
the `||` is only evaluated by the reverse-dependency pin pass
(`rev_dep_or::consumer_or_demotions`), which files pins/demotions and cannot add a
graph node. In the plain first walk the virtual atom resolves to `AlreadyInstalled`
and nothing expands the virtual's `RDEPEND`, because portuale has no
`_expand_new_virtuals` port (`grep -rn expand_new_virtuals crates/` → one comment
citation only).

## Candidate fix (tried, rejected)

`enqueue_installed_deps` (`resolve/pass/unresolved_dep.rs:418`) gates the installed
walk on `ctx.deep.recurses_at(depth)`. Letting an installed **new-style virtual**
through that gate (`key.0 == "virtual" && live_metadata_for_installed(..).is_some()`,
no `installed_walked` insert) makes the target shape match Portage exactly, and the
full `-p portage-repo` suite moves only one cell:

```
tests::tests_consumer_or_view::sibling_virtuals_of_a_merge_bound_parent_share_the_atom_a_sibling_chose
  left: []   right: [("virtual/p590vva-1:0/0::testrepo", "=dev-libs/p590vprov-5.42*"), ...]
```

That cell is pinned against Portage (q19 capture), so the gate is **not** the
faithful shape: Portage inlines the virtual's deps into the *consumer's* dependency
string and never walks the virtual as an installed package (no `_add_pkg_deps`).
A first variant that also inserted `installed_walked` broke two pins
(`update_without_deep_keeps_an_installed_virtual_provider_silent` as well).

## Second candidate (tried, rejected) — attribution is the whole problem

Splicing the virtual's `RDEPEND` tokens into the consumer's flattened dep list
(`expand_new_style_virtuals` called from `enqueue_entry_deps`, before
`split_off_disjunctions`, single-tree-version only) leaves **one** cell moved, and the
move names the missing piece exactly:

```
sibling_virtuals_of_a_merge_bound_parent_share_the_atom_a_sibling_chose
  left: [("dev-libs/p590xpull-1:0/0::testrepo", "=dev-libs/p590vprov-5.42*")]
  right: [("virtual/p590vva-1:0/0::testrepo", "=dev-libs/p590vprov-5.42*"), ("virtual/p590vvb-1:0/0::testrepo", …)]
```

The arm is now enqueued (so the target shape would work), but it is attributed to the
*consumer*. Portage sets the **virtual** as the expanded atom's graph parent
(`_expand_new_virtuals`: `atom_graph.add(pkg, virt_atom_node)` / `add((x, id(x)), graph_parent)`),
which is what Q19's capture shows. So the expansion must be queued with
`owner = (virtual, pkg)` and `owner_version = <virtual version>`, and its `||` groups
must defer against that owner — while *not* marking the virtual `installed_walked`
(that is what erased the Q19 rows in the first candidate). A `DeferredSite` variant
that behaves like `Installed` minus the `installed_walked` insert is the shape to add.

Conclusion: the fix must be an inline `_expand_new_virtuals` port at flatten time,
attributed to the virtual, with its groups deferred on a site that does not mark the
virtual as a walked consumer. Recorded here so the next agent does not re-run the same
three experiments.

## Third and fourth candidates (tried, rejected) — the queue is the wrong carrier

Both were built on the `DeferredSite::Installed { mark_walked: false }` variant and a
`enqueue_new_style_virtual_deps` pass that queues a virtual's `RDEPEND` with
`owner = ("virtual", pkg)`:

| run | graph seen by `tests::tests_p572::scratch_q19` |
|---|---|
| baseline | `p590vprov AlreadyInstalled 5.42`, `virtual/p590vva|vvb AlreadyInstalled 1`, `p590xpull New 1`, **2 skip rows** (`=dev-libs/p590vprov-5.42*`, skipped 5.44) |
| expansion queued after the entry's deps | `p590vprov Upgrade 5.42→5.44` **+ `p590vprov NoVisibleCandidate`**, `p590xpull New 2`, **0 skip rows** |
| expansion queued before them | identical to the row above |
| plus "drop chosen arms an installed package satisfies" | identical (the NVC survives the filter) |

So queueing the expansion (owned by the virtual) makes the virtual's `||` arm a *hard*
dependency of the walk: the backtracker then prefers the consumer's `=5.44*` upgrade and
abandons the virtual's `=5.42*` as a masked miss (`NoVisibleCandidate`), where Portage
keeps `5.42`, skips `p590xpull-2` and reports the two skip rows. The baseline gets that
right through the **reverse-dependency pin** path, not the queue.

Design consequence for the next attempt: a new-style virtual's expansion must feed the
*same* two channels Portage has:
- an arm an installed package satisfies stays a **parent atom / pin** (the conflict
  machinery then reports the skip row) — `rev_dep`/`consumer_or_arms` already models
  this;
- an arm that needs a **fresh merge** (the `#572` target: `~perl-core/Encode-3.210.0`)
  must become a node.

Doing both through the queue inverts the backtracking preference and is rejected. The
candidate shape is therefore: keep the expansion out of `enqueue_entry_deps`, and teach
the reverse-dependency pass to hand its *non-installed* chosen arms to the next walk as
queue items (a feedback kind, or a seed list like `complete_seed_atoms`), while leaving
installed-satisfied arms as pins.

## The clue the last experiment produced (start here next time)

Measured on the Q19 fixture with the expansion reverted (`scratch` probe, baseline):

```
ENTRY dev-libs/p590vprov AlreadyInstalled { version: "5.42" } req=[("dev-libs", "p590xpull")]
ENTRY virtual/p590vva AlreadyInstalled { version: "1" }       req=[("dev-libs", "p590xpull")]
ENTRY virtual/p590vvb AlreadyInstalled { version: "1" }       req=[("dev-libs", "p590xpull")]
ENTRY dev-libs/p590xpull New { version: "1" }                 req=[]
SKIPPED 2
```

In the baseline the two skip rows exist because `dev-libs/p590xpull-2` asks for prov
**5.44**, the installed virtuals' `=p590vprov-5.42*` **pin** blocks it, `p590xpull-2` is
masked and the 5.44 request is withheld. With the expansion in place the same atoms also
arrive as queue items owned by the virtuals, and the run upgrades prov to 5.44 and files
the arm as `NoVisibleCandidate` — i.e. **the pin that blocks 5.44 is lost once the atom is
also a queued dep.** So the next attempt must first establish, with a print of
`constraints_from_consumer`'s `enforced`/`dropped` pins for that fixture, whether
`rev_dep` skips filing the pin for an atom that is also a queue item — and if so, keep
filing it. Only then is it safe to queue the fall-through arm.

## Product-change status

Reverted. `cargo test --release -p portage-repo` is green (1303 passed, the `#572` pin
ignored), clippy clean. Nothing in `crates/portage-repo/src/resolve/` is modified.

## Disposition

* `LLM/superpowers/plans/2026-10-28-572-or-bound-reverify.md` carries the slice.
* `crates/portage-repo/src/tests/tests_p572.rs` is the RED pin (fails on `main`;
  goes green with the plan's Task 2).
* No product code, no pmtest contract pin, nothing committed in this task.
* Bed rows `perl-544-stabilization-version-selection-{encode,sub-name}` could not be
  re-run here (no container runtime on this host); the plan's Task 4 names the bed
  re-grade as the closing gate.
