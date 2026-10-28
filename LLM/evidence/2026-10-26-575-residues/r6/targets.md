# R6 (#600) targets, written before the L0 run (2026-10-28)

Rule and RED list: s0.md ("The rule", "RED list"). Printed count = Portage's
`backtracked`: rule A (failed walk counts, also at the cap), rule C (a restart
whose children are all rejected still counts), rule B (a grow `Config` that
Portage would not restart for does not count).

## Must move to Portage's count
slotconflictunsolvable/slotconfgroup/scuseparent 6/20; slotusegroup 4/20;
ormissbad and abort-masked-{mid,last,cycle}, abort-unsat-mid 1/20; G0 1/20 and
`--backtrack=1` 1/1; needer/othermod triangle 4/20; k3 3/20; needer keeper 4/20;
igw 3/20; cgp VER 3/20; B cells `--autounmask-backtrack=y r594vdep r594qa` 0/20.
## Must not move
bt-y-abc 3/20; `r6b r6a` with `=y` 1/20; r594etop 0/20; every rc, row and block
(only the `(backtrack: N/M)` line may change).
## Corpus
About 255 rows move +1 (`corpus-impact.tsv`); bless only after checking that the
only changed line is the count. 244 equal a Portage count. Exceptions to record
as residues, not to hide: the 6 `--debug` rows (Portage prints no count line) and
`-uD(N) dev-libs/omasked` (portuale rc 1 vs Portage rc 0: a separate divergence).
## L0
L0 does not compare counts: no cell may move at all (baseline
`r3/l0-after.txt`, `l0-20261028T011350Z`). Any move is a stop.
