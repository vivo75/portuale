# crates/portage-world/src/lib.rs
First-pass walk of `@world`: per-slot best ebuilds, dependency atoms, and
the slot sets the reorder pass ranks.

## fn narrow_slotless (#739, #742)
- A slotless atom is one requirement, not one per slot. In check mode
  (`Options::rebuilds == false`: `mrg --check-world`) the atom is met by the
  visible slots already installed, else by the best slot alone; an installed
  slot satisfies the atom, so the check stays silent about newer slots.
- In plan modes (`rebuilds == true`: `mrg --plan-world`, including
  `=emptytree`) the same atom reaches the best visible slot only (the order
  of `rank`: visible first, then highest version, then repo priority), even
  when an older slot is installed. An older installed slot is listed only
  when another atom reaches it (an atom naming that slot, or a dependency).
  Grounded in charter property 3 (newest visible version every constraint
  allows): on this host the resolver's `-pvuDN @world` plan adds
  `app-emulation/wine-staging:11.21` as new while `:11.20` is installed, and
  the fast plan missed it before this rule; with the rule the cross-check
  `world-crosscheck.py` resolver-only row is gone and `--check-world` output
  is byte-identical.
- The same narrowing answers #742 (`mrg --plan-world=emptytree` listed both
  installed slots of `dev-libs/dualslotpkg`, the resolver's `-pe @world`
  only `:2`): one rule for what an atom without a slot asks for, in the plan
  and in `-e`. Which slots an atom reaches is mode-dependent, so `World` has
  no per-mode record of it; `World::reached` holds the first pass's slots.
- `Memos::newest_slot` carries the mode (set from `Options::rebuilds` in
  `walk`); `narrow_slotless` itself stays one branch: in plan mode the
  installed set is ignored and the existing keep-the-top fallback does the
  work. A bare atom whose slots are all uninstalled still lists exactly the
  best slot (`nothing_installed_takes_the_best_slot_only` covers both modes).
- Judged divergence (not a defect): dep-reached slotless atoms now also
  sprout their newest slot, which the resolver does not always do (measured
  on this host: `dev-lang/rust-bin:1.99.0` and `media-libs/gexiv2:0.16` are
  fast-only after the change). Direct probes (`emerge -pv =rust-bin-1.99.0`,
  `=gexiv2-0.16.2-r1`) merge both cleanly, so no constraint is broken and
  property 3 supports listing them; the plan output contract already discloses
  that the fast plan can list more than the resolver. The owner confirmed the
  rule (plan 06.739 Q1).

## fn branch_preference (#740, residue of plan 02.692)
- Portage's terms (`dep_check.py`, `dep_zapdeps` choice bins): a branch whose
  `cat/pkg`s are all installed (`Installed`) beats one that is merely
  available (`Available`); a branch with an atom no visible ebuild satisfies
  is `Unsatisfiable` and never chosen. Check mode keeps exactly this order.
- Plan-mode extension, portuale's own (charter property 3, not Portage): a
  branch every non-blocker atom of which the best visible candidate of its
  slot matches outranks a merely installed one. No slot named: the atom must
  match the package's best visible; a slot named: the best visible in that
  main slot, never a parallel slot (`w740prov:1` is best-visible with
  `w740prov-2:2` present). Order, best last: `Unsatisfiable` < `Available` <
  `Installed` < `BestVisibleAvailable` < `BestVisibleInstalled` (derived `Ord`;
  the two new variants are declared after `Installed`). The installed bit is
  the tie-break (selenium `|| ( firefox firefox-bin )`: both best-visible,
  `firefox-bin` installed, wins); an installed arm that is not best-visible
  (host `openssl:0/3`, slot 0's best is 4.1.0 sub-slot `0/4`) loses to a
  best-visible uninstalled one (`openssl-compat:3`).
- Gated on `Memos::newest_slot` (set from `Options::rebuilds`, the same flag
  as `narrow_slotless`); memoised per branch string (`Memo<AltPreference>`)
  because the rank scans every atom's candidates. Determinism: `rank` plus the
  listed branch order, `tie_break` unchanged.
- The arm alone is not the whole mechanism: taking arm 2 adds the requester
  `dev-libs/openssl:0/3`, and only then does the reorder pass (`chosen()` is
  the first visible version satisfying every requester) keep 3.6.5 although
  kept `app-admin/sudo`'s `openssl:=` best is 4.1.0. S0 capture
  (`docs/evidence/02.740/requesters.stderr`): arm `[openssl-compat:3]` ->
  `Available`, arm `[openssl:0/3]` -> `Installed`; `dev-libs/openssl:0`
  `chosen=3.6.5` with `openssl:0/3` requested by exactly the four
  `openssl-compat` consumers and `openssl:=` by sudo et al.
- Pinning tests: `a_plan_prefers_the_branch_that_admits_the_best_version`,
  `the_same_branch_choice_in_check_mode_keeps_the_installed_one`,
  `a_best_visible_tie_prefers_the_installed_branch`,
  `a_stale_installed_arm_loses_to_a_best_visible_uninstalled_one`,
  `a_parallel_slot_arm_is_judged_against_its_own_slot`; end to end
  `portuale::plan_world::the_fast_plan_takes_the_newest_arm_of_a_disjunctive_dependency`
  and pmtest `test_740_the_plan_takes_the_newest_arm`.
