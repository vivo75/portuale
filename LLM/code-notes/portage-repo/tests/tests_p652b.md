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
- Backlog numbers removed from the comment (original text): Leg A (N-form, RED): the Try-Tiny-shaped consumer over the installed dead SLU virtual with the chained holder in the request closure. Portage (NF oracle + texlive bed cell) holds the virtual: no Sub-Name merged, the consumer and driver merge fresh, the top reinstalls under the holder pin. Portuale latches the pre-skip flat choice and merges Sub-Name.  Posture note (deliberate, bed-grounded): portuale upgrades the provider outright on this indirect shape where Portage/C1 skip it (the C1-bare selection-side divergence family, out of #652's scope -- the transient selection is untouched by the consumer re-pop). The test therefore pins the holder pressure that DOES render (top reinstalls) and leaves the provider outcome unasserted, so the fix turning the consumer hold green cannot break this test for posture reasons.  RED before Task 2 at the no-Sub-Name assertion.

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
- Backlog numbers removed from the comment (original text): Leg B (H-form): the requested early TH consumer over the bare provider. The fixture vdb grew three installed `=5.42*` holders under Task 1 (`p656hold{1,2,3}`), so this world renders the #656 leg-(a) holder shape -- and Portage on exactly this shape reinstalls: H1 (`probe656h_oracle.py`: `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.42.2', 'dev-perl/s652early-1']`, no skip, no fall-throughs). The (a) gate files the kept virtuals' arms, the mask lands 5.44, the seeds void, and the transient reinstalls beside the requested early consumer with neither pin merged. Re-pinned 2026-11-05 (was: transient-U + Encode-N, the tolerate+keeper render that predates the holders).  Bed-divergence note: the real libreoffice/gimp cells still merge U + Encode (their requested closure carries versioned forcers the hermetic miniature lacks, and leg (b)'s keeper/posture holds that outcome -- the Task-5 bed re-grade covers it). The miniature and the bed cell are different shapes in Portage itself (H1 = R vs bed = U); this pin tracks the miniature's own oracle, not the bed's.
- Backlog numbers removed from the comment (original text): Holder-win posture: the transient reinstalls (H1 oracle mergelist; the #656 leg-(a) enforcement-open mask, not the selection-side U).

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

## module
- Backlog numbers removed from the comment (original text): #652 leg A (N-form consumer path) + leg B (H-form upgrade hold): RED pins.  S0 #652 (`LLM/evidence/2026-11-04-652/`, this branch) refuted the novel-demoter corner and rescoped #652 to two live shapes, both with a committed Portage-3.0.83 oracle:  - Leg A (N-form): Try-Tiny binds the dead installed SLU virtual and the choice latches pre-skip; Portage re-resolves post-skip and holds the virtual (no `dev-perl/Sub-Name` merged). Bed ground truth: L0 `l0-20261104T075942Z` real `app-text_texlive.txt` (and the XML-Parser, kdecore-meta, texlive-core, texlive-latex cells) carries NO Sub-Name row -- the 5 Sub-Name `[extra]` rows are portuale-only. Playground oracle (`LLM/evidence/2026-11-04-652b/probe652b_oracle.py` NF): `SUCCESS True`, `MERGELIST ['virtual/perl-s652foo-2.223.0-r1', 'dev-perl/s652try-1']` -- the driver schedules 5.44 (its `||` first resolves `=perl-5.44*`), the walk skips it (re-resolves to `=perl-5.42*` verbatim in the trace), the consumer `||` keeps the virtual conjunction over Sub-Name throughout. Portuale latches the pre-skip flat choice (the #643 inline expansion binds the dead arm) and never re-pops it, so it merges Sub-Name where Portage holds. - Leg B (H-form): under the LIVE merging transient the seed feed fires both fall-throughs where Portage merges Encode but holds TH. Bed ground truth: L0 `l0-20261104T075942Z` real `app-office_libreoffice.txt` merges `perl-core/Encode-3.210.0` (row 33) and carries NO `perl-core/Test-Harness` row and NO `virtual/perl-Encode` or `virtual/perl-Test-Harness` rows (other fresh virtuals do merge; gimp identical: Encode row 22, no TH row, perl U). Portuale fires both seeds (demoter `p651prov-5.44.0` survives) and merges both pins.  The `p652*` fixture family extends the #651 C1 substrate hermetically (same `p651prov` provider axis + `p651enc/p651th` virtuals + `p651enccore/p651thcore` pins + installed `p651holder/p651top/p651pin` background pressure; reused where the shape fits, forked where the new legs require it): - N-form: `virtual/p652slu-1.680.100_rc` (installed; bed SLU-virtual text family: 5.42 arm + `~` pin at a version with NO core ebuild -- the dead arm) + `dev-perl/p652subname-0.280.0-r1` (fall-through leaf, like the `p651*core` leaves) + `dev-perl/p652try-1` (fresh; Try-Tiny-shaped `!minimal?` consumer `||` + bare provider dep) + `virtual/p652foo-2.223.0-r1` (fresh-only driver virtual with a 5.44-first-arm `||`, pulled by the consumer -- the collapse of the live fresh-virtual driver chain into one edge, documented here). - H-form: `dev-perl/p652early-1` (fresh; bare `virtual/p651th` dep -- the libreoffice-cell early TH-parent chain in one edge) requested ahead of the bare provider. Everything else is C1 verbatim.  `minimal` defaults OFF by profile-chain absence (C1 precedent), so the `!minimal?` blocks are live.  Portage: dep_check.py bins (`:462-467` replacing skip, `:531-544` installed_downgrade, `:622-626`/`:696-703` preferred ladder, `:634-635` downgrade guard, `:392-402` bin order, `:745-800` in-bin upgrade promotion); consumer-expansion ground `:197,220,264`. The (b) bin record: under an identical posture both fall-through arms land in identical bins (perl arm `other` via the guard, pin arm `preferred_non_installed` wins) -- the live split comes from evaluation posture (self-parent re-eval vs transient-parent replacing-eval), full analysis in the Task-1 report; no arm-level bin distinguishes TH (the brief's Step-1 STOP premise, flagged there explicitly).
