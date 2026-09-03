# Plan: `||`-preference feedback driving a backtrack retry

*Working plan — 2026-09-03.* The last architectural item in Part 2.A
(`docs/scope-backlog.md`): the `'backtrack` loop reconciles slot
conflicts, masks unsolvable ones, tries autounmask levels in-loop, and
drives the slot-op-rebuild cascade — but a `||` (any-of) group's chosen
alternative is **never revisited** when it leads to a downstream failure.

## What real portage does

There is no dedicated "`||` backtracking" pass. It falls out of two
existing feedback paths, both of which end in `runtime_pkg_mask`:

1. **slot conflict** (`backtracking.py::_feedback_slot_conflict`) — the
   conflicting `pkg` is added to `runtime_pkg_mask`.
2. **missing dependency** (`depgraph.py:3484-3503` →
   `_feedback_missing_dep`) — when a dependency `dep.atom` of
   `dep.parent` has **no** matching package (not merely a USE change,
   and `dep.parent` isn't already masked and backtracking is allowed),
   real sets `_backtrack_infos["missing dependency"] = dep`,
   `_need_restart`, and masks `dep.parent`.

On the retry, `dep_check`/`dep_zapdeps` re-evaluates every `|| ( … )`
group. A masked package makes its choice-bin `all_available = False`
(`dep_check.py:449-476`), so a *different* alternative — the next one
whose packages are all unmasked — wins. When backtracking is exhausted
the unsatisfied dep is reported (`_unsatisfied_deps_for_display`), i.e.
the end state for a genuinely unresolvable `||` is unchanged.

## Where portuale stands

- `use_reduce_flat_disjunctive` + the closure at `backtracking_resolve`'s
  main BFS dep flatten (lib.rs ~11838) picks the **first alternative all
  of whose atoms `atom_currently_satisfiable` accepts** — base
  tree-visibility only. It does **not** consult the `'backtrack` loop's
  accumulated `slot_constraints` (the `!=cpv` negatives that are real's
  `runtime_pkg_mask`), so a backtrack-masked package inside a `||` group
  is still chosen.
- A dependency atom with no visible candidate becomes a
  `NoVisibleCandidate` `GraphEntry` and is **reported, never retried** —
  portuale has no "missing dependency" backtrack path at all.

## Slices

### Slice 1 — the accumulated runtime mask feeds `||` selection — **SHIPPED 2026-09-03**

Container-verified byte-for-byte vs real portage 3.0.82.2
(`TEST/scripts/42-or-backtrack.sh`). Shipped as three coupled changes:

1. **`atom_currently_satisfiable` grows `extra_constraints: &[String]`**
   (`&[]` no-op at every non-disjunctive call site), applying the same
   `!`-negative filter `resolve_pretend` uses. The two disjunctive
   closures inside `backtracking_resolve` (main BFS flatten +
   `enqueue_dependencies`'s `--deep` walk) pass
   `slot_constraints.get(&(cat,pkg))` per atom.
2. **`use_reduce_flat_disjunctive` defers a `||` group's chosen atoms**
   to after the plain deps of the same parent — real `_create_graph`
   drains `dep_stack` fully before popping one `_dep_disjunctive_stack`
   entry (`depgraph.py:3257-3268`), so `||`-pulled packages merge after
   the plain ones. Without this the retry's merge order diverged
   (`orbtclean` ahead of `orbttool-1.0`). Zero fallout across the 1242
   contract tests.
3. **slice-3 masks the *highest* conflicting instance** (`sc.instances`
   vercmp-max) rather than the first-resolved one — real backtracking's
   downgrade bias, and the version a `||`-pulled `>=` atom re-selects,
   so `dep_zapdeps` yields on the retry. No-op for every existing
   conflict fixture (their first-resolved version was already the
   highest).

Fixture `dev-libs/{orbttool,orbtclean,orbtblocked}`: `orbtblocked`
RDEPEND `|| ( >=dev-libs/orbttool-2.0 dev-libs/orbtclean )
=dev-libs/orbttool-1.0`. The first `||` alternative collides with the
hard `=orbttool-1.0` in `orbttool:0` → unsolvable conflict → backtrack
masks `orbttool-2.0` → retry picks `orbtclean`. `--backtrack=0` still
reports the conflict. Contract test
`test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`.

---

*Original Slice 1 sketch (for reference):*

Thread `slot_constraints` into the disjunctive closure. An alternative
atom whose only `atom_currently_satisfiable` candidates are all excluded
by the `!=cpv` negatives accumulated for its `cat/pkg` counts as
**not** satisfiable → the next alternative is chosen. This wires the
*already-shipped* slice-3 slot-conflict feedback (and any future mask
feedback) into `||` choice — real's `runtime_pkg_mask → dep_zapdeps`
path, nothing more.

- `atom_currently_satisfiable` grows an `extra_constraints: &[String]`
  parameter (empty `&[]` at every existing call site — a strict no-op),
  applying the same `!`-negative filter `resolve_pretend` already uses
  (lib.rs 7358/7443). Or a thin wrapper if that keeps the diff smaller.
- The two disjunctive closures inside `backtracking_resolve` (main BFS
  flatten + `--deep` `AlreadyInstalled` walk) look up
  `slot_constraints.get(&(cat,pkg))` per atom and pass it. The two in
  `resolve_pretend_graph` (`root_deps_*`) stay `&[]` — a `--root-deps`
  build-dep `||` group is not a backtracking target.
- **Blast radius:** nil unless a pass has *already* accumulated a
  negative (only slice 3's unsolvable-slot-conflict path does today)
  **and** that cp appears inside a `||` group. Every existing `||`
  fixture has no such negative → byte-identical.
- **Fixture:** `dev-libs/orbtblocked` with
  `RDEPEND="|| ( dev-libs/orbtconflict dev-libs/orbtclean )"` where
  `orbtconflict` forces an unsolvable slot conflict (two parents pin
  incompatible slots) and `orbtclean` resolves cleanly. Real picks
  `orbtclean`. Container cross-check.

### Slice 2 — "missing dependency" backtracking — **SHIPPED 2026-09-03**

Container-verified byte-for-byte vs real portage 3.0.82.2
(`TEST/scripts/43-or-missing-dep.sh`). As shipped:

- A dependency atom resolving to `NoVisibleCandidate` records a
  `missing_dep_trigger = ((parent_cat, parent_pkg), "!=parent-cpv")`
  once per pass, gated on: backtracking live (`backtrack_max > 0`,
  `mask_phase == None`), the parent is a **merge-bound, non-top-level**
  entry, the `!=parent-cpv` not already in the `missing_dep_masked`
  latch, **and** the atom has no candidate even ignoring its USE deps
  (`!atom_currently_satisfiable(bare_atom)` — real `depgraph.py:3473`
  does not missing-dep backtrack a USE-only failure; that stays the
  autounmask machinery's job and an unfixable `[flag]` dep is a plain
  NVC).
- The driver (after the autounmask re-run check) folds the negative
  into `slot_constraints`, latches it, `backtrack_iteration += 1`,
  `continue 'backtrack`. Slice 1's `||` closure then yields to the next
  alternative. Latch + ceiling guarantee termination; an un-fixable dep
  falls through to the unchanged NVC report.

Fixture `dev-libs/{ormisstop,ormissbad,ormissgood}`: `ormisstop`
RDEPEND `|| ( dev-libs/ormissbad dev-libs/ormissgood )`; `ormissbad`
(visible) RDEPEND `dev-libs/ormiss-nonexistent` (no candidate anywhere).
Real masks `ormissbad`, retry picks `ormissgood`; `--backtrack=0`
reports the missing dep. Contract test
`test_or_group_alternative_yields_to_the_next_on_a_missing_transitive_dep`.
Zero fallout across the 1243 pre-existing contract tests (one existing
autounmask-breakage unit test flagged the USE-only guard, now in place).

---

*Original Slice 2 sketch (for reference):*

When resolving a dependency atom yields `NoVisibleCandidate`, and
backtracking is live (`backtrack_max > 0`, `backtrack_iteration <
backtrack_max`, `mask_phase == None`), and the **parent** entry's
`cat/pkg` is not already in `slot_constraints` as a full mask: add
`!=<parent-cpv>` to `slot_constraints[parent_cp]` and `continue
'backtrack`. A `missing_dep_masked: HashSet<(String,String,String)>`
latch (mirrors real's `dep.parent not in _runtime_pkg_mask`) prevents
re-masking the same parent → guaranteed termination. When the latch or
`backtrack_max` blocks the retry, fall through to the current
`NoVisibleCandidate` report unchanged.

Combined with Slice 1, a `|| ( a b )` whose `a` is visible but pulls in
an unsatisfiable transitive dep now yields to `b`.

- **Risk:** this touches the common "unsatisfied dependency" path. The
  guard set is: only under `--backtrack != 0`, only once per parent,
  only when `mask_phase == None`, and the reported end state for a
  still-unresolvable graph must stay identical (contract tests for
  every existing `NoVisibleCandidate` fixture must not move).
- **Fixture:** `dev-libs/ormisstop`
  `RDEPEND="|| ( dev-libs/ormissbad dev-libs/ormissgood )"`;
  `ormissbad` (visible) `RDEPEND="dev-libs/ormiss-nonexistent"`;
  `ormissgood` clean. Real picks `ormissgood`, no missing-dep error.
  Plus a negative fixture: `|| ( a b )` where *both* subtrees are
  unsatisfiable → the missing-dep error still shows (backtrack
  exhausted), byte-identical to today.

## Lockstep + verification

Both slices ship Rust + `emerge_pretend_reference.py` in one commit,
byte-identical stdout/stderr under the full contract suite. Each slice
gets a `TEST/scripts/NN-or-backtrack-*.sh` container diff vs real
portage 3.0.82.2 that must be empty. `cargo fmt`/`clippy` clean.
