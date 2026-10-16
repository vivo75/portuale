# crates/portage-repo/src/resolve/greedy_slots.rs

Greedy installed-slot arguments for the default solver (backlog #359 S3).

## fn greedy_slots_below_best

- Portage `lib/_emerge/depgraph.py`, `_greedy_slots` (the slot filter: top
  slot, then installed slots whose own best is lower). Shared by the default
  solver (string versions, `vercmp_ordering`) and the pubgrub bridge's
  `target_slots` (`portage_solver::Version`), which was refactored onto it
  in S3 with its `s6_target_slots_*` pins unchanged.
- The bridge's copy had no blocker lookahead (backlog #368, noted at
  `solver_bridge.rs:24`); the shared core covers the plain expansion only,
  and the bridge still does not look ahead.

## fn greedy_slot_atoms

- Portage `lib/_emerge/depgraph.py`, `select_files` (`if "--update" in
  myopts`: `_set_args`, greedy pass, `_set_args`, blocker-lookahead revision)
  and `_greedy_slots`. Only `AtomArg` participates, never set members, so
  `@world`/`@selected` members and anything in `argument_sets` is skipped;
  an `AtomArg` always names a category (`"/" in x`), hence the `/` gate.
- Installed slots come from the vdb match of the ORIGINAL atom (so a pinned
  `gs:2` or `=gs-2` trivially yields nothing: the installed `1.0:1` does not
  match), filtered to the same cp (`pkg.cp == highest_pkg.cp`); each slot's
  own best is selected for the bare `cp:slot` atom with no version range
  (`Atom(f"{highest_pkg.cp}:{slot}")`), exactly like Portage.
- The pool is the walk's own (ebuilds unless `--usepkgonly`, plus usable
  binaries), filtered by `is_visible` and the pass's `union_constraints`
  for the cp, so a backtrack mask moves the expansion: S3 recomputes every
  pass in `run_pass` (plan risk "re-run per attempt", `depgraph.py`'s
  "`select_files` per depgraph"). USE-dep narrowing and autounmask flips are
  deliberately NOT re-applied here; the queued seeds walk the normal path
  and narrow there.
- Installed-slot order is sorted low-first: Portage pops a hash set (pinned
  by `PYTHONHASHSEED` in the captures); the contract suite needs a stable
  seed order.
- The seeds ride `seed_queue` after the user args as depth-0, owner-less
  items (arguments). `(argument)` chain lines and world writes read the
  request atoms (`chain_arg_lines` filters `ctx.atoms`; world records
  `atom_args`), so the original text is carried automatically and no
  `cat/pkg:1` leaks into world (guarded by `test_portuale.py`'s
  dualslotpkg world test).
- Blocker-lookahead detail that bit once: `match_from_list` strips the
  leading `!` before matching, so blocker-ness must be read from the raw
  token (`starts_with('!')`); parsing the stripped form and asking for
  `blocker != None` is always false.
- The `pg359-gsblk` S0 cell (`s0-e-gsblk-pvu`) DOES discriminate the
  lookahead: pass 1 alone plans `U gsblk-1.1` with an unsatisfied block
  (rc 1); with the lookahead the output is byte-identical to Portage
  (`NS` + uninstall + `blocks b`, `Conflict: 1 block (all satisfied)`).
  The plan's conditional residue for a missing lookahead is therefore not filed (its number #394 was reused on 2026-10-16 for the unrelated `--rebuild-if-*` re-walk gap).
