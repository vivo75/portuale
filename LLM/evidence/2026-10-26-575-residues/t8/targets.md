# Task 8 — target cells for Tasks 9–10 (written 2026-10-26)

A cell or pin not listed here must not move in Tasks 9–10 (Global
Constraints, L0 gate). Portage outcomes come from host Portage 3.0.83 on
scratch copies of the pmtest fixtures (`oracle.md`, `captures/`), or from
the bed's Portage side.

## New fixture cells (do not exist in pmtest yet; Task 9 adds them)

| Id | Shape | Today's portuale | Portage (cited) | Moved by |
|---|---|---|---|---|
| T1 `swap-noslotop` | p569 heal shape; installed `p569virt-1` RDEPEND `\|\| ( =p569prov-5.40* =p569prov-5.42* =p569prov-5.38* ~core )` (installed arm listed second), no slot-op atom; tree, md5-cache and vdb edited the same way; `-pv dev-libs/p569pull` | rc 0, 2/20, `N p569pull-1` + skip row `=5.42*` | rc 0, **0/20**, `U p569prov-5.44 [5.42]` + `N p569pull-2`, no skip block (`captures/portage-c-swap-noslotop.nodebug.txt`) | 10b (P1) |
| T2 `swap-slotop` | T1 plus `p569prov:=` in the consumer, vdb `:0/5.42=` | rc 0, 2/20, `N p569pull-1`, skip rows `p569prov:0/5.42=` **and** `=5.42*` | rc 0, 4/20, `N p569pull-1`, skip row `p569prov:0/5.42=` only (`captures/portage-c-swap-slotop.nodebug.txt`) | 10a (built atom as conflict parent; S0c) |
| T3 `orig-noslotop` | p569 shape without the slot-op atom, installed arm listed first | rc 0, 2/20, same rows as Portage | rc 0, **4/20**, `N p569pull-1`, skip row `=5.42*` (`captures/portage-c-orig-noslotop.nodebug.txt`) | 10a (choice order; S0d) |

Amended after the review of Tasks 9–10: T2's rows depend on the built
`:0/5.42=` atom acting as a conflict parent, so T2 belongs to 10a, not to
the `||` choice (review finding 2). Task 9 builds T1–T3 as separate fixture
families.

## Existing pins whose backtrack count may move (rows must not)

The contract pins below do not assert the count (checked 2026-10-26), but
the harvested corpus records the `Dependency resolution took … (backtrack:
N/20)` line where a case prints it. These are the only lines allowed to
drift, and only to Portage's count:

| Pin / cell | portuale today | Portage (`captures/counts-nodebug.txt`) | Moved by |
|---|---|---|---|
| k3 argument (`=dev-libs/paired-2.0`, keeper in world) | 0/20 | 3/20 | M2 |
| k3 needer | 1/20 | 4/20 | M2 |
| igw plain (`>=dev-libs/igw0x-2`) | 0/20 | 3/20 | M2 |

**Completed 2026-10-27 by Task 9 S0e** (`t9/s0e-counts.md`), see the
next section. Original note: this list was incomplete (review finding 9). Task 9 S0e completes it
with every CASES row whose pass reaches `dropped_pin_mask_restart` (the cgp
rows, the igw contrast, the k3 hard-dependency pin, `=dev-libs/p563pull-2`,
…), each with Portage's count. Only the completed list governs the 10a gate.

Known divergence kept as it is (not a target): the G0 combination, where
portuale gives 0 restarts and Portage 1/20. That is #594 (plan R3).

## L0 bed cells (baseline `l0-phase0.txt`, run `l0-20261026T222948Z`)

No L0 cell is a firm target: the oracle did not trace the bed's perl heals.
Verdicts below are read from `l0-phase0.txt` (corrected 2026-10-26: the
first version of this section took the bed-runner's summary, which called
`dev-db_mariadb` unexplained).

- **Explained only by `perl-544-stabilization-built-pin-residual`**:
  `dev-lang_perl`, `dev-perl_XML-Parser`,
  `virtual_perl-ExtUtils-MakeMaker` and `dev-db_mariadb`. Each of these
  may move **only to `clean`** (Portage's own output on the bed).
- **Unexplained cells that also carry findings from that entry**:
  `dev-lang_ruby`, `dev-ruby_rake` and `kde-apps_kdecore-meta` (`order`),
  and `media-gfx_gimp` (`order,totals`). There is also
  `gnome-base_gnome-shell`, explained by `gnome-shell-cluster-a-abort` plus
  two `perl-544-stabilization-version-selection-*` entries. **Owner ruling
  2026-10-26:** a move of these cells is not a stop when the manager has
  checked that it is an improvement: only findings from the perl-544
  entries disappear or move to Portage's output, and every other finding
  stays byte for byte the same (`l0-report.json`, by finding and entry
  name). Any other move of them is a stop.
- `MULTI_deep-update-world` (`unexplained:exit`) is not a target.

Any other move, or a move of a listed cell to anything but the allowed
outcome, is a stop condition.

## Full count list (Task 9 S0e, 2026-10-27)

Every call in the pmtest suite whose pass reaches `dropped_pin_mask_restart`
(heal fired, or a residual record held back by G0/G1/`--backtrack=0`), run
side by side with host Portage 3.0.83 (`t9/s0e-counts.md`,
`t9/captures/s0e/`). The rows are equal unless the table says otherwise.

| Cell | portuale | Portage | Corpus line |
|---|---|---|---|
| `p569pull` (#568/#569 heal) | 2/20 | 2/20 | no |
| `p563pull` | 2/20 | 2/20 | no |
| T1 `p589apull` | 2/20 → after 10a probably 4/20 (`N pull-1`, allowed) → after 10b | 0/20, `U prov-5.44` + `N pull-2` | no |
| T2 `p589bpull` | 2/20, two skip rows → after 10a still 2/20 with the extra `=5.42*` row → after 10b | 4/20, one row `:0/5.42=` | no |
| T3 `p589cpull` | 2/20 | 4/20 | no |
| keeper hard dependency `needer` | 1/20 | 4/20 | no |
| `p569pull t8x` | 3/20 | 3/20 | no |
| k3 argument `=paired-2.0` | 0/20 | 3/20 | no |
| igw plain, `>=igw0x-2` and `<igw0x-1` | 0/20 | 3/20 | no |
| cgp VER, both forms | 0/20 | 3/20 | no |
| needer/othermod triangle, both argv orders | 3/20 | 4/20 | **yes** |
| cgp USE | 0/20 | 0/20 (R4 keeps it) | no |
| `test_oracle_209`, or-choice `--backtrack=0` | 0/0 | 0/0 (must not move) | no |

**Allowed corpus drift in 10a:** only the two triangle CASES rows
(`--pretend dev-libs/needer dev-libs/othermod` and the reversed order),
`backtrack: 3/20` → `4/20`, with the rows byte-identical. The other cells
have no corpus entry. Their Portage counts are 10a's RED targets, checked
by the REDs and a recorded before/after `PORTUALE_BT_TRACE=1` run, not by
the corpus. T1 and T2 reach Portage only after 10b (review C1).

Separate finding (#598, plan R5): `--pretend --update --deep` on
`p569pull` and on the three `p589*pull` aborts in portuale (rc 1, 0/20) where
Portage merges (rc 0). It is not a 10a target.
