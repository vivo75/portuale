# #601 Task 0 — mechanism (2026-10-28)

Cell: `emerge -puvD @world` on the bed (world seeded with tmux, eix,
gentoolkit, blake3), Portage 3.0.83, portuale `df3bb523-dirty`.
Bed runs: `l0-20261028T215003Z` (portuale `--debug` on the MULTI cell) and
`l0-20261028T215114Z` (`@world` with `--debug` on both sides). A third,
container-only run used `PORTUALE_BT_TRACE=1`. Captures in `captures/`.

## Verdict: one missing atom (H1), display half still open

The final pass of both programs resolves the same 141 packages. The
difference is one atom in the force-reinstall family: Portage's
`forced reinstall atoms:` list (35) versus portuale's `BT replace-in` set (the
rebuild set, 34).

| | Portage | portuale |
|---|---|---|
| set size | 35 atoms | 34 |
| `dev-lang/perl:0` | **in** | **absent** |
| other 34 (`dev-perl/*`, `app-text/po4a`, …) | in | in (identical) |

`comm` of the two atom lists differs only in `dev-lang/perl:0`.
Portage's `r` on perl comes from `lib/_emerge/resolver/output.py:812-816` (a
force-reinstall arg matches the package, via `_iter_atoms_for_pkg`,
`depgraph.py:4988`) and its "causes rebuilds for:" block from
`_compute_abi_rebuild_info` (`depgraph.py:931-1145`), printed by
`_show_abi_rebuild_info` (`depgraph.py:1146-1159`), which starts from exactly
those atoms. So one missing atom is a sufficient explanation for both display
gaps in Portage.

Whether it is also portuale's cause is **not** established by this capture
(corrected 2026-10-28, see "Corrections after review"): the 34 measured here
are the trace's `slot_operator_replace_installed` (the rebuild set), while `r`
reads the separate `probe_replaced` set, and the `forced reinstall atoms:`
dump this section first relied on prints the `--reinstall-atoms` CLI option,
not an internal set.

Why perl is in Portage's set: `_slot_operator_update_backtrack`
(`depgraph.py:2415`) adds `_replace_installed_atom(child)` at `:2454-2462`,
`if new_child_slot is None and child.installed`; the child of the missed ABI
update (`dev-lang/perl-5.42.2:0/5.42=`) is the installed perl. (`:2404-2413`
is the slot-change twin that files an installed child as well.) portuale's
installed-parent probe files the same pair
(`slot_operator/installed_parent_update_probe.rs:189-190`; `:119-121` is the
unrelated `arg_pins_installed` gate), so **the open question for Task 0b is
why that filing does not reach the final `r` set or the pair list in this
cell** — probably `child_has_slot_update`, the perl-5.44 upgrade being already
scheduled, the pair scan's scope, or the filing landing in
`slot_operator_replace_installed` but not `probe_replaced`. Not established by
reading alone.

## The backtrack count (12/20 vs 1/20) — a different shape of search

- Portage, first pass: one perl slot conflict, then **133**
  `backtracking due to missed slot abi update` records (752
  `slot_operator_update_probe` blocks) all folded into **one** config
  change; `backtracking try 1`; the second pass settles
  (`captures/portage-first-abi-backtrack.txt`, `portage-backtrack-try-1.txt`).
- portuale (`PORTUALE_BT_TRACE=1`, 26 internal tries): try 1 `SlotConflict`
  on perl; tries 2-24 `MissingDep`, each masking **one** `virtual/perl-*`
  version (`virtual/perl-IO`, `-CPAN-Meta`, `-ExtUtils-ParseXS`, …) because
  its `=dev-lang/perl-5.44*` atom is unsatisfied; try 25 `Config`; try 26
  `Settle` with the 34-atom **rebuild** set (`captures/portuale-bt-trace.txt`;
  the trace prints `slot_operator_replace_installed`, not `probe_replaced`
  and not `abi_rebuilds`).
  Printed as `12/20` (the printed counter is a separate, known issue: #608;
  its #600 half is closed).

So portuale reaches the same rebuilds by a longer path. The count is not
required by #601 and stays out of scope, but the two facts may share a cause:
Portage's probe seeds the perl child in its very first pass; portuale learns
the set only after the `MissingDep` masks.

## The order divergence (#10) — separate

One swap: Portage `net-libs/nghttp3` then `sys-libs/libseccomp`; portuale the
reverse. Nothing in the replace-set finding touches it. Decide in Task 3
whether it is a tie-break of `_serialize_tasks` (ported) or a consequence of
the different search path (the rebuild nodes are inserted at different times).
Re-measure after Task 2 before spending effort on it.

## What this changes in the plan (updated 2026-10-28 after review)

- Task 0b (new): fix the debug instrument and dump `slot_operator_replace_installed`,
  `probe_replaced` and `abi_rebuilds` on the settling pass, for this cell and
  for the green control `dev-libs/p589bpull`. Explain the empty pair list
  before naming a fixture or a fix site.
- Task 1 fixture: `p589bpull` already prints `r U` + the block and is pinned
  green (`test_emerge_pretend_contract.py:36680-36736`), so a same-shape
  `X:0/1 -> X:0/2` fixture would not be red. Build only the shape Task 0b
  shows the cell differs by; assert the display pair and the `r` set, not the
  counter.
- Task 2: fix the site Task 0b names (pair scan scope / `probe_replaced`), not
  the renderer and not a second seeding path (the child insert exists).
- Cell cost: the full cell needs ~3 min per side; the probe of a single
  bed run with `--debug` is 30k-67k lines — keep only excerpts in evidence.

## Corrections after review (2026-10-28)

The first version of this file mis-cited and over-read its own captures. Fixed
here so the record is usable:

1. **Set size 36 → 35.** `captures/portage-forced-reinstall-atoms.txt` holds
   35 `atom:` lines (34 + `dev-lang/perl:0`). Portuale's 34 are the same 34.
   The old "36 vs 34, identical except perl" contradicted itself.
2. **Wrong `output.py`.** `812-816` is `lib/_emerge/resolver/output.py`; the
   bare name also matches `lib/portage/output.py:812-816` (the progress
   dialog).
3. **Wrong block lines.** `_compute_abi_rebuild_info` is `depgraph.py:931-1145`
   (the seed loop is `:940-950`); `_show_abi_rebuild_info` is `:1146-1159`.
4. **Wrong child-filing lines.** `_slot_operator_update_backtrack` starts at
   `depgraph.py:2415` and files the installed child at `:2454-2462`;
   `:2562-2566` is inside `_slot_operator_update_probe`.
5. **Wrong portuale probe lines.** The insert is
   `installed_parent_update_probe.rs:189-190`; `:119-121` is the
   `arg_pins_installed` gate.
6. **The H2 exclusion was unsound.** `captures/portuale-final-forced-atoms-empty.txt`
   is empty by construction: `pretend/mod.rs:637` passes `opts.reinstall_atoms`
   (the `--reinstall-atoms` CLI option, `pretend/args.rs:778-792`) into
   `print_resolver_debug`, while Portage's counterpart prints the internal
   force-reinstall args (`depgraph.py:940-950`). H2 is open until Task 0b
   dumps `probe_replaced` and `abi_rebuilds`.
7. **The 34 are the rebuild set.** The `BT replace-in` line prints
   `slot_operator_replace_installed` (`resolve/feedback/mod.rs:158-169`); the
   `r` column reads `probe_replaced` (`backtrack_params.rs:367-370` →
   `assemble.rs:295` → `result.slot_operator_replaced` → `pretend/mod.rs:495`).
   The two sets are not interchangeable.
8. **The machinery is not missing.** `p589bpull`/`p569pull` already produce
   `r U` + the block and are pinned green, so the cell's gap is shape-specific
   and the open question is the discriminator, not a missing insert.
