# Task 22 — `dep_zapdeps` finer choice bins

Agent brief for backlog item #22 (`docs/backlog-tasks.md`), resolver section [A]. Suggested repo location: `docs/task-22-dep-zapdeps-choice-bins.md`.

**Read [`AGENTS.md`](https://claude.ai/AGENTS.md) and [`docs/agent-context.md`](https://claude.ai/chat/agent-context.md) first.** This file does not replace them. It adds what is specific to this task: the real algorithm, the current state of portuale, the traps, a slice plan, and which kind of model should do which part.

Line numbers below are from portuale commit `659a039` and upstream Portage `master`. They drift, so always re-locate code by **function name** and re-verify before trusting any claim here (AGENTS.md step 1).

------

## 0. Corrections to the backlog line (fix these first)

The one-liner in `backlog-tasks.md` has two stale pointers:

- It cites "real `depgraph.py` `_dep_zapdeps`". The function is actually **`dep_zapdeps` in `lib/portage/dep/dep_check.py`**. It is called from `dep_check()` in the same file. `depgraph.py` only mentions it in comments.
- It says "affects `solver_bridge.rs` `||` closures". **It does not.** `solver_bridge.rs` only walks `DepEntry::AnyOf` to build an over-approximated closure (`referenced_cpns`). The choice between alternatives is made by pubgrub/resolvo, not by a zapdeps-style ranking. Keep the bridge **out of scope** for this task; it is related to #34 (pubgrub over-merge), not to #22.

Also note that `backlog-tasks.md` is generally stale. Many Tier-1/Tier-2 items are marked shipped in `docs/scope-backlog.md`. #22 itself is still open (`scope-backlog.md` Part 4, item 1).

------

## 1. Ground truth: real `dep_zapdeps`

### 1.1 Entry and recursion

- **Early exit.** `if not reduced or unreduced == ["||"] or dep_eval(reduced): return []`. `reduced` holds the satisfied flags computed by `dep_wordreduce` against the caller's db (in depgraph, the composite graph db). An already-satisfied structure yields no atoms.

- **All-of lists** recurse and return only the *unsatisfied* atoms.

- **For a `||` node**, each alternative's `atoms` is either `[x]` or `dep_zapdeps(x, …)` for a nested list. So a nested all-of alternative contributes **only its unsatisfied atoms**. This is a trap; see §3.

- **Before zapdeps**, `dep_check()` does two things:

  - it expands new-style virtuals (`_expand_new_virtuals`);
  - it may convert the structure to DNF (`_overlap_dnf`, used when `||` groups overlap on the same cp).

  `minimize_slots` is `True` **only when `_overlap_dnf` actually changed the structure**. It is *not* a depclean flag.

### 1.2 The nine bins, in order

```text
0  preferred_in_graph   ≡ preferred_installed ≡ preferred_any_slot   (ONE list object)
1  preferred_non_installed
2  unsat_use_in_graph
3  unsat_use_installed
4  unsat_use_non_installed
5  other_installed
6  other_installed_some
7  other_installed_any_slot
8  other
```

`preferred_installed = preferred_in_graph` and `preferred_any_slot = preferred_in_graph` are **aliases**. They are one bin, not three. Order inside bin 0 comes only from append order plus the in-bin reorder in §1.5.

### 1.3 Per-alternative facts

The loop runs over non-blocker atoms.

- `avail_pkg`

  - It is the highest match of `atom.without_use` in the repo/bin db. If the parent replaces this child, the child being replaced is used instead.
  - No match sets `all_available = False` and `all_use_satisfied = False`, then `break`.

- **`conflict_downgrade`** (bug 531656). The graph db has more than one match in `avail_pkg`'s slot, `avail_pkg` is lower than the highest of them, and `downgrade_probe(avail_pkg)` is false.

- USE deps

   (

  ```
  atom.use
  ```

  )

  - If nothing matches the full atom, set `all_use_satisfied = False`.

  - Then compute `violated_conditionals` against `pkg_use_enabled(avail_pkg)`.

  - Set 

    ```
    all_use_unmasked = False
    ```

     in either of two cases:

    - a violated *enabled* flag is in `use.mask`;
    - else, a violated *disabled* flag is in `use.force` and not in `use.mask` (bug 515584).

  - If the full atom does match, `avail_pkg` becomes the highest USE-matching package.

- **`installed_downgrade`**. `avail_pkg` is lower than the highest package in its slot, `downgrade_probe` is false, and that highest package is installed or in the graph.

- `slot_map[cp:slot] = avail_pkg`

  , and 

  `cp_map[cp]`

   is kept internally consistent (bug 600346):

  - If an existing `cp_map` entry is in the same slot, compute two flags: whether every atom of that slot matches the current package, and whether every atom matches the previous one.
  - If the previous package matches all and the current one does not, `continue`. This skips **only** the `cp_map` update; `slot_map` and `slot_atoms` were already written.
  - Otherwise, update `cp_map` if the current package is higher, or if it matches all and the previous one did not.

- **`want_update`**. Any `slot_map` package for which `graph_interface.want_update_pkg(parent, pkg)` is true.

- **`new_slot_count`**. The number of non-virtual slot atoms with no graph-db match. For a removal action it is `len(slot_map)` instead.

### 1.4 Classification

The graph-db-present branch is the only one that matters for portuale.

**If `all_available`:**

1. **`all_installed`**: every `Atom(atom.cp)` matches in vardb. Virtuals are exempt.

2. **`all_installed_slots`**: `all_installed`, and every slot atom in `slot_map` matches in vardb. Virtuals are exempt.

3. `conflict_downgrade or installed_downgrade` → `other`.

4. **`all_in_graph`**: every non-blocker, non-virtual atom has some graph-db match that is actually `in graph`.

5. A circular atom → 

   ```
   other
   ```

   . That is either:

   - an `--onlydeps` parent self-dep, or
   - a hit in the `circular_dependency` map filled by backtracking.

6. Otherwise:

   - USE satisfied:
     - `all_in_graph` → bin 0
     - `all_installed` → bin 0
     - else → bin 1
   - USE unsatisfied:
     - `not all_use_unmasked` → `other`
     - `all_in_graph` → bin 2
     - `all_installed_slots` → bin 3 (**slots**, not plain `all_installed`)
     - else → bin 4

**If not `all_available`:**

- Every non-blocker atom matches vardb exactly → `all_installed_slots = True`, bin 5.
- Some match → bin 6.
- Any `Atom(cp)` matches vardb (fuzzy match, bug 522652) → bin 7.
- Otherwise → bin 8 (`other`).

### 1.5 In-bin reorder (each bin with ≥ 2 choices)

1. If `minimize_slots`, stable-sort by `new_slot_count`.

2. Outer loop over the snapshot 

   ```
   choice_1 in choices[1:]
   ```

   . Inner loop over the

   live

    list 

   ```
   choice_2 in choices
   ```

   , stopping when 

   ```
   choice_2 is choice_1
   ```

   . Promote 

   ```
   choice_1
   ```

    in front of 

   ```
   choice_2
   ```

   , then 

   ```
   break
   ```

   , in either case:

   - `c1.all_installed_slots and not c2.all_installed_slots and not c2.want_update`;
   - over the cps shared by both `cp_map`s (using `vercmp`), either `has_upgrade and not has_downgrade`, or `c1.all_in_graph and not c2.all_in_graph and not (has_downgrade and not has_upgrade)`.

### 1.6 Return rule

```python
for allow_masked in (False, True):
    for choices in choice_bins:
        for choice in choices:
            if choice.all_available or allow_masked:
                return choice.atoms
```

On pass 1, an `all_available` choice filed in **bin 8** (downgrade, circular, or masked-USE) **wins over** non-available choices in bins 5–7. Bins 5–7 only matter on pass 2.

------

## 2. Current portuale state

### 2.1 What is modelled

`AltPreference { Unsatisfiable < Available < Installed }`, in `rust/portage-use-reduce/src/lib.rs` (~702).

- `Installed` = bin 0: all installed at the cp level (`atom_cp_installed`) OR all in graph (`atoms_all_in_graph`).
- `Available` = bin 1.
- `resolve_disjunctions` keeps the first alternative at the best rank and **breaks early on `Installed`**.
- If every alternative is `Unsatisfiable`, it keeps the literal `||` group (the "never silently drop a dep" invariant).
- Backtracking feedback (`runtime_pkg_mask`) already reaches the probe through `slot_constraints` / `disj_constraints`.

### 2.2 Code sites

**Rust**

- ```
  portage-use-reduce/src/lib.rs
  ```

  - `AltPreference`, `use_reduce_flat_disjunctive`, `resolve_disjunctions`.
  - Unit tests `disjunctive_*` (~972–1085).

- ```
  portage-repo/src/lib.rs
  ```

  - Probe closure in `backtracking_resolve` (~16135), for the New/Upgrade walk.
  - A near-duplicate probe closure in `enqueue_dependencies` (~17197), for the AlreadyInstalled recursion.
  - Helpers:
    - `atom_currently_satisfiable` (~7968); this **includes the USE-dep check**;
    - `atom_cp_installed` (~8420);
    - `atoms_all_in_graph` (~8446);
    - `candidate_iuse_and_use` (~7688);
    - `forced_or_masked_flags` (~2953);
    - `installed_versions` (~5086);
    - `installed_pkg_iuse_and_use` (~5353).
  - `root_deps_satisfied_atoms` / `unsatisfied_root_deps_atoms` (~9129+) use a binary Available/Unsatisfiable probe. **Leave them as they are** unless slice 0 decides otherwise.
  - Grep `use_reduce_flat_disjunctive` for every call site.

**Python mirror** (`python/emerge_pretend_reference.py`)

- `_resolve_disjunctions` (~5592), `_use_reduce_flat_disjunctive` (~5664).
- `_disj_pref` closures (~12194, ~12969).
- Root-deps sites (~6007, ~6052).
- Helpers `_atom_currently_satisfiable` (~5694), `_atom_cp_installed` (~5907), `_atoms_all_in_graph` (~5936).

**Existing contract tests** (`tests/test_emerge_pretend_contract.py`)

- `test_any_of_group_prefers_the_installed_alternative`
- `test_or_group_prefers_a_branch_already_in_the_graph`
- `test_or_group_installed_preference_skips_a_required_use_broken_first_alternative`
- `test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`
- `test_or_group_alternative_yields_to_the_next_on_a_missing_transitive_dep`
- `test_any_of_group_falls_back_to_every_alternative_when_none_satisfiable` (**expected to change** in slices 3–4)
- `test_root_deps_disjunctive_branch_selection_matches_between_implementations`

### 2.3 Known divergences from real (the work)

| #    | Divergence                      | Real                                                         | portuale today             | Slice      |
| ---- | ------------------------------- | ------------------------------------------------------------ | -------------------------- | ---------- |
| D1   | No in-bin reorder; early break  | upgrade / in-graph / installed-slots promotion               | first-listed at best rank  | 2          |
| D2   | Bin 0 split                     | installed-slots vs any-slot distinction feeds the reorder    | cp-level only              | 2          |
| D3   | USE-unsatisfied alternatives    | bins 2–4, or `other` if masked/forced                        | `Unsatisfiable`, discarded | 3          |
| D4   | Masked alternatives, pass 2     | bins 5–7 + `allow_masked`                                    | literal `||` fallback      | 4          |
| D5   | Circular self-dep               | `other` with `all_available=True`, still returnable on pass 1 | `Unsatisfiable`            | 4          |
| D6   | Downgrade guards                | `conflict_downgrade` / `installed_downgrade` → `other`       | absent                     | 5          |
| D7   | `minimize_slots` (DNF overlap)  | sort by `new_slot_count`                                     | absent                     | 5          |
| D8   | Nested all-of alternative atoms | only unsatisfied atoms count                                 | all flattened atoms count  | 0 (decide) |

The real-tree evidence that bins matter is `TEST/findings/l0.md` cluster R. Before the `all_in_graph` fix, `kdecore-meta` resolved with 53 extra packages and spurious autounmask advice. Bin choice changes **autounmask output**, not just which package is picked.

------

## 3. Traps (read before writing any code)

1. **The bin-0 aliasing.** Do not create three separate bins for in-graph, installed, and any-slot.
2. **`unsat_use_installed` tests `all_installed_slots`**, not `all_installed`.
3. **The pass-1 / pass-2 rule.** An `all_available` choice in `other` beats non-available choices in bins 5–7 (§1.6).
4. **The bug 600346 `continue`** skips only the `cp_map` update.
5. **Snapshot vs live iteration** in the reorder (`choices[1:]` vs `choices`). Port the exact semantics and cover them with a 3-alternative fixture.
6. **Removing the early `break`** on `Installed` is required for D1. Check the cost at real-tree scale (`bench/`).
7. **`atom_currently_satisfiable` mixes two things**: availability without USE, and USE satisfaction. Real keeps them separate (`atom.without_use`, then `atom`). Slice 1 must split them.
8. **Nested alternatives (D8).** Real ranks an all-of alternative by its *unsatisfied* atoms only. Do not change this silently. Slice 0 decides whether to port it or document it as a cut.
9. **Determinism.** `new_slot_count` depends on graph insertion order (real's own comment admits variance). portuale is deterministic by design, so any port must use portuale's deterministic BFS order and document it.
10. **The "never silently wrong" invariant.** Picking a USE-unsatisfied or masked branch instead of the literal `||` fallback must surface as autounmask / masked disclosure. It must never become a silently merged package with wrong USE.
11. **Crate boundary.** `portage-use-reduce` keeps tokens as opaque strings. Version comparison and cp/slot maps belong in `portage-repo` or behind a callback. Do not pull `portage-dep` / `portage-versions` into `portage-use-reduce` without owner sign-off.

------

## 4. Slice plan

Every slice follows AGENTS.md steps 1–10:

- Rust and Python in lockstep, in one commit.
- Hand-written fixtures (check name collisions first; suggested prefix `dev-libs/zap*`).
- A `CASES` entry plus a pinned-output contract test plus a Rust unit test.
- A `what-this-proves.md` paragraph and a `scope-backlog.md` update.
- The full verification pass.
- No commit or push unless explicitly asked.

**Resolver change ⇒ run `TEST/run/l0-resolver.sh` at the end of slices 2, 3 and 4.** Baseline at the time of writing: 96/120 clean. A slice must not lower that number without an adjudicated entry in `TEST/compare/known-divergences.yaml`.

Wherever the container allows it, verify each new fixture's expected output against **real `emerge -p`** (3.0.82.2). Pin real's output, not your reading of the source.

A contract test may be flipped **only** with a real-Portage justification cited in the docstring and in `what-this-proves.md` (precedent: the 2026-09-07 `preferred_installed` slice). Never flip a test just to make a slice pass.

### Slice 0 — Design and grounding (doc only, no behaviour change)

**Deliverables.** Write `docs/history/dep-zapdeps-bins-design.md` containing:

- the §0 backlog-pointer fixes, applied to `backlog-tasks.md`;
- a mapping table from each real input to its portuale equivalent or an explicit cut:
  - `mydbapi.match_pkgs(atom.without_use)`
  - `pkg_use_enabled` / `use.mask` / `use.force`
  - `vardb.match` (exact, slot, cp)
  - `graph_db` / `graph` (`entries` + `merge_bound_cpv`)
  - `will_replace_child`
  - `want_update_pkg`
  - `downgrade_probe`
  - `circular_dependency`
  - `_expand_new_virtuals`
  - `_overlap_dnf` / `minimize_slots`
  - `dep_wordreduce` / the early exit
- an API decision. Option A: the probe returns an `AltFacts` struct and selection moves into `portage-repo`. Option B: `portage-use-reduce` receives a bin-classifier plus a comparator callback.
- a decision on D8;
- a per-slice fixture list, one fixture per bin rule;
- a list of existing tests expected to flip, with the real behaviour that justifies each.

**Acceptance.** Owner has approved the API option, the D8 decision, and the list of cuts.

**Stop and ask** on anything in the mapping table marked "no analogue".

### Slice 1 — Refactor (zero behaviour change)

**Scope.**

- Deduplicate the two `portage-repo` probe closures into one helper, and do the same in Python.
- Split `atom_currently_satisfiable` into an availability check and a USE check, keeping a wrapper so other callers are unchanged.
- Introduce `AltFacts` (or the option chosen in slice 0) that still yields exactly today's three ranks.

**Acceptance.**

- Whole test suite green with **zero** test edits.
- `emerge --pretend --json` output byte-identical to before on every fixture. Diff the old and new binaries across `CASES`.

### Slice 2 — Bin 0/1 fidelity and in-bin reorder (D1, D2)

**Scope.**

- Add `all_installed_slots`, `want_update` (or its documented stand-in), and `cp_map` with the bug 600346 rule.
- Port the §1.5 reorder without `minimize_slots`, and remove the early break.

**Fixtures (each isolating one rule).**

- Slot upgrade: `|| ( foo:1 foo:2 )`, both installed, ascending order; real picks `foo:2`.
- In-graph over installed-only when the installed-only branch is listed first.
- Installed-slots over any-slot.
- A 3-alternative case exercising the snapshot/live iteration.

**Acceptance.** Fixtures match real `emerge -p`, and L0 is not lower than baseline.

### Slice 3 — `unsat_use_*` and full `all_use_satisfied` (D3)

**Scope.**

- Classify USE-unsatisfied alternatives into bins 2–4.
- Implement the `all_use_unmasked` check through `violated_conditionals` semantics plus `forced_or_masked_flags`.
- Verify the interaction with the in-loop USE autounmask: which branch now gets the "USE changes are necessary" advice.

**Fixtures.**

- `|| ( foo[a] foo[b] )` ordering relative to `preferred_non_installed`.
- A masked-flag branch demoted to `other`.
- A forced-flag branch demoted to `other`.
- An installed-slots USE-unsatisfied branch beating a non-installed one.

**Acceptance.** Real's autounmask advice reproduced byte-for-byte on the fixtures, and L0 not lower than baseline.

**Stop and ask** if the literal-`||` fallback contract test must change beyond what slice 0 predicted.

### Slice 4 — `other_*` bins, the `allow_masked` pass, circular-as-`other` (D4, D5)

**Scope.**

- Add bins 5–8 and the two-pass return.
- Stop treating the circular self-dep as `Unsatisfiable`: file it in `other` as available.
- Check against #20 (masked-dependency disclosure): a masked-installed branch chosen on pass 2 must produce real's "All ebuilds … masked" block, not a merge.

**Fixtures.**

- Masked but exactly installed.
- Masked with some atoms installed.
- Masked and installed only at the cp level (bug 522652 shape).
- Circular-only available alternative versus a masked alternative.

**Acceptance.**

- Fixtures match real.
- `test_any_of_group_falls_back_to_every_alternative_when_none_satisfiable` is re-pinned to real behaviour, with justification.
- L0 not lower than baseline.

### Slice 5 — Downgrade guards and `minimize_slots` (D6, D7)

**Scope.** Port `conflict_downgrade` / `installed_downgrade` where portuale's graph state can express them. For `minimize_slots`, first decide whether portuale ever produces the `_overlap_dnf` shape.

**Expected outcome.** Partly documented cuts. Record each cut in `scope-backlog.md` §A with its real-source citation.

**Stop and ask** before approximating `downgrade_probe` or `want_update_pkg` with anything that is not a faithful port.

------

## 5. Model routing

| Work                                                         | Tier                               | Why                                                          |
| ------------------------------------------------------------ | ---------------------------------- | ------------------------------------------------------------ |
| Slice 0 design, API decision, cut list                       | **Frontier** (Opus-class or above) | Must reason jointly over real semantics, portuale's BFS model and the backtrack loop, and know when to stop and ask. |
| Slice 3 implementation                                       | **Frontier**                       | USE mask/force semantics plus autounmask interaction; highest risk of silent wrong resolution. |
| Slice 5 decisions                                            | **Frontier**                       | Mostly deciding what is faithful versus what must be cut.    |
| Review of every slice diff before completion                 | **Frontier**                       | Catches fixtures that pass without isolating the rule, and silently chosen defaults. |
| Slices 1, 2, 4 implementation (after slice 0 approval)       | **Mid-tier** (Sonnet-class)        | Well-specified by this brief plus the design doc, and guarded by the contract suite. |
| Python-mirror lockstep of an approved Rust change            | **Mid-tier**                       | Mechanical but needs care across a ~21k-line file.           |
| Closure dedup, md5-cache fixture boilerplate from an exact spec, doc paragraphs | **Small** (Haiku-class)            | Low ambiguity, fully checked by the verification pass.       |

Hand-off rule: a cheaper model **never** makes a decision that §4 marks as "stop and ask". It escalates instead.

------

## 6. Per-slice verification checklist

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --release --all-targets` (zero warnings)
- [ ] `cargo test --release` (whole workspace)
- [ ] `python3 -m pytest tests -q` (whole suite)
- [ ] Rust vs Python run empirically on the new fixtures and diffed (not just via pytest)
- [ ] New fixture outputs checked against real `emerge -p` where the container allows
- [ ] Every flipped test justified in its docstring and in `what-this-proves.md`
- [ ] `TEST/run/l0-resolver.sh` (slices 2–4) is not lower than baseline, or the divergence is adjudicated
- [ ] `bench/` shows no significant regression (removing the early break costs probes)
- [ ] `what-this-proves.md` paragraph appended; `scope-backlog.md` §A / Part 4 updated
- [ ] No commit or push unless explicitly requested

## 7. Out of scope

- `--solver=pubgrub|resolvo` `||` handling (see #33/#34).
- The root-deps binary probes, unless slice 0 decides otherwise.
- Any config-writing autounmask (Part 3 non-goal).
- Non-deterministic behaviour copied for its own sake (Part 3 precedent).