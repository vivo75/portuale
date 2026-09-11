# Task 22 — `dep_zapdeps` finer choice bins: agent brief

Backlog item #22 (`docs/backlog-tasks.md`, resolver section [A]; still open in `docs/scope-backlog.md` Part 4). This brief supersedes `docs/022_dep_zapdeps.{chatgpt,claude}.md` and `docs/022-agent-task-22-zapdeps.{chatgpt,claude}.md`; it keeps what was verified against the code and drops the rest.

**Read `AGENTS.md` and `docs/agent-context.md` first.** This file adds only what is specific to #22. Line numbers drift: re-locate everything by function name before trusting a claim here (AGENTS.md step 1).

------

## 0. Fix the backlog line before starting

The one-liner has two stale pointers. Correct them in `docs/backlog-tasks.md` as part of slice 0:

- Real `dep_zapdeps` is in **`lib/portage/dep/dep_check.py`**, not `depgraph.py` (which only mentions it in comments). It is called from `dep_check()` in the same file.
- `rust/portage-repo/src/solver_bridge.rs` is **out of scope**. It only walks `DepEntry::AnyOf` to over-approximate the closure; pubgrub/resolvo choose the branch. That is #34, not #22.

------

## 1. Current state in portuale

- `rust/portage-use-reduce/src/lib.rs`: `AltPreference { Unsatisfiable, Available, Installed }` and `resolve_disjunctions()`. The crate treats tokens as opaque strings; the caller passes a probe closure `&[String] -> AltPreference`. The loop keeps the first alternative at the best rank and **`break`s early on `Installed`** — this is what makes any in-bin ordering impossible today.
- `rust/portage-repo/src/lib.rs`: two nearly identical probe closures (the main New/Upgrade walk and `enqueue_dependencies`). Each computes `all_available` via `atom_currently_satisfiable()` — which **already checks USE deps**, so a USE-unsatisfied alternative is ranked `Unsatisfiable` instead of landing in an `unsat_use_*` bin. `Installed` is `all cp installed || atoms_all_in_graph`, i.e. real's aliased bin 0.
- `python/emerge_pretend_reference.py`: `_resolve_disjunctions()` mirrors the same three ranks. **Every change below lands there in lockstep.**
- `tests/test_emerge_pretend_contract.py`: ~430 cases comparing Rust to the Python mirror. This proves Rust == mirror, **not** Rust == real Portage. Real parity is only checked by `TEST/run/l0-resolver.sh` in a container.

------

## 2. Ground truth: real `dep_zapdeps` (verify in `dep_check.py`)

**Eight bins, not nine.** `preferred_installed` and `preferred_any_slot` are the *same list object* as `preferred_in_graph`. `choice_bins`, in order:

```
preferred_in_graph          (== preferred_installed == preferred_any_slot)
preferred_non_installed
unsat_use_in_graph
unsat_use_installed
unsat_use_non_installed
other_installed
other_installed_some
other_installed_any_slot
other
```

**Three independent facts per alternative** (the part a weak model collapses into one boolean):

- `all_available` — every non-blocker atom matches a *visible* package, **ignoring USE deps**.
- `all_use_satisfied` — those matches also satisfy the atom's USE deps.
- `all_use_unmasked` — where USE deps are unsatisfied, the flags that would need changing are not in `use.mask` / `use.force` (bug 515584).

**Classification shape** (with `graph_db` present, which is portuale's case):

```
if not all_available:
    all_installed          → other_installed
    some_installed         → other_installed_some
    any cp installed       → other_installed_any_slot   (bug 522652, cp-level)
    else                   → other
elif conflict_downgrade or installed_downgrade or circular_atom:
    → other
elif all_use_satisfied:
    all_in_graph           → preferred_in_graph
    all_installed          → preferred_in_graph          (aliased)
    else                   → preferred_non_installed
else:
    not all_use_unmasked   → other
    all_in_graph           → unsat_use_in_graph
    all_installed_slots    → unsat_use_installed         (slots, not cp)
    else                   → unsat_use_non_installed
```

`all_installed` is cp-level; `all_installed_slots` additionally requires the `slot_map` atoms to match installed packages. Virtuals count as installed (zero cost).

**In-bin ordering** runs separately inside each bin (so bins never interleave):

1. if `minimize_slots`: stable sort by `new_slot_count`;
2. promote `choice_1` ahead of `choice_2` when `choice_1.all_installed_slots and not choice_2.all_installed_slots and not choice_2.want_update`;
3. otherwise promote on `vercmp` over the intersecting `cp_map`: upgrade and no downgrade wins; or `all_in_graph` over not-in-graph unless that would eliminate an upgrade.

`minimize_slots` is `True` only when `_overlap_dnf` rewrote the structure into DNF (overlapping `||` groups on the same cp). It is **not** a depclean flag, although the upstream comment says depclean is its main beneficiary.

**Two-pass return:**

```
for allow_masked in (False, True):
    for bin in choice_bins:
        for choice in bin:
            if choice.all_available or allow_masked: return choice.atoms
```

Consequence: every `other_*` choice has `all_available == False`, so the whole `other_*` family is only ever returned in the second pass. Its effect is on the failure path — what autounmask and masked-dependency disclosure (#20) see — never on a healthy `@world`.

**Inputs portuale does not have:** `want_update_pkg`, `downgrade_probe`, `circular_dependency`, `will_replace_child`, and a `graph_db` that mutates during the pass. Each must be mapped to a portuale equivalent or recorded as a documented cut. That decision is the owner's (AGENTS.md rule 3).

------

## 3. Design rules (non-negotiable)

1. **Facts first, bins second.** Do not add variants to `AltPreference` until the probe returns a struct of independent facts. A richer enum on top of today's `atom_currently_satisfiable()` produces bins that are structurally present and semantically wrong.
2. **Keep `portage-use-reduce` atom-agnostic.** Candidate analysis and any `vercmp` live in `portage-repo`. Either the probe returns a rich struct plus a comparator, or branch selection moves into `portage-repo` and the crate only enumerates alternatives. Decide in slice 0; don't drift.
3. **One probe closure.** Merge the two duplicated closures into a helper before adding facts to them.
4. **Preserve the literal-`||` fallback** for the "nothing available at all" case. Picking a branch where today portuale keeps the group changes what autounmask and #20 see; that change is deliberate in slice 3, not a side effect of slice 1.
5. **A fixture that passes without isolating one rule is worse than none.** Each fixture differs between alternatives in exactly one property.

------

## 4. Slices

Each slice: both language sides, contract `CASES` entry + pinned-output test, Rust unit test, `what-this-proves.md` paragraph, full verification pass (AGENTS.md step 8). Model tier is a recommendation for the *executor*; every diff is reviewed by a frontier model regardless.

### Slice 0 — design, no behaviour change — *frontier*

- Fix the two backlog pointers (§0).
- Produce `docs/022-design.md`: a table mapping every real input (§2) to a portuale source or an explicit cut, with rationale.
- Choose the API shape (rule 2) and get owner sign-off via `AskUserQuestion`.
- Deliverable is a doc and a decision, no code.

### Slice 1 — refactor to facts — *mid-tier*

- Merge the two probe closures.
- Probe returns a struct: `all_available` (USE-free), `all_use_satisfied`, `all_installed` (cp), `all_installed_slots`, `all_in_graph`, `cp_map` (cp → chosen version), `slot_map`.
- Derive today's three ranks from the struct so that **all contract tests pass unchanged** and `fixtures/` output diffs empty between Rust and Python.
- Acceptance: zero behavioural diff, `atom_currently_satisfiable` split into a USE-free availability check plus a USE check.

### Slice 2 — in-bin ordering — *mid-tier*

- Remove the early `break`. Rank every alternative, then apply §2 ordering rules 2–3 inside the best bin (rule 1 / `minimize_slots` only if slice 0 mapped DNF detection; otherwise cut and document).
- Three fixtures, one rule each: `|| ( foo:1 foo:2 )` both installed → picks `foo:2`; in-graph over installed-only; all-installed-slots over any-slot.
- This is the only slice with user-visible effect on a healthy system; do it before slice 3 — it has no dependency on the USE machinery and forces the API decision early.

### Slice 3 — `unsat_use_*` bins — *frontier*

- Add `all_use_unmasked` (read `use.mask` / `use.force` for the candidate's profile).
- Classify per §2. USE-unsatisfied-but-unmasked alternatives now select a branch instead of falling back to the literal group.
- Fixtures: `|| ( foo[a] foo[b] )` with neither flag on; same with `a` masked; verify `unsat_use_*` sorts **after** `preferred_non_installed`.
- Riskiest slice: it changes the fallback (rule 4) and interacts with the in-loop autounmask and the `'backtrack` loop. Expect existing contract cases to flip; each flip needs a written justification against real.

### Slice 4 — `other_*` bins and the `allow_masked` pass — *mid-tier*

- Implement the `not all_available` branch and the two-pass return.
- Fixtures: masked-but-installed; partly installed; installed at cp level only (bug 522652).
- Verify against #20's masked-dependency disclosure; the output should match real's choice of which masked atom gets reported.

### Slice 5 — downgrade guards, `minimize_slots`, `circular_atom` — *frontier for the cuts, mid-tier for the code*

- Only what slice 0 mapped. Everything else becomes a documented cut in `scope-backlog.md`. Note that `conflict_downgrade` / `installed_downgrade` belong with the slot-conflict work (#23/#24) if their inputs turn out to need backtracking state.

------

## 5. Traps (each has already fooled a model)

- Counting nine bins and appending to `preferred_installed` / `preferred_any_slot` as separate lists.
- Reusing `atom_currently_satisfiable()` for `all_available`.
- `unsat_use_installed` keys on `all_installed_slots`, not `all_installed`.
- The bug-600346 `continue` skips only the `cp_map` update, not the `slot_map` update.
- Nested all-of alternatives contribute only their *unsatisfied* atoms to the `||` choice.
- Writing the Rust side and forgetting `_resolve_disjunctions` in the mirror.
- A contract fixture that passes because both implementations share the same wrong reading of real. Real parity comes only from L0.

------

## 6. Verification beyond the standard pass

After slices 2, 3 and 4, and before the task is called done:

- Add synthetic ebuilds to `TEST/images/overlay/porttest/` for the four behaviours that diverge from today: upgrade promotion, USE-unsat-unmasked, USE-unsat-masked, `allow_masked` second pass.
- Run `TEST/run/l0-resolver.sh` on the full atom list plus those probes and diff against real `emerge -p`. No new divergences; note which of the existing L0 divergences (if any) close.

------

## 7. Out of scope

`solver_bridge.rs` (#34); full backtracking; `_slot_conflict_backtrack` mask-target analysis (#23); slot-operator rebuild undo (#24). If a slice seems to need one of these, stop and surface it.