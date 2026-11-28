# crates/portage-repo/src/tests/tests_p651.rs
#651: a seed committed BEFORE the slot conflict opens must not survive it.
Backlog #651; plan `LLM/02.651-seed-skip.opus.md` (done); S0 #649
(`LLM/evidence/2026-11-03-649/REPORT.md`); C1 oracle
`LLM/evidence/2026-11-04-651/probe651_oracle.py`.

## fn bare_request_with_transient_merging_seeds_the_fall_through
- Bare N1-mirror leg: `resolve_bare_in(&root, &["dev-libs/p651prov"])`
  (binary parity: `selective = false`, like the CLI's bare
  `emerge --pretend`).
- Oracle history: written when the fixture vdb carried no `=5.42*`
  holder, pinning the selection-side transient-U (`U p651prov-5.44.0` +
  both fall-throughs N) against the N1 playground reinstall
  (`['dev-lang/perl-5.42.2']`) -- the recorded S0 section-6 "N1 vs N2
  perl-row divergence", explicitly NO-CHANGE to guard #651 against
  over-blunting (a fix green on the chain leg by dropping seeds while
  still merging the transient here confuses the two demotions).
- Re-pinned 2026-11-05 (#656 leg (a)): Task 1 added three installed
  `=5.42*` holders (`p656hold{1,2,3}`) to the shared vdb, so this world
  IS N1 + holders -- and Portage's verdict on exactly that shape is
  reinstall: P2 (`probe656_oracle.py`, `SUCCESS True`,
  `MERGELIST ['dev-lang/perl-5.42.2']`, no skip) and H0
  (`probe656h_oracle.py`, same with tree-present holders:
  tree-presence is invisible to Portage). New pin: prov R 5.42.2 + no
  fall-throughs + no virtual U/D. The old U pin was correct for its
  holder-free world; the world changed under it, not the oracle.
- Backlog numbers removed from the comment (original text): The bare N1-mirror leg: the fixture vdb grew three installed `=5.42*` holders under Task 1 (`p656hold{1,2,3}`), so this world is N1 + holders and Portage's verdict on exactly that shape is reinstall, not upgrade: P2 (`probe656_oracle.py`: `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.42.2']`, no skip) and H0 (same with tree-present holders: tree-presence is invisible to Portage). The #656 leg-(a) gate files the kept virtuals' `||` arms on the holder shape, the mask lands 5.44, the seeds void, and the provider reinstalls with neither fall-through merged -- the R posture the old U pin predates. Re-pinned 2026-11-05 (was: transient-U + both fall-throughs, written when the fixture vdb carried no `=5.42*` holder).

## fn chained_holder_skips_the_transient_and_holds_the_installed_binding
- C1 leg: bare `[p651prov, p651top]` (requested-through holder). See the
  test docstring for the oracle (`probe651_oracle.py` C1:
  `['dev-perl/top-1', 'dev-lang/perl-5.42.2']`, WARNING-proven skip).
  Unaffected by #656 leg (a) (same rows post-gate: the holder/top plain
  pins already skip; the virtual-arm pins join them consistently).

## module
- Backlog numbers removed from the comment (original text): #651: a seed committed BEFORE the slot conflict opens must not survive it.  S0 #649 (`LLM/evidence/2026-11-03-649/REPORT.md`) locks the cause and #650 proved the open-conflict half hermetically (the withhold gate declines a seed whose demoter sits in the committing pass's open slot conflict). #651's residue is the commit-before-conflict ordering the bed re-grade proved (`LLM/backlog-tasks.md` #651): on the `dev-lang_perl` cell both seeds `commit` in the first pass with demoter `perl-5.44.0` and zero `decline` lines — the committing pass has no open conflict yet, the transient's conflict surfacing only as try-2 `SlotConflict` feedback with a try-3 settle mask — and the feed never re-visits a settled seed.  Expected values are Portage 3.0.83's own playground (`LLM/evidence/2026-11-03-649/probe649_mutations.py` N1/N2 verbatim, re-run 2026-11-04, plus the C1 chained-holder run in `LLM/evidence/2026-11-04-651/probe651_oracle.py`): - N1 (installed perl-5.42.2 + installed 5.42-arm virtuals, tree 5.44.0 with the textually-identical PDEPEND, bare `dev-lang/perl` request): `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.42.2']` — the virtual kept, no fall-through merged. - C1 (N1 + a tree+installed `dev-perl/holder-1` carrying the cell's `dev-lang/perl:0/5.42` reverse edge, pulled transitively through a requested `dev-perl/top-1` instead of requested directly — the N2 leaf is inert, the direct request resolves the conflict too early): Portage `SUCCESS True`, `MERGELIST ['dev-perl/top-1', 'dev-lang/perl-5.42.2']` — reinstalls only, the transient skipped, no fall-through merged.  The `p651*` fixture family mirrors that C1 shape hermetically (`dev-libs/p651prov-5.42.2`/`-5.44.0` for `dev-lang/perl` with the live `!minimal?` PDEPEND cut to the two interacting conjuncts, per the M4/Carp precedent; `virtual/p651enc-*`/`virtual/p651th-*` with the verbatim real RDEPENDs; `dev-libs/p651enccore-*`/`dev-libs/p651thcore-*` for `perl-core/*`; `dev-perl/p651holder-1` for the background reverse edge, pulled through requested `dev-perl/p651top-1`; `dev-perl/p651pin-1` installed-only for the cell's background reverse-edge pressure, N2 mirror — inert, never merged). `minimal` defaults OFF by profile-chain absence, so the PDEPEND block is live; the commit-before-conflict ordering is verified by the seed-decision debug log (`SEED commit` with demoter `p651prov-5.44.0` before any `SlotConflict` mask involving it, zero `decline` lines — recorded verbatim in `probe651_oracle.py`).  Portage: dep_check.py (`:531-544`/`:634-635` transient demotion, `:636-655`/`:599-616` post-skip re-resolution); consumer-expansion ground `:197,220,264`.

## fn complete_walk_keeps_the_masks_the_first_walk_settled_under
- Backlog numbers removed from the comment (original text): The complete-graph walk starts from the masks the first walk settled under. Bare `emerge -pv virtual/p651th`: Portage 3.0.83 (host, on a copy of the fixtures) masks `p651prov-5.44.0`, then `p651th-3.520.0` (its `=p651prov-5.44*` arm is left unsatisfiable), and reinstalls `p651th-3.500.0-r1`, with or without `--complete-graph`. The slot conflict met while backtracking turns complete mode on (#602), and Portage runs `_complete_graph` in the same depgraph, under that `runtime_pkg_mask` (`depgraph.py` `_resolve_conflicts`). A complete walk without the masks re-picks `p651th-3.520.0` as an upgrade: the bed's `virtual/perl-ExtUtils-MakeMaker` regression of `l0-20261107T041916Z`.
