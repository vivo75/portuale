# R4 S0: expected L0 bed moves

Source: `pmtest/differential-test-bed/logs/l0-20261027T071513Z/{real,portuale}/<cell>.txt`
(excerpts in `captures/bed/`). Logs were enough; the bed was not rerun.

## media-gfx_gimp and gnome-base_gnome-shell: **no move from the handler**

- Portage: `backtrack: 0/20`, rc 1. The `dev-lang/perl:0` conflict (installed 5.42.2 vs ebuild 5.44.0)
  has `=dev-lang/perl-5.4x*` and `dev-lang/perl:0/5.42=` parents. These are version and slot reasons,
  so `is_a_version_conflict` is True and every configuration is rejected at SC:837-846. `changes` is
  empty, and **no handler explanation is printed**: Portage prints the package.mask advisory.
- The arm that stops the loop is **arm 3, the autounmask arm** (D:11865-11883), not the handler.
  On try 0 arm 2 builds the handler, which finds nothing, then arm 3 sets
  `_autounmask_backtrack_disabled`. Portage prints `The following USE changes are necessary to proceed:`
  and ` * In order to avoid wasting time, backtracking has terminated early` (D:11249-11256):
  `real/media-gfx_gimp.txt:223,241`, `real/gnome-base_gnome-shell.txt:495,538`. This corrects s0b's
  "another arm of D:11838-11885": it is the autounmask arm.
- After R4, as long as the port keeps arm order 1→2→3 and a version conflict yields no solution:
  no change in count (0/20), rows or rc.
- **A residual divergence in these cells' text, already there today and next to R4.** Portage's
  advisory ends `…can not be installed simultaneously.` with **no** `--backtrack=30` sentence: the
  displayed depgraph is the try-0 pass with `_allow_backtracking=True` (D:1765-1775). portuale
  prints the sentence (`portuale/media-gfx_gimp.txt:215`, `portuale/gnome-base_gnome-shell.txt:487`),
  because `settled_with_backtracking` is set only on the replay arm (`crates/portage-repo/src/resolve/driver.rs:124`).
  If R4 routes every `need_config_change` stop (arm 2 *and* arm 3) through one "report this pass,
  backtracking still allowed" exit, that sentence goes away in both cells and they move toward
  Portage. The L0 report does not grade this today, so expect no grade change.
- The other text differences there (which perl parents are shown and how many, the extra
  5.42.2 instance block, the order of the autounmask entries) are the 10a/10b and #567 topics in
  `t9/s0b.md`, not R4.

## Other cells

- `grep -l "It might be possible to solve\|Enabling --newuse"` over `real/*.txt` in this run finds
  0 cells. So no L0 cell has a handler explanation and none can move by gaining one.
- Portage prints a slot conflict in exactly four cells: gimp, gnome-shell, **app-office_libreoffice**
  and **kde-plasma_plasma-meta**. All four have the same shape:
  - a `dev-lang/perl:0` version conflict with an empty handler;
  - the try-0 stop through the autounmask arm (`USE changes are necessary` +
    `terminated early`, Portage `backtrack: 0/20`);
  - no `--backtrack=30` sentence in Portage's text, while portuale prints it
    (`portuale/app-office_libreoffice.txt:262`, `portuale/kde-plasma_plasma-meta.txt:504`).
- plasma-meta also differs in count today: portuale `1/20` vs Portage `0/20`. That is a portuale
  backtrack before the autounmask stop, not the handler; it is the plasma-meta topic in memory/#244.
- Expected R4 moves in L0: **none from the handler**. Possible text-only moves (the `--backtrack=30`
  sentence disappearing) in these four cells, if the stop exit is unified as described above.

## Contract side (see cases.md)

- `test_complete_graph_use_break_fails_with_world_consumer`: the advisory becomes Portage's
  `(Change USE: -icu)` explanation. The count stays 0/20. The only expected output move.
