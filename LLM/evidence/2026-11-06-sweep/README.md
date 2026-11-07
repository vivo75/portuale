# Backlog sweep 2026-11-06/07: index

Branch `backlog-sweep-2026-11-06` in both repos. Shas are from `git log` in each repo; "-" means no commit of that kind. Per-item evidence sits in the numbered subdirectories and in `clusterA.md` .. `clusterD.md` (diagnosis reports).

| Item | Status | portuale | pmtest | Notes |
|---|---|---|---|---|
| #602 | DONE 2026-11-07 | `382c9a19` | `0a947e6` | slot conflict while backtracking enters complete mode |
| #602 regression | DONE 2026-11-07 | `8a5bda63` | `b97338d` | complete walk starts from the first walk's backtrack masks (`602-regression/`) |
| #603 | DONE 2026-11-07 | `7a818198` | `5f48c66` | world-only consumer atom matching nothing installed no longer pins |
| #604 | DONE 2026-11-07 | `c62341c4` | `f755dae` | Q19 virtual leak keyed like Portage's atom intern key |
| #605 | DONE 2026-11-07 | `9bad4749` | `cfe6bcf` | shipped together with #671 |
| #606 | DONE 2026-11-07 | `4568309b` | `af79d1a` | installed consumer's chosen `||` arm is a parent of the merge it selects |
| #607 | DONE 2026-11-06 | `6a758d82` | `c731f2c` | only the built slot-operator atom is shown |
| #609 | DONE 2026-11-06 | `d14a2d7e` | `1ac580e` | final results stop at Portage's first failing dependency |
| #611 | OPEN, partly done | (a) `ca4981b6`, (c) `c47f9207`, (e) `f5ca4324` | (a) `5a2f7fc`, (c) `d743140`, (e) `338ef15` | (b) and (d) remain |
| #613 | DONE 2026-11-07 | `25665c08` | `7c3168f` | back off over both packages when the installed one also conflicts |
| #617 | OPEN, annotated | - | - | not reproducible on this host; quirk candidate Q28; needs bed S0 |
| #596 | OPEN | - | - | bed capture feasible, needs a new pmtest runner (`clusterD.md`) |
| #625 | DONE 2026-11-07 | (a) `9a5edd21` (pin/trace), (b) `809a2379` | (a) `a0f5f50`, (b) `96624bb` | (a) portuale already matched; (b) the walk's `||` choice owns the edge |
| #648 | OPEN, annotated | - | - | no oracle; line cites refreshed; parked at Tier 3 |
| #654 | CLOSED 2026-11-07 | - | - | no longer reproduces, premise wrong; real gap is #677 |
| #656 | DONE 2026-11-07 | - (work is #657: `fa85b574`, `9a2e7701`) | - | superseded by #657; plan header flipped |
| #658 | OPEN, re-scoped | `286b7c15` (records attempt 3) | - | the block prints; the parent LIST (10-36 vs 69 rows) differs; attempt-3 diff in `658/` |

## Items filed during the sweep

- #666 `force_reinstall` argument atoms are not modelled in `want_installed_pkg` (residue of #607).
- #667 blank lines around the slot-conflict `--verbose-conflicts` NOTE differ from Portage.
- #668 a `||` group whose branches both miss prints both misses; Portage prints the one its walk stops at (residue of #609).
- #669 a downgrade picked through an argument's own dependency prints `N` instead of `UD`.
- #670 the backtrack count of a slot-operator rebuild run differs from Portage (residue of #602).
- #671 an upgrade breaking an unwalked installed consumer's `[use]` dependency is a slot conflict (DONE with #605, same pmtest `cfe6bcf`, portuale `9bad4749`).
- #672 the `!!! Multiple package instances` block drops the ROOT suffix.
- #673 a walked consumer's `[use]` dependency broken by the upgrade.
- #674 with `--complete-graph` the backtrack count is the complete walk's.
- #675 a "causes rebuilds" block for a consumer that is upgraded anyway.
- #676 an installed consumer reached only through a merge-bound package is not evaluated in complete mode.
- #677 `--tree` does not print the `[nomerge]` owner chain (real gap behind #654).
- #678 pytest run from inside `pytests-contract-suite/` skips the corpus drift check when a test id contains a `/`.
