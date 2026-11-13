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
