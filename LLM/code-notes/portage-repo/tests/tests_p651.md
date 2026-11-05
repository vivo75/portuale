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

## fn chained_holder_skips_the_transient_and_holds_the_installed_binding
- C1 leg: bare `[p651prov, p651top]` (requested-through holder). See the
  test docstring for the oracle (`probe651_oracle.py` C1:
  `['dev-perl/top-1', 'dev-lang/perl-5.42.2']`, WARNING-proven skip).
  Unaffected by #656 leg (a) (same rows post-gate: the holder/top plain
  pins already skip; the virtual-arm pins join them consistently).
