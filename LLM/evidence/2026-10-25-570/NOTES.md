# #570 S0 oracle: benign disclosure blocks the dropped-pin heal (G0)

Cell (`probe-g0.sh`, both managers, `emerge -pv`, fixture ROOT + scratch
Portage configroot with absolute repo paths — same harness as
`LLM/evidence/2026-10-26-568/probe-p569.sh`): one healable dropped-pin
residual (`dev-libs/p569pull`, the Task 2 `#569` family shape) **plus** one
benign disclosure on an unrelated cp (`dev-libs/abort-masked-mid`, whose
dep `dev-libs/maskeddep` is `package.mask`'d) on a single two-atom probe.

Raw captures: `captures/{portage,portuale}-g0{combo,heal,mask}.*`.
Pretend-only (`-p`): no vdb writes. The Portage side still runs
`Performing Global Updates` against `ROOT=$FIX`, which moved
`fixtures/var/db/pkg/dev-libs/{oldmovepkg → newmovepkg, slotmovepkg SLOT}` —
restored with `git checkout --` + `rm -rf newmovepkg-1.0/` right after the
run (pmtest tree verified clean).

## Order: disclosure before heal

`g0combo` (`dev-libs/p569pull dev-libs/abort-masked-mid`):

- Portage: rc 1, `backtrack: 1/20`, reports **only** the masked disclosure
  (`!!! All ebuilds that could satisfy "dev-libs/maskeddep" ... have been
  masked`, required by `abort-masked-mid-1.0` / the argument). No skip
  block, no `triggered by backtracking` notice, no residual slot conflict —
  the heal never fires through the disclosure.
- portuale (Task 2 tree, G0 already in place): rc 1, `backtrack: 0/20`,
  reports the residual `dev-libs/p569prov:0` slot conflict (stdout) **and**
  the masked disclosure (stderr). No skip block, no heal excursion — G0
  fired. Display delta vs Portage (residual block printed alongside the
  masked block) is out of scope for #570; the pin is resolver-level.
  Count nuance, also out of scope: Portage counts `1/20` (one missing-dep
  retry) where portuale counts `0/20` — the `PORTUALE_BT_TRACE=1` transcript
  shows two `Feedback:MissingDep` rounds masking only the disclosed parent
  (never the provider), yet the settled result reports zero restarts. The
  exact driver path is not established; what the pin relies on is only the
  settled values (restarts 0, residual recorded, abort loud).

Controls:

- `g0heal` (`dev-libs/p569pull` alone): both rc 0, `backtrack: 2/20`, skip
  block + `triggered by backtracking` notice — the residual is healable
  without the disclosure.
- `g0mask` (`dev-libs/abort-masked-mid` alone): both rc 1 with the masked
  disclosure (Portage `backtrack: 1/20`, portuale `backtrack: 0/20`) — the
  disclosure is benign and unrelated to the residual.

## Consequence for the unit test

The G0 cell is constructible and G0 fires on it, so #570 closes with a
passing pin, not a non-gap: `g0_benign_disclosure_blocks_the_dropped_pin_heal`
in `crates/portage-repo/src/tests/tests_p563.rs` resolves both atoms
together and asserts the loud residual abort (rc 1 shape: `Aborted` outcome
via the masked NVC entry, the `p569prov` slot conflict still recorded) with
zero backtrack restarts and no skip rows.
