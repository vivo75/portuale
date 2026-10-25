# #491 hermetic probe record (`probe-p491.sh`)

Oracle: Portage 3.0.83 vs portuale (branch tip `48dcf084` + Task 6 working
tree), hermetic `p491*` fixture (installed `p491lib-1` `6/7`, `p491con-1`
`:6` with vdb-baked `>=dev-libs/p491lib-1:6/7=`, installed-only keeper
`p491keep-1` pinning `dev-libs/p491lib:6/7=`; tree `p491lib-2` `6/8`,
`p491con-2` `:6` with `>=dev-libs/p491lib-1:6`, update root `p491top-1/2`
`:6` — mirrors installed libplasma-6.7.5/plasma-pa-6.7.5 vs the 6.8.1
ebuilds, keeper merkuro-26.08.1-r1). All cells `emerge -pv
--ignore-default-opts --getbinpkg=n --usepkg=n --color=n`; captures in
`captures/<pm>-<cell>.{argv,rc,stdout,stderr}`.

## Cell results (post-fix working tree; agreement cells identical pre-fix)

| cell | argv | Portage | portuale |
|---|---|---|---|
| upd | `-uDN =con-2` | rc 0, 0/20, lib+con upgrade | rc 0, 0/20, identical |
| bare | `=con-2` | rc 0, 0/20, con only | rc 0, 0/20, identical |
| con-upd | `-uDN con` | rc 0, 0/20, lib+con upgrade | rc 0, 0/20, identical |
| top-upd | `-uDN =top-2` | rc 0, 0/20, con+top upgrade, lib-1 kept | rc 0, 0/20, identical |
| top-bare | `-uDN top` | rc 0, 0/20, con+top upgrade | rc 0, 0/20, identical |
| v1 | `-uDN =con-1 =lib-2 keep` | **rc 1, 6/20**, lib:6 conflict (lib-2 vs lib-1 kept by con-1's `>=lib-1:6/7=` and keep-1's `lib:6/7=` pins; both "ebuild masked or unavailable") | rc 0, 1/20, lib-2 + rR con-1 rebuild, keep-1's pin silent |

Selection finding (from `--debug`, `p491-debug-top-upd.stderr`): Portage's
`_select_package` keeps installed lib-1 under the keeper's built pin
(dep_zapdeps downgrade family) — relaxing the keeper's pin to `>=lib-1:6`
schedules lib-2 with backtrack 0/20. Both managers agree on every
non-conflict cell.

## V1 divergence (named remaining difference, not Task 6)

V1's `=con-1` arg plays selective-keep (live: `@world` traversal under
selective update): con-1 stays installed in-graph while lib-2 is scheduled.
Portage explores 6 tries (slot-conflict + missing-dep masks,
`captures/p491-debug-v1.stderr`: 4 "Starting slot conflict handler" rounds,
zero update-probe lines — the pinned consumer's dep wants no update) and
aborts rc 1 on the lib:6 conflict. Portuale's conflict feedback rebuilds
con-1 same-version (rR, deps re-read from the unbaked `:6=` ebuild, dropping
the baked `:6/7=` pin) and settles rc 0. Fixing that needs conflict-feedback
depth (enumerate mask configs like `_slot_confict_backtrack`) plus keeping
installed consumers' baked pins across forced reinstalls — a separate slice
with its own number, not the pass-0 seed.

## Guards (post-fix working tree)

- `@world` (`emerge -uDpvN @world`): portuale rc 0, **4/20** (branch baseline
  4/20; Portage 4/20), Total identical (426 packages), merge rows
  byte-identical (429; `world-ptg.rows` vs `world-ptl.rows`, 0 diff lines).
  Round 1 placed the new step beside the built-parent probe and moved @world
  to 5/20 (extra Config restart preempting the MissingDep mask per cycle);
  round 2 moved it last in `collect_feedback_inner` (clean passes only) and
  restored 4/20 with identical rows (`portuale-world-postfix.out`,
  `portage-world-postfix.out`).
- Keeper pins (pmtest, focused): 14/14 green (`test_keeper_reachable_*` ×3,
  `test_or_choice_avoids_downgrade_into_the_graphed_update`,
  `test_complete_graph_*` / `test_ignore_world_*` / `test_upstream_*`).
- `[B]` unit test
  `built_parent_update_probe_leaves_a_child_another_installed_consumer_restricts`
  green (workspace `cargo test --release`: 3068 passed, 0 failed).
- Full pmtest suite: 4494 passed, 2 skipped, 2 xfailed, 0 failed
  (branch expectation per #568: 4492 + 2 pins; no corpus drift).
- Live kdbusaddons pair (`=kdbusaddons-6.31.0 -uDpvN --backtrack=20`):
  portuale rc 1, 20/20, kde-frameworks conflicts, replace-in 28 from try 4 —
  unchanged by this slice (`portuale-bt-trace-postfix.txt`,
  `portuale-per-pass-postfix.txt`). Target 63 pkgs / 14 slots unreached:
  the seed needs world-reached installed consumers in-graph, which needs
  complete-graph member traversal (Task 6 residue).

## Probe-side mutation incident (2026-10-25, restored)

The first Portage `-uDN` probe run printed "Performing Global Updates" and
applied the fixture repo's `profiles/updates/2Q-2024` to the shared vdb
(`oldmovepkg-1.0` files deleted, `slotmovepkg-1.0/SLOT` rewritten,
`newmovepkg-1.0/` created) — breaking `vdb_view` tests until restored via
`git checkout` + `rm -rf` in `../pmtest` (mtimedb itself is git-ignored).
Re-runs are quiet (record current). Repro: re-run `probe-p491.sh`, then
`git -C ../pmtest status` must show only the `p491*` additions.
