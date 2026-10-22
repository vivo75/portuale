# S0 trace findings (plan 01.477, 2026-10-22)

Captured same day, same host: `PORTUALE_BT_TRACE=1 portuale emerge -uDpvN @world` (42 tries, 368 packages,
`backtrack: 20/20`; trimmed trace in `portuale-bt-trace.txt`) and Portage `emerge -uDpvN @world --debug`
(374 packages; `runtime_pkg_mask` snapshots, new top-level keys per snapshot):

    P1 +mask_built binary mesa-26.2.4, binary kwin-6.8.0;
       +slot conflict ebuild plasma-activities-6.7.5, ebuild 6.6.6-r1, installed 6.7.5
    P2 +slot conflict ebuild libdisplay-info-0.4.0
    P4 +missing dependency installed libplasma-6.7.5
    P5 +missing dependency ebuild libplasma-6.7.5   (P6 ebuild libplasma-6.6.6-r1)

## Answers to the three open questions

1. **Why no `mask_built` (#478).** The 42 portuale tries contain zero `SlotOperatorBuilt` masks
   (reasons seen: 14 `MissingDependency`, 1 `SlotConflict`). Two structural causes:
   - Portage's `feedback()` (`resolver/backtracking.py`, "config" before "slot conflict") adds the
     `slot_operator_mask_built` change as a *config* node and derives the slot-conflict nodes from
     it, so P1 carries mask_built and the conflict masks together. Portuale's `collect_feedback`
     returns the first step that fires (`feedback/mod.rs` chain): `step_slot_conflict_backtrack`
     returns before `step_slot_operator_probe` is ever reached on a pass that has a conflict.
   - Even when reached, `step_slot_operator_probe` (`feedback/slot_operator_probe.rs`) is gated on
     `has_nvc` (a no-visible-candidate dead end); Portage's `_slot_operator_update_backtrack` fires
     during the walk for any non-installed built parent whose `:=` child has an update.
2. **Puller origin (#477).** Not examined further; the trace now prints `pullers` per conflict, and
   try 1/2 show plasma-activities pulled by plasma-meta-6.8.0, kmail, gwenview, skrooge (all 6.8.0
   side) -- the stale-installed-puller hypothesis is untested until S1/S2 change the trajectory.
3. **Missing-dep owner (#479).** The masks on `libplasma` are `!=kde-plasma/libplasma-6.8.0`
   (tries 8): the owner is the *merge-bound new* libplasma, not the installed 6.7.5 Portage masks.
   The owner is the direct parent in these tries (not plasma-meta at try 4 only), so the plan's
   "owner resolved to top ancestor" suspicion is **not** supported; the difference is that portuale's
   walk has libplasma-6.8.0 in the graph at that point because the slot-conflict choices
   (try 1/2: mask 6.8.0 vs mask 6.7.5, order 6.8.0 first) explore the opposite branch to Portage's
   (which keeps 6.8.0 and masks every 6.7.5/6.6.6 instance at once).

## Consequence for the plan

The first divergence is try 1: Portage masks *all non-chosen instances in one node*
(ebuild + binary + installed) plus mask_built; portuale branches per choice. S2 therefore needs a
walker-level change (probe during the walk + config-node-then-conflict chaining), which the plan's
section 3 says to surface to the owner before starting.
