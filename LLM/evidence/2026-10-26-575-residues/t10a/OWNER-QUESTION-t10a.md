# Owner question, Task 10a (2026-10-27)

10a (#580 P2+P3) is green and uncommitted in the working tree. It is held on
one plan stop condition.

## Gate results
- Full gate `r10a2`: fmt, clippy (0 warnings), 3195 Rust tests, pmtest
  4553 passed, 0 corpus drift.
- A hash-order flake in `test_oracle_209_…` was fixed in the one allowed fix
  round (sort in `rev_dep.rs` and `slot_conflict.rs`). It now passes 20 of 20
  serial runs here and 30 of 30 in the implementer's runs.
- Keepers #54 K3, #222, #223, #35 R4 and every `t8/pins.md` pin are green.

## Why it stopped
L0 run `l0-20261027T141949Z` (`l0-after.txt`): 88/5/27, the same as the
baseline, with no verdict-class change. But `MULTI_deep-update-world` moved,
and `t8/targets.md` calls it "not a target", so a move is a stop.

The move:

| | r4 (`l0-20261027T124915Z`) | 10a | Portage |
|---|---|---|---|
| rc | 1 | **0** | 0 |
| perl slot-conflict block | printed (two `perl-544-stabilization-slot-conflict-a/b` findings) | gone | none |
| merge list | 107 lines, comparison suppressed | **141, same package set, same `Total:` line** | 141 |
| findings | `exit` | `flags` (perl `[r U]` vs `[U]`), `order` (diverges at #10) | — |

The two new findings were hidden before: the list was not compared while
the rc differed.
- `flags`: Portage marks the perl upgrade with `r` and prints a "causes
  rebuilds for:" block. portuale prints neither.
- `order`: the list is an ordering difference only, with the same set.

Both look like existing slot-operator rebuild and merge-order gaps that are
now visible. They are not regressions.

## Other changes on the bed (allowed, perl-544 only)
- The four perl conflict cells (gimp, gnome-shell, libreoffice, plasma-meta)
  gain the built `:0/5.42=` parent rows (+21 each; Portage has more).
- The skipped-updates cells change only row order.
- No other finding moved.

## Also decided by the manager (tell me if you disagree)
- After 10a, five count cells print one try fewer than Portage (k3, igw,
  cgp VER 2 against 3; needer and the triangle 3 against 4). The search makes
  Portage's tries; the printed count is an older driver exit-count bug.
- I treated that as explained and filed it as R6 / #600. Because of it, the
  planned triangle drift from 3/20 to 4/20 did not happen.

## Options
1. Accept the move as an improvement, commit 10a, and file the
   `deep-update-world` perl `r` flag/rebuild notice as a residue (#601).
   Then continue with 10b. (Recommended)
2. Hold 10a and investigate the `r` flag and order in this plan first.
3. Do not land 10a: restore to `2dc48dda` and narrow #580.
