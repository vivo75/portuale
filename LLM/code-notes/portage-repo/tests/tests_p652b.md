# crates/portage-repo/src/tests/tests_p652b.rs
#652 leg A (N-form consumer path) + leg B (H-form upgrade hold): RED pins.
Backlog #652 (+ Update note); plan `LLM/02.652-postskip-reresolve.opus.md`
Task 1 product; S0 `LLM/evidence/2026-11-04-652/`; Task-1 oracle
`LLM/evidence/2026-11-04-652b/` (probe + verbatim `.txt`).

## fn nform_consumer_holds_the_virtual_no_subname
- N-form (leg A) fixture: C1 substrate (`p651prov` axis + `p651enc/th`
  virtuals + pins + installed `p651holder/p651top/p651pin`) + `p652slu`
  (installed dead SLU virtual, bed-text family) + `p652subname` (leaf
  fall-through) + `p652try` (fresh Try-Tiny-shaped consumer, pulls prov
  bare + the `p652foo` driver) + `p652foo` (fresh-only driver virtual).
  Request `[p651top, p652try]` (holder pressure first so the holder
  conflict is open before the consumer subtree resolves).
- Oracle: playground NF (`SUCCESS True`, mergelist `[s652foo, s652try]`
  -- driver falls back `=perl-5.44*` -> `=perl-5.42*` mid-walk, consumer
  keeps the virtual conjunction throughout) + bed texlive cell (real: no
  `dev-perl/Sub-Name` row; the 5 Sub-Name `[extra]` rows are portuale-only).
- Portuale (current): merges Sub-Name (RED at the absence assertion);
  posture assertions that pass now: top R, try/foo N, virtuals kept.
  Deliberately unasserted: prov U (portuale upgrades the indirect
  provider where Portage/C1 skip -- C1-bare selection-side divergence
  family, out of scope) and the seed pins (C1-bare GREEN scope).

## fn hform_upgrade_holds_th_while_encode_merges
- H-form (leg B) fixture: C1 substrate verbatim + `p652early` (fresh,
  bare `virtual/p651th` dep -- the libreoffice-cell early TH-parent
  chain collapsed to one edge). Request `[p652early, p651prov]`
  (transient merges, no skip).
- Oracle history: the BED first (playground cannot render the hold -- see
  the (b) record below): real libreoffice row 33 merges
  `perl-core/Encode-3.210.0` and carries NO `perl-core/Test-Harness` row
  and upgrades neither `virtual/perl-Encode` nor
  `virtual/perl-Test-Harness` (other fresh virtuals do merge; gimp
  identical: Encode row 22, no TH row, perl U). Pre-#656 portuale merged
  `p651thcore` (RED at the absence assertion); everything else already
  matched the bed (prov U, enccore N, virtuals kept, early N), so the
  failure isolated the TH hold exactly.
- Re-pinned 2026-11-05 (#656 leg (a)): the fixture vdb grew three
  installed `=5.42*` holders (`p656hold{1,2,3}`), so this world renders
  the holder shape -- and Portage on exactly this shape reinstalls: H1
  (`probe656h_oracle.py`, same evidence dir: `SUCCESS True`,
  `MERGELIST ['dev-lang/perl-5.42.2', 'dev-perl/s652early-1']`, no skip,
  no fall-throughs). New pin: prov R 5.42.2 + early N + no pins + no
  virtual U/D. The miniature and the bed cell diverge in Portage itself
  (H1 = R vs bed = U: the bed's requested closure carries versioned
  forcers the miniature lacks); this pin tracks the miniature's own
  oracle, the bed cell stays covered by leg (b) + the Task-5 re-grade.

## (b) bin record (upgrade posture, Portage 3.0.83 dep_check.py)
- Under an IDENTICAL posture both fall-through `||`s resolve identically
  (no arm-level bin distinguishes TH from Encode -- the brief's Step-1
  STOP premise, established with traces; the asymmetry premise itself is
  bed-proven, only the bin-level hypothesis is refuted):
  - perl arm (`=dev-lang/perl-5.42*`): `other` via the downgrade guard
    (`:634-635`), because `installed_downgrade` fires (`:531-544`:
    highest-in-slot `dev-lang/perl:0` is 5.44.0, in graph via the merging
    transient, avail 5.42.2 is lower and not downgrade-probed).
  - pin arm (`~perl-core/*`): `preferred_non_installed` (`:696-703`:
    avail is the tree pin = highest, not installed, not in graph) -- wins.
- The same `||` evaluated with parent=merging-transient
  (virt_parent=installed virtual) resolves the OPPOSITE way: `replacing`
  is true (`:462-467` `will_replace_child`: transient 5.44 replaces
  5.42.2 for slot 0), so the guard is skipped and the perl arm lands in
  `preferred_in_graph` (`:696`, installed 5.42.2 satisfies), beating the
  pin's `preferred_non_installed` -- hold, no merge.
- Bin order (`:392-402`): preferred_in_graph (=installed=any_slot, same
  list), preferred_non_installed, unsat_use_*, other_installed*,
  other; in-bin "prefer upgrades" promotion (`:745-800`); first
  available bin wins (`:813+`).
- The live split (Encode merges, TH holds on the same cell) comes from
  evaluation posture, not arm bins: Encode's sticking evaluation is the
  self-parent re-fire post-5.44 (pin wins); TH's is the early
  pre-5.44 self-eval latching the perl arm (preferred_in_graph, guard
  inapplicable with 5.44 absent) plus the transient-parent replacing-eval
  (perl wins again) -- `existing_node` stickiness means the pin edge is
  never added for TH. Playground B-series (scratch, not committed):
  installing newer virtual texts + visible KEYWORDS still upgrades both
  (slot-op `@__auto_slot_operator_replace_installed__` set with
  force_reinstall skips installed); dropping the newer virtuals removes
  the restart but reinstalls both olds with both pins (eliminate/rebuild
  keeps them: tree `:=` evaluates to `:0/5.44=` vs installed `:0/5.42=`);
  entry-order-only shapes fail on the unresolvable perl:0 conflict (no
  backtrack feedback without a newer replacement). The hermetic Rust
  fixture does not need the playground to render the hold: it reproduces
  portuale's over-fire (seed feed fires both under the live merging
  transient -- S0 cell traces) on a bed-faithful shape, with the bed as
  the hold oracle.
- Validity traps hit while building the playground (for the next agent):
  the probe-form `!>` (`!>...-r999`, single suffix) is REQUIRED -- the
  true-tree double suffix (`!>...-r1-r999`) is an invalid atom that masks
  the installed package and flips version selection to NEW; the
  playground profile is x86-stable so dicts need `KEYWORDS="x86"` (the
  default `~x86` masks everything and the parent=None expansion race then
  picks NEW); tree ebuilds carry `:=`, installed copies `:0/5.42=`.
