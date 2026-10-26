# Task 8 — premise contradictions (§P), written 2026-10-26

Source: `oracle.md` (Portage 3.0.83; D = `lib/_emerge/depgraph.py`, B =
`lib/_emerge/resolver/backtracking.py`, DC = `lib/portage/dep/dep_check.py`,
PT = `lib/_emerge/resolver/package_tracker.py`). The manager re-checked
D:7910, DC:379-380, D:5691-5696, PT:230-252, D:8704-8710 and D:2266-2280 by
hand, and the swap-noslotop captures.

1. **The candidate mechanism was the second hop, not the first.** Task 8
   named "the missing-dependency path in `_add_dep` (D:3460-3510)" as the
   likely mechanism. In every heal and keeper trace the first mask comes
   from `_slot_confict_backtrack` (D:2220-2304, "backtracking due to slot
   conflict"). The missing-dependency mask on the puller (D:3517 → B:197-206)
   is the second hop, and portuale already has it (`step_missing_dep`).
2. **`dep_zapdeps` is not "installed-preferred" for these consumers.** The
   code-notes of both heuristics say Portage resolves each `||` group
   installed-preferred. For the installed reverse-dep consumers that carry
   the pins, Portage evaluates the `||` in complete mode against
   `_graph_trees` (D:8705 → D:6009-6016), whose package tracker hides an
   installed package replaced by a same-slot merge node (PT:230-252). So the
   installed-matching arm is usually *unavailable*, and the first-listed
   arm in its bin wins (live probe `captures/which-trees-swap-noslotop.txt`).
   In the normal walk the downgrade guards (DC:480-490, DC:531-541) can pick
   a non-installed arm (R4, bug 531656). Discriminator `swap-noslotop`:
   Portage merges `p569prov-5.44` + `p569pull-2` with 0/20 backtracks, while
   portuale heals to `p569pull-1` (merge list differs).
3. **G1's mechanism differs; its outcome matches.** Portage does not refuse
   to mask: it backtracks over the slot conflict, every choice dead-ends on
   the argument (D:5613-5616), and the final `get_best_run` pass
   (B:287-298, D:12420-12439) prints the residual. Same rows; backtrack
   counts 3/20, 4/20, 3/20 (k3 argument, k3 needer, igw) against portuale's
   0, 1, 0.
4. **Line drift.** The plan's `backtracking.py:237-243` is B:237-240
   (`slot_operator_mask_built`) plus B:241-242
   (`slot_operator_replace_installed`); B:197-205 is B:197-206.

Effect on the plan: Task 8's own job was to find the real mechanism, so the
contradiction is within its remit. Tasks 9–10 are rewritten from
`design.md`; the owner rulings in §0 ("full port with stop conditions",
heuristics retired one by one) still hold. The rewrite goes to the external
re-review before any resolver code (§P).
