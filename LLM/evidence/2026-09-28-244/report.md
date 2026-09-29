# #244 Slice A — diagnosis (S0a–S0e) and the pin inventory

Batch: [`docs/batch-2026-09-28_244.md`](../../batch-2026-09-28_244.md) §4.
Portuale under test: `main` @ `feedb558` (release). Real: Portage 3.0.82.2
(container, and the vendored checkout's `ResolverPlayground`). Every capture
cited here is in this directory ([`README.md`](README.md)).

## S0a — the six `aub0` orders, real text

Fixture facts that decide the shape: `aub0a` RDEPENDs `aub0d[-foo]`, `aub0b`
RDEPENDs `aub0d[foo]`, `aub0c` RDEPENDs `>=aub0d-1`; `aub0d-0` has `IUSE="foo"`,
`aub0d-1` has `IUSE="bar"`; the fixture profile enables `foo` globally
(`repo/profiles/base/make.defaults`). So `aub0d-0` is `foo` by default, **A**
(`aub0a`) is the requester that needs an autounmask change (`-foo`), **B**
(`aub0b`) is satisfied by the default, and **C** (`aub0c`) can only take
`aub0d-1` — a second instance in slot 0.

Real walk order (playground `--debug`): the three argument packages are added
in argv order, then `_dep_stack` is popped LIFO, so the **last** argument's
dependencies are walked first. For argv `x y z` the dep walk order is `z, y, x`.

**`--autounmask-backtrack=y`** (`fixture-oracle/real/--autounmask-backtrack_y_*`),
rc 1 in every order. Stdout carries no merge rows. Every order ends with the
same block:

    emerge: there are no ebuilds built with USE flags to satisfy "dev-libs/aub0d[-foo]".
    !!! One of the following packages is required to complete your request:
    - dev-libs/aub0d-0::testrepo (Change USE: -foo)
    (dependency required by "dev-libs/aub0a-0::testrepo" [ebuild])
    (dependency required by "dev-libs/aub0a" [argument])

The only order-dependent part is a slot-conflict block printed **before** that
miss, in the two `a`-first orders only:

| argv | walk order | slot-conflict prefix |
|---|---|---|
| c b a | A, … | none |
| c a b | B, A, … | none |
| b c a | A, … | none |
| b a c | C, A, … | none |
| a c b | B, C, A | `aub0d-0` (`USE="foo"`, by B) first, then `aub0d-1` (by C) |
| a b c | C, B, A | `aub0d-1` (by C) first, then `aub0d-0` (`USE="foo"`, by B) |

This corrects #244's entry wording: the reported instance and USE change do
**not** depend on the order. Only the slot prefix does.

**`--autounmask-backtrack=n`** (`fixture-oracle/real/--autounmask-backtrack_n_*`),
rc 1 in every order, **two shapes**:

- `c b a`, `b c a`, `b a c` (A walked before B): the same missing-USE block as
  above, with no merge rows and no slot prefix.
- `c a b`, `a c b`, `a b c` (B walked before A): a merge list
  `aub0d-0 USE="-foo"`, `aub0d-1 USE="-bar"`, then the three arguments **in
  argv order**. It is followed by the slot-conflict block, in which
  `aub0d-0 USE="-foo"` is "pulled in by" **both** `aub0d[foo]` (B) and
  `aub0d[-foo]` (A), B first, each with its own `^^^` marker line, and
  `aub0d-1` is pulled in by C. The instance order in that block is admission
  order (`aub0d-0` first when B is walked before C, `aub0d-1` first in `a b c`).
  Then come the USE-changes block (`=dev-libs/aub0d-0 -foo`, required by A) and
  the "backtracking has terminated early" notice.

Two-argument cross-check (`fixture-oracle-2/`): `b a` fails with the
missing-USE block under both values. `a b` fails under `=y` and prints the
list plus USE change plus notice (no conflict, since there is no C) under `=n`.

## S0b — the abort family and the sister cells, real text

Same bed run (`fixture-oracle/real/`, real 3.0.82.2). Normalising only the
bed noise (the `for <ROOT>` suffixes, the `Undefined license group` warning,
Global Updates, news, the header and timing lines), portuale matches real
byte for byte on `abort-unsat-{mid,last}`, both `does-not-exist`/`newpkg`
orders and `--autounmask=n =odc0a-{1,2}`. The remaining diffs are
pre-existing and **not** walk-order effects:

- `abort-masked-{mid,last}`, `abort-masked-cycle`: real also prints the
  `package.mask` file's comment lines (the masked-by-file comment
  disclosure). Portuale omits them.
- `abort-cycle-{mid,last}`, `abort-au-cycle`, `abort-au-restart-cycle`: real's
  reduced cycle list duplicates tree rows and counts them (`Total: 4`).
  Portuale dedups. This is the standing tree-duplication cut
  (`history/abort-path-spec.md` §7 (a)).
- `abort-au-plain`, `useflagpkg[-foo]`: one-space vs two-space before `USE=`
  under bare `-p`.
- `useflagpkg[-foo]`: portuale's extra `# required by useflagpkg-1.0` self-row
  (**#248**, not fixed here).
- `--autounmask-use=n useflagpkg[-foo]` and `=mia0a-{1,2}`: real prints the
  "built with USE flags" block with a `Change USE` / `Missing IUSE` line for
  an **argument** atom. Portuale prints the bare "no ebuilds to satisfy" line.
  This is `_show_unsatisfied_dep`'s argument arm, not a walk-order effect.
- `--backtrack=0 aus0l aus0m`: identical apart from spacing (see D0 below).

`dev-libs/aubreaktop` (`fixture-oracle-2/`): under the default, real matches
portuale's collected `>=aubreaksub-1.0 brk` change, except that real's chain has
three lines (`aubreakwant`, `aubreaktop`, argument) and portuale's has two.
Under `=y`, real fails with `aubreaksub[brk]`, and its chain is **only** the
failing dependency's own path (`aubreakwant` → `aubreaktop` → argument).
Portuale prints `aubreaktop` → argument → `aubreakunwant` → `aubreakwant`,
which is the "#19-parked multi-branch narrowing".

## S0c — the pin inventory (by name)

Direction: **moves** = text changes toward the real capture; **unchanged**
= must stay byte-identical.

- `test_emerge_pretend_contract.py` CASES, the six rows
  `autounmask: upstream test_autounmask_use_breakage pg0 {c/b/a,…,a/b/c} fails like real`:
  rc 1 **unchanged**. The row text is prose and is corrected only if it
  claims more than rc. `test_output_invariants.py` runs over them: output
  **moves** (no rows, the miss block, the order-dependent prefix), and the
  invariants must still hold.
- `test_autounmask_breakage_abandons_autounmask_when_a_flag_is_wanted_both_ways`:
  the `=y` stderr chain **moves** to real's three lines (`aubreakwant`,
  `aubreaktop`, argument). The default-mode half stays **unchanged** except the
  missing `# required by dev-libs/aubreakwant-1.0::testrepo` line (a dep-chain
  fill gap on an existing-slot flip; it is in the same family as #248 but is a
  missing row, not an extra one, so it is handled in Slice B only if
  mechanism (1)'s chain rule reaches it, otherwise it gets its own number).
- Rust `autounmask_use_breakage_abandons_autounmask_and_re_resolves_clean`
  (`portage-repo`): it asserts the graph state (not the display), so it is
  expected **unchanged**. Re-checked after S1.
- `test_abort_*` (10 functions, `test_emerge_pretend_contract.py`) and the
  Rust `abort_outcome_*` family: every abort fixture carries **one** walk-time
  failure, so the first-failure choice cannot change. Expected **unchanged**,
  and verified by name after S1.
- The first-bad-wins pair (`dev-libs/does-not-exist` + `dev-libs/newpkg`, both
  orders) is an **argument**-level failure (before `_create_graph`) and is
  already byte-identical to real. **Unchanged**.
- New pins (Slice C): the twelve `aub0` cells (six orders × two
  `--autounmask-backtrack` values) plus the two-argument pair, pinned from
  `fixture-oracle{,-2}/real/`.

## S0d — "the order-dependent slot prefix", named exactly

Confirmed from real's code and the captures. After the backtrack loop,
`_backtrack_depgraph` (`_emerge/depgraph.py:12262-12280`) re-runs one clean
pass with `myparams["autounmask"] = False` when `autounmask_breakage_detected()`
(`:11779`) finds an unsatisfied dependency that a package would satisfy but for
an autounmask change. In that clean pass `_create_graph` (`:3254-3271`) returns
0 at the first failing `_add_dep` (`:3483-3522`, A's `aub0d[-foo]`). Every node
admitted **before** that point stays in the package tracker. If the dep walk
reached both B (→ `aub0d-0`) and C (→ `aub0d-1`) first, the tracker holds a slot
conflict, and `display_problems` (`:11104`) prints it before the leftover
`_unsatisfied_deps_for_display` item (`:11279`). The conflict's instance order
is tracker insertion order (admission order). Example: `a c b` →
`fixture-oracle/real/--autounmask-backtrack_y_dev-libs_aub0a_dev-libs_aub0c_dev-libs_aub0b.txt`.
So the phrase names the conflict block **before** the miss, whose presence and
entry order follow walk order, as §4 of the batch assumed.

Why real reaches the clean pass (the other half of the mechanism), from the
`--debug` twins:

- A before B: A's `aub0d[-foo]` creates `aub0d-0` with the **needed** change
  `-foo`. B's `aub0d[foo]` would need `+foo`, which contradicts a needed
  change. The flip is rejected (`depgraph.py:7714`), so B's dep fails and the
  pass fails. Under both values, `need_config_change()` ends the loop and the
  breakage check sends it to the clean pass.
- B before A: B takes `aub0d-0` at its default `foo`, and A's flip to `-foo` is
  the first needed change on that node, so it is accepted. B's parent edge
  stays, now violated (both parents show in the conflict block). The pass
  completes with a slot conflict (C's `aub0d-1`) and an autounmask change.
  Under `=n`, `need_config_change()` stops there, which gives the list,
  conflict, change and notice. Under `=y`, the loop goes on: the `-foo` change
  is fed back as config, B's `aub0d[foo]` now fails in every later try, and the
  exhausted loop's breakage check sends it to the same clean pass
  (`bt-y-cab.debug.txt`: tries 0–2, then `autounmask breakage detected`).

## S0e — portuale's divergence points and the narrowest port points

1. **Walk order.** Portuale already admits the argument packages in LIFO
   order (its `--debug` trace for `c b a` walks `aub0a` first) and walks deps
   through the `run_pass` BFS queue. For these depth-1 graphs that equals
   real's LIFO order, so the `aub0` divergence is **not** caused by processing
   order. For deeper graphs BFS ≠ DFS. The port point for "which failure is
   first, and what was admitted before it" is a post-pass replay of real's
   `_create_graph` LIFO/DFS over the settled graph. `merge_order::build_digraph`
   already replays that walk for `.order`, and its discovery loop is the model.
   Owner decision 2026-09-29: post-pass replay, not a walk rewrite.
2. **Needed-change visibility (root cause of the `aub0` divergence).** When a
   node is **created** by a requester whose use-dep needs a flip (A first), the
   run_pass candidate arm ("Real `--autounmask-use` USE resolution") flips
   `use_flags` and records an `AutounmaskChange`, but it never writes the flip
   into the pass's needed-change map (`use_overlay`). A later requester of the
   same node (B) is checked in the existing-slot arm against
   `effective_use_flags(config, …)`, the **default** USE. That check finds
   `foo` on and reports B satisfied, so the contradiction (`use_broke`) is never
   raised and the breakage clean pass never runs. It also explains portuale's
   inconsistent output (the row says `USE="-foo"`, the conflict block says
   `USE="foo"`). The port point is to record the created-node flip as a needed
   change and check later requesters against the flipped USE, which is real's
   `_pkg_use_enabled` with `_needed_use_config_changes`.
3. **Abort decision.** `abort_outcome` (`portage-repo`) classifies the settled
   graph after the pass and takes the first `NoVisibleCandidate` in `entries`
   order. Its doc comment names the cut this batch closes. The port point is
   the replay's first failure.
4. **Slot prefix.** Slot conflicts are recorded in-walk regardless of any
   failure, and the pretend display prints them **after** the unsatisfied
   block on the abort path. The port point is to keep only conflicts whose
   instances the replay admitted before the first failure, in admission order,
   and print them before the miss (real's `display_problems` order).
5. **Dep chain of the reported failure.** The shared chain walker unions every
   requirer of the failing `cat/pkg`. The port point is the first failure's own
   parent path, which is what real's `_get_dep_chain` walks for the
   `_unsatisfied_deps_for_display` item.

## D0 input — the #195 batch-4 sister cells

- `--backtrack=0 aus0l aus0m`: already identical to real apart from the
  bare-`-p` `USE=` spacing: list + `>=dev-libs/aus0k-1 -foo` change, no
  failure. It is the "default-satisfied requester walked first" shape
  (`aus0m`'s `aus0k[foo=]` is walked before `aus0l`'s `aus0k[-foo]`, so the
  `-foo` flip is the first needed change and is accepted) — a regression
  cell for mechanism 2, which must keep it unchanged. The #195 entry's
  observation (contradictory rows) is stale.
- `=mia0a-{1,2}`, `--autounmask-use=n useflagpkg[-foo]`: `_show_unsatisfied_dep`'s
  argument-atom arm. It is untouched by the walk mechanics, so it is a new
  residue number at D0.
- `--autounmask=n =odc0a-{1,2}`: already byte-identical to real (the #195
  entry's observation is stale).
