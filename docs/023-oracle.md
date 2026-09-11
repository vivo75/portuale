# 023 oracle — real-portage slot-conflict backtracking cases vs portuale

B1 slice of `docs/023-backtracking_resolve.md`. Each case is an upstream
`lib/portage/tests/resolver/` `ResolverPlayground` test translated to
`fixtures/repo/…` (+ `metadata/md5-cache/…`) with per-test tmp ROOTs for
installed/world state; the upstream `mergelist` **is** the oracle (no
container run needed). Contract tests `test_oracle_*` pin portuale's
current output (Rust == Python byte-identical on all twelve) and are
labelled MATCH / DIVERGENT / PARTIAL accordingly.

World/method note: upstream `@world` runs with an empty `@system`;
portuale's fixture profile ships `@system = {newpkg, withdeps,
systempkg}` (+`upgradepkg`), so every `@world` run below also lists
those — fixture-profile noise, identical on both sides, filtered out of
the comparison. Already-installed packages are likewise silent in both
implementations. L0 shows nothing attributable: the 22 `[version]`
findings were a fixed comparator artifact (slot-blind identity keying),
per `TEST/findings/l0.md` cluster M — re-running L0 for this slice is
therefore not diagnostic and was not repeated beyond the Phase A runs.

EAPI note: upstream cases use EAPI 5/7; fixtures use EAPI 8 throughout
(portuale treats 5+ identically and none of these cases uses
EAPI-gated dep syntax).

## Table

| case | upstream source | real mergelist | portuale mergelist | verdict | mechanism exercised |
|---|---|---|---|---|---|
| mgf | `test_slot_conflict_mask_update.py` (mask highest, not first-pulled) | `[mgfc-1, mgfb-1, mgfa-1]` | same | MATCH | highest-child mask (portuale slice 3 already ships it) |
| btb | `test_backtracking.py::testBacktracking` (`=btba-1` + `btbb`, both orders) | `[btba-1, btbb-1]` | same | MATCH | explicit pin beats transitive pull (no backtrack involved) |
| btn | `test_backtracking.py::testBacktrackNotNeeded` (`--backtrack 1`) | 4 pkgs, any order | same set+order | MATCH | one-step solvable growth inside the mask budget |
| btw | `test_backtracking.py::testBacktrackWithoutUpdates` (installed `btwz-1`, no `--update`) | `[btwz-2, btwb-1, btwa-1]`, any order | same | MATCH | fresh highest selection over an installed old version |
| btmu | `test_backtracking.py::testBacktrackMissedUpdates` (`--update --deep --selective`) | `[]` | `[]` | MATCH | selective no-op (correctly missed update) |
| boost | `test_slot_conflict_update.py` (missed-update: mask lower first) | `[boost-build-1.53.0, boost-1.53.0, libcmis-0.3.1]` | same upgrades (`libcmis` installed-silent) | MATCH (versions) | highest-first + solvable growth decides before any mask path; the lower-mask-first ranking never triggers because no conflict arises. Guard, not a ranking probe. |
| virt | `test_slot_conflict_update_virt.py` (bug 692746: mask the all-matched existing node) | `[mysql-connector-c-8.0.17-r3, libmysqlclient-21, DBD-mysql-4.44.0]` | same upgrades (`DBD-mysql` installed-silent) | MATCH (versions) | same as boost: no slot conflict arises, so the 692746 append never triggers. Guard, not a probe. |
| btnr | `test_backtracking.py::testBacktrackNoWrongRebuilds` (`--backtrack 6`) | `[]` | `[btra-2 U, btrc-2 U]` + residual slot conflict | DIVERGENT | Second-opinion review confirmed the deeper cause: `btra-1` (via installed `btrd-1`'s `<btra-2`) resolves `AlreadyInstalled` and never becomes a slot instance (`resolved_slots` indexes merge-bound outcomes only), so **no direct slot conflict ever forms** -- the reported block is a dropped-pin residual. No mask node can form, with or without C2's stack. Closing this needs installed-instance conflicts first (backlog #25: `_complete_graph` nomerge nodes), and only then do C2's 375573 discard + C3's ranking have anything to work on. C2's search shape is still the required substrate. |
| bwd | `test_aggressive_backtrack_downgrade.py` (bug 693836, `--update --deep`) | `[]` | `[]` (no `firefox`/`libvpx`/`ffmpeg` lines) | MATCH | no aggressive downgrade on either side. Finer `conflict_downgrade` variants are backlog #35, not #23. |
| a522084 | `test_solve_non_slot_operator_slot_conflicts.py` (bug 522084, `--update --deep`) | `[app-misc/A-2, app-misc/B-0]` | `[app-misc/A-2]` (`B-0` absent) | PARTIAL | version choice MATCHES (A-2, no missed update -- the #23-adjacent half). The missing `B-0` subslot rebuild is the `:=` rebuild path (backlog #24), filed separately, not a #23 driver. |

Skipped, with reason: `test_missed_update.py` (Qt 6.9.3→6.10.1) is
`xfail` *upstream* -- real portage itself does not solve it (pending
"earlier slot operator backtracking", bug 964705/968228); it is #24
territory, not oracle material for #23. `testBacktrackInconsistent-
ForcedRebuildWithBlocker` (gr-iqbal/qwt/boost) is slot-op-rebuild +
blocker machinery (#24 + scheduler), likewise out of scope.

## C3 additions (not in the B1 brief)

| case | source | real | portuale | verdict | mechanism |
|---|---|---|---|---|---|
| mg2 | C3 first-only deferral (no direct upstream equivalent; real `_feedback_slot_conflicts` takes `conflicts_data[0]`) | all `-1.0`s (by construction: two independent mask-good-first subtrees under `mg2top`) | same 7 lines | MATCH | two simultaneous conflicts: only the first becomes nodes, the second is handled under each sibling in later passes. Guards the deferral order. |
| 375573-unit | Rust unit `check_runtime_pkg_mask_discards_fully_masked_parent_sets` | real `_check_runtime_pkg_mask` truth table (discard iff every conflict parent masked; no-parent and missing-dep arms valid) | same | MATCH | Direct predicate oracle: no fixture reaches the 3-pass parent-masking chain end to end yet (recorded gap, not a divergence). |
| mg3 | C4 similar-grouping (`mgfc-3.0` visible, pulled by nothing; default budget) | all `-1.0`s | same 3 lines | MATCH | Missed-update sibling joins the mask group; both languages settle identically. |
| mg3-bt1 | same, `--backtrack 1` | all `-1.0`s (grouped 1 node + mask-aware selection picks `mgfb-1.0` directly) | conflict reported (`[mgfc-1, mgfb-2, mgfa-1]`) | DIVERGENT | Portuale groups the node identically but still selects highest-visible `mgfb-2.0` and NVCs; the downgrade needs a second mask step the budget denies. Mask-aware candidate fallback, backlog #36 (beyond #23's loop scope). |

## Verdict: GO for Phase C (narrow, re-scoped after C2)

Exactly one case diverges (btnr), but C2's implementation + second-opinion
review showed the divergence is NOT closable by the node stack alone: the
conflict is residual (no direct slot instances without installed-nomerge
nodes, backlog #25). Phase C still ships, re-scoped:

- C2 (done): the real search shape (node stack, mask-step budget,
  `get_best_run`, 375573 wiring, dead-end abandonment) with zero pin
  moves -- the substrate every later piece needs.
- C3: real mask-choice generation (ranked one-node-per-choice). Changes
  WHICH masks form, not whether conflicts are visible; verified against
  the mgf/boost/virt regression pins + L0.
- C4: similar-grouping + closure.
- btnr closes only with #25 (installed instances as conflict parties)
  feeding C2/C3's machinery -- recorded as the named follow-up, not a
  C-scope change.

Non-#23 residue filed, not driving C: the a522084 `B-0` rebuild miss
(#24 slot-operator rebuild undo path).

## Verdict: CLOSED with #23 (2026-09-11)

C2+C3+C4 shipped, all green (contract 1124 + 3 xfailed, zero moves from
the C2 baseline; L0 byte-identical post-grouping). Final table: every
case matches except three filed divergences, each with a named home
outside #23 -- btnr (needs #25 installed-nomerge instances before the
search can see the conflict), a522084-`B-0` (needs #24 `:=` rebuild),
mg3-bt1 (needs #36 mask-aware selection fallback). C4 acceptance met:
no table entry moved down; mg2 + 375573-unit + mg3-default are new
matches; the rest held.
