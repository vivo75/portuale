# #491 analysis — kdbusaddons trajectory parity (re-captured 2026-10-25)

Oracle: Portage 3.0.83 `--debug`, `emerge -uDpvN --backtrack=20
=kde-frameworks/kdbusaddons-6.31.0` on gentoo@350c08822 (rc 1).
Portuale: batch-branch binary @48dcf084 with `PORTUALE_BT_TRACE=1` (rc 1).
Raw 74 MB / 759 k-line Portage capture is ephemeral (`/tmp/kdb-portage/`,
not committed); per-pass digests live in this directory.

## The shape still reproduces (tree moved since 2026-10-23, same family)

Both sides exhaust backtracking and fail rc 1, with divergent pass-0 inputs:

- **Portage**: pass 0 fires **39 missed-slot-ABI probes + 11 slot-conflict
  backtracks**. Try 1 carries a 24-atom `slot_operator_replace_installed`
  set and prints `Total: 63`. Try 2 grows the set to 31 and prints `Total:
  189`, adding `slot_operator_mask_built` on 7 plasma binaries
  (powerdevil, plasma-workspace, kwin, plasma-vault, plasma-desktop,
  plasma5support, libplasma 6.8.1). Tries 3–19 add one missing-dependency
  mask each (`>=plasma-activities-6.8.1:6=` for powerdevil, workspace,
  kwin, vault …). Try 20 re-fires 9 probes + 26 slot backtracks, then
  `backtracking aborted after 20 tries`. Final abort: **14 conflicting
  slots**, headed by `libplasma:6` (6.8.1 binary vs installed 6.7.5 kept by
  installed consumers) and `plasma-activities:6`. Filed text said 48 pkgs /
  12 slots; the current tree says 63 / 14 — same divergence family.
- **Portuale**: `backtrack: 20/20`, rc 1. Pass 0 **settles clean**
  (`replace-in 0`, no conflicts). Try 2 files RevDep pins (kcmutils,
  kpackage, kparts, llvm). Try 3+ is SlotConflict feedback on
  **kde-frameworks only** (30 slots: 6.31.0 binaries vs installed 6.30.0
  kept by `=…-6.30*:6` pins). A 28-atom replace set appears only at try 4.
  **libplasma/plasma-activities never conflict**; no `SlotOperatorBuilt`
  mask is ever filed.

## Which consumers Portage's probe seeds per pass

`_slot_operator_trigger_reinstalls` runs at the end of **every** pass
(incl. pass 0). Each firing `_slot_operator_update_backtrack` files
`(root, slot_atom)` for the installed parent **and** the installed child
into `slot_operator_replace_installed`, plus `slot_operator_mask_built`
for non-installed binary parent/child — **while the installed child stays
graphed** as a nomerge node kept by other installed consumers' pins, so
the next pass reports the two-instance slot conflict.

- Pass 0 → try 1 (24): 16 libplasma consumers (plasma-pa/desktop/milou/
  workspace/kdeplasma-addons/merkuro/kirigami-addons/kscreen/kscreenlocker/
  vault/print-manager/powerdevil/login-manager/welcome/bluedevil/plasma-nm;
  installed child libplasma-6.7.5:6/7, new parent 6.8.1) + freerdp and
  qtwebengine (child openh264) + weston-16.0.0 (child libdisplay-info-0.3.0;
  the [B] shape, accepted) + libv4l (child libbpf).
- Try 1 → try 2 (+7 = 31): qemu, dolphin, kidletime, kio, kwindowsystem,
  kwin-x11, iproute2 — second-wave consumers reached through the rebuilt
  graph. Stable tries 3–19.

## What `fold_rebuild_scan` pre-folds instead

Portuale's scan only runs **inside SlotConflict feedback** (and the
built-parent probe). Pass 0 has no conflicts — its walk never holds the new
binary and the installed instance in one slot — so nothing seeds try 1.
When the fold finally fires (try 4: 28 atoms ≈ Portage's 31 minus
dolphin/kidletime/kio/kwindowsystem, plus kwin), it schedules the consumers
as plain `r` rebuilds **and drops the installed instances**: merkuro-26.08.2
is merged as ebuild, qtwebengine likewise. Portage instead keeps
merkuro-26.08.1-r1 and qtwebengine-6.11.2 installed, reporting both as
`cannot be rebuilt (ebuild masked or unavailable)` — their old `:6/7=` /
`:6=` pins keep libplasma-6.7.5 graphed, which keeps the libplasma and
plasma-activities conflicts alive through all 20 tries. That keep-graphed +
unrebuildable-consumer pair is the whole ball game: without it, no seed
timing change can produce Portage's conflict family.

## What a `replace_installed` seed changes (the port)

Mirror `_slot_operator_update_backtrack`'s filing in the pass-0 path: seed
`slot_operator_replace_installed` with installed consumers whose
replacement parent pulls a new child sub-slot, keep the installed child
graphed, and mask stale binaries as `SlotOperatorBuilt`. Expected effect on
the live pair: try 1 carries the plasma-24 set (Total ≈ 63), try 2 adds the
+7 wave with the 7 plasma `mask_built` rows (Total ≈ 189), and the abort
gains the libplasma/plasma-activities conflicts. `@world` P1–P4 must not
move (375 blocks, `backtrack: 5/20`, rc 0); keeper pins #54 K3, #222, #223,
#35 R4 and the [B] unit test stay green.
