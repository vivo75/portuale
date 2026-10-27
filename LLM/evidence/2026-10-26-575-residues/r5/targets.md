# R5 (#598) targets, written before the L0 run (2026-10-27)

Cause (s0.md): portuale's slot-operator rebuild scan (`rebuild_scan.rs`
same-slot arm) only looks at consumers in `slot_op_reachable`. Portage
files the rebuild for any installed consumer the request's own walk
recursed into (`--deep`, D:3834). Fix direction: widen the same-slot scope
to `pass.installed_walked`.

## Must move to Portage (RED)
- `-pvuD dev-libs/p569pull`, `-pvuD dev-libs/p589bpull`: `r U prov-5.44`,
  `N core`, `rR virt-1`, `N pull-2`, "causes rebuilds for:" block, 1/20.
- `-pvD` (no `-u`) on both, and `-pvu --deep=2 p569pull`: same rows
  (captures in `captures/`). The `-D` rows may be a separate fix if the
  scope widening does not reach them: report, do not force.

## Must not move (controls)
- `-uD` and `-D` on p589a / p589c: 0/20, no rebuild.
- `-u --deep=1` and `-u` on p569pull: `N pull-1`, 2/20.
- `--backtrack=0 -uD p569pull`: rc 1, same conflict block.
- the world-seeded `W-*` cells; every `t8/pins.md` pin; keepers #54 K3, #222, #223.

## L0
- Predicted: no per-atom cell moves (no `--deep`). `MULTI_deep-update-world`
  (`-puvD @world`) stays as in `t10b/l0-after.txt` (#601 is a different
  mechanism). Any other cell moving is a stop; perl-544-only improvements
  follow the owner ruling.
