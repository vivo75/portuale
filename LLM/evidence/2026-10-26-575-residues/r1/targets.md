# R1 (#592) targets, written before the L0 run (2026-10-27)

Rule (s0.md): a pass that hits an unsatisfied dependency which stops
Portage's walk (D:3535 -> D:3276-3280 -> `_resolve` D:5691-5693) never
reaches `altlist`'s slot-conflict processing. portuale files conflict
feedback in such passes.

## Must move to Portage (RED, fixtures `dev-libs/r592*`)
1. `-pv r592atop`: rc 0, 1/20 (portuale 3/20).
2. `-pv --backtrack=1 r592atop`: rc 0, 1/1 (portuale rc 1).
3. `-pv r592ctop`, plus `--backtrack=1`: rc 0, 1/20 and 1/1.
4. `-pv --autounmask=n r592dtop`: rc 1, 0/20, no merge list (portuale rc 0).
5. `-pv --autounmask=n r592ftop`: rc 1, 0/20 (portuale 5/20).

## Must not move
- E (6/20, 5 rows), E `--backtrack=1` and `=0`; D and F at default options.
- Every `t8/pins.md` pin, keepers #54 K3, #222, #223, R4 and 10a/10b pins.
- Known divergence kept (not R1): A `--backtrack=0` prints a conflict block
  Portage's stopped graph never has; empty repo name `::` on a failed
  conflicted graph (residues filed).

## L0 (baseline `r5/l0-after.txt`, run `l0-20261027T195839Z`: 91/16/44... cell by cell)
- No cell is expected to move. "May move, only toward Portage": 
  `kde-plasma_plasma-meta`, `gnome-base_nautilus`, `app-editors_gedit`,
  `gnome-base_gnome-shell` (Portage ends on a masked / USE-unsatisfied dep).
- Any other move is a stop. Perl-544-only improvements follow the owner ruling.
