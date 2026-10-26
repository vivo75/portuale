# Task 8 — target cells for Tasks 9–10 (written 2026-10-26)

A cell or pin not listed here must not move in Tasks 9–10 (Global
Constraints, L0 gate). Portage outcomes come from host Portage 3.0.83 on
scratch copies of the pmtest fixtures (`oracle.md`, `captures/`), or from
the bed's Portage side.

## New fixture cells (do not exist in pmtest yet; Task 9 adds them)

| Id | Shape | Today's portuale | Portage (cited) | Moved by |
|---|---|---|---|---|
| T1 `swap-noslotop` | p569 heal shape; installed `p569virt-1` RDEPEND `\|\| ( =p569prov-5.40* =p569prov-5.42* =p569prov-5.38* ~core )` (installed arm listed second), no slot-op atom; tree, md5-cache and vdb edited the same way; `-pv dev-libs/p569pull` | rc 0, 2/20, `N p569pull-1` + skip row `=5.42*` | rc 0, **0/20**, `U p569prov-5.44 [5.42]` + `N p569pull-2`, no skip block (`captures/portage-c-swap-noslotop.nodebug.txt`) | M1 |
| T2 `swap-slotop` | T1 plus `p569prov:=` in the consumer, vdb `:0/5.42=` | rc 0, 2/20, `N p569pull-1`, skip rows `p569prov:0/5.42=` **and** `=5.42*` | rc 0, 4/20, `N p569pull-1`, skip row `p569prov:0/5.42=` only (`captures/portage-c-swap-slotop.nodebug.txt`) | M1 (rows), M2 (count) |
| T3 `orig-noslotop` | p569 shape without the slot-op atom, installed arm listed first | rc 0, 2/20, same rows as Portage | rc 0, **4/20**, `N p569pull-1`, skip row `=5.42*` (`captures/portage-c-orig-noslotop.nodebug.txt`) | M2 (count only) |

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

## L0 bed cells (baseline `l0-phase0.txt`, run `l0-20261026T222948Z`)

No L0 cell is a firm target: the oracle did not trace the bed's perl heals.
The three cells explained by `perl-544-stabilization-built-pin-residual`
(`dev-lang_perl`, `dev-perl_XML-Parser`, `virtual_perl-ExtUtils-MakeMaker`)
may move **only to `clean`** (Portage's own output on the bed); any other
move of them, or any move of any other cell, is a stop condition. Open
question for the re-review: whether `media-gfx_gimp`, `dev-db_mariadb` and
`MULTI_deep-update-world`, which carry perl-544 lines among other
divergences, should get the same "only toward Portage" allowance.
