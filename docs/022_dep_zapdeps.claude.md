Let me look at the backlog document.The numbered backlog (the "#19" reference) seems to live in agent-context.md; let me find item 22.Nothing numbered "22" in the public docs pages, so let me clone the repo and search it directly.Found it in `docs/backlog-tasks.md`. Now let me look at the real `dep_zapdeps` and portuale's current `||` code to judge the gap.I cloned both portuale and upstream Portage and read the relevant code. The item is in `docs/backlog-tasks.md`, and `scope-backlog.md` Part 4 confirms it's still open.

## What the task actually is

Real `dep_zapdeps` (in `lib/portage/dep/dep_check.py`, ~480 lines) sorts every `||` alternative into nine choice bins, reorders candidates inside each bin, and returns the first entry of the best non-empty bin. It makes a second pass that allows masked alternatives.

Portuale's version is `AltPreference` in `portage-use-reduce`, with three ranks: `Installed`, `Available` and `Unsatisfiable`. It picks the first alternative at the best rank and stops early on `Installed`. That covers the common case; the 2026-09-07 `virtual/wine` fix was exactly this. The gaps are real, though:

- **USE-unsatisfied alternatives are discarded.** `atom_currently_satisfiable` checks USE deps, so an alternative like `foo[x]` with `x` off is ranked `Unsatisfiable`. Real Portage still considers it, in the `unsat_use_*` bins after `preferred_non_installed`. It also prefers alternatives whose missing flags aren't in `use.mask`/`use.force` (bug 515584). When nothing is fully satisfied, real picks a branch and autounmask suggests USE changes. Portuale falls back to keeping the literal `||` group.
- **There are no `other_\*` bins and no `allow_masked` pass.** Real prefers masked-but-installed alternatives, then partly installed ones, then installed at the cp level (bug 522652).
- **There is no ordering inside a bin.** Real promotes alternatives with upgrades, so `|| ( foo:1 foo:2 )` picks `foo:2` when both are installed. It also promotes all-in-graph alternatives over installed-only ones, and all-installed-slots over any-slot. Portuale's early `break` makes this impossible. This is the one part that can affect ordinary `@world` updates, not just failure paths.
- **The downgrade guards are missing.** `conflict_downgrade` (bug 531656) and `installed_downgrade` push a choice to `other`. `minimize_slots` for depclean is also missing.

Two pointers in the backlog line are stale, and I'd fix them before handing the task to any model. It cites "`depgraph.py` `_dep_zapdeps`", but the function is in `dep_check.py`. It also says the task affects "`solver_bridge.rs` `||` closures", but the bridge only walks `AnyOf` to over-approximate the closure and lets pubgrub/resolvo choose. Porting zapdeps there is a different problem, tied to #34. More generally, `backlog-tasks.md` is out of date: items 2–4, 7–14, 18, 21 and 28 are marked shipped in `scope-backlog.md`.

## How difficult

I'd call it upper-medium: harder than a Tier-1 slice, easier than #19 or #24. The algorithm is fully specified in source and isn't research. The difficulty is in four places:

1. **Real's inputs don't map cleanly onto portuale's state.** Real uses `graph_db`, `want_update_pkg`, `downgrade_probe`, `will_replace_child`, `circular_dependency`, and a graph that changes during the pass. Portuale runs a single deterministic BFS and flattens the result. Some bins will need documented cuts, and those decisions belong to the owner (AGENTS.md rule 3).
2. **The crate boundary.** `portage-use-reduce` deliberately treats tokens as opaque strings. In-bin sorting needs version comparisons over a cp→package map. Either the probe returns a rich struct plus a comparator, or the selection moves into `portage-repo`. That's an API decision.
3. **Subtle details that are easy to get wrong.** `preferred_installed`, `preferred_in_graph` and `preferred_any_slot` are the same list object in Python, so they form one bin, not three. The `continue` for bug 600346 skips only the `cp_map` update, not the `slot_map` update. The `unsat_use_*` bins must come after `preferred_non_installed` for `|| ( foo[a] foo[b] )`.
4. **Knock-on effects.** Picking a branch instead of keeping the literal `||` fallback changes how the in-loop autounmask and the #20 masked-disclosure path behave. Existing contract tests may flip, as three did last time. The work has to be done in the ~21k-line Python mirror in lockstep, and it's a resolver change, so an L0 run is mandatory.

On value: none of the current 24 L0 divergences are attributed to this item, so most of the payoff is edge-case parity. The in-bin ordering is the exception.

## How to split it

**Slice 0: design, no behaviour change.** Fix the stale pointers. Map each real input to a portuale equivalent or an explicit cut. Choose the API shape and get sign-off. Merge the two nearly identical Rust probe closures (the New/Upgrade walk and `enqueue_dependencies`) into one helper.

**Slice 1: refactor.** Make the probe return a struct (`all_available`, `all_use_satisfied`, `all_use_unmasked`, `all_installed`, `all_installed_slots`, `all_in_graph`, `cp_map`, …) while reproducing today's three ranks exactly. All ~430 contract tests must still pass unchanged.

**Slice 2: in-bin ordering.** Add upgrade promotion, in-graph over installed, and installed-slots over any-slot, and remove the early break. Use three fixtures, each isolating one rule. This is the best value for the effort.

**Slice 3: `unsat_use_\*` and full `all_use_satisfied`.** Include the `use.mask`/`use.force` check and verify how autounmask interacts with it. This is the riskiest slice.

**Slice 4: `other_\*` bins and the `allow_masked` pass.** Check it against #20's disclosure.

**Slice 5: downgrade guards and `minimize_slots`.** These will likely end up partly as documented cuts.

## Frontier or cheaper model?

Mixed. The repo's verification gate helps a lot: byte-identical Rust/Python contract tests, clippy with zero warnings, and L0. That gate makes a mid-tier model viable for execution. It doesn't catch a wrong design or a fixture that passes without actually isolating the behaviour, which AGENTS.md itself warns about.

- **Frontier model:** slice 0, slice 3, the cut decisions in slice 5, and reviewing every slice's diff. These need to hold real's semantics, portuale's BFS model and the backtrack loop in mind together, and to recognise when to stop and ask. A cheaper model tends to pick a default silently or port the aliased bins as three separate bins.
- **Mid-tier (Sonnet-class):** slices 1, 2 and 4, once the design exists, plus most of the Python mirroring.
- **Small/cheap model:** the closure dedup, md5-cache fixture boilerplate from a precise spec, and doc updates in `what-this-proves.md` and `scope-backlog.md`.

This code decides which package gets installed on real systems, and a wrong choice is silent. So I wouldn't economise on the design and review steps even if the implementation runs on a cheaper model.