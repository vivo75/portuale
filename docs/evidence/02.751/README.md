# S0 evidence for #751 — the kept-virtual cut probe (verdict A, Path A, 2026-11-14)

Backlog #751: a kept `virtual/*` package whose installed provider is broken
would order early instead of waiting for its rebuild, because `build_time_used`
(`crates/portage-world/src/order.rs`) excludes `virtual/*` targets and the
virtual's provider edge stays cuttable. Plan `LLM/02.751-kept-virtual-cut.opus.md`
§3 builds the shape on purpose and lets the checker judge it.

**First capture: NEEDS_CONTEXT.** The plan's literal R/B/R topology
(`user -B-> virt -R-> prov -R-> user`) cuts `(prov,user)` — first cuttable
edge in cycle order, cuttable with or without the exclusion — so the
`virtual/*` rule was never consulted and both variants stayed
clean-but-excused (`cycle-noted` via the #745 second build). See "First
capture" below; the authorised amendment (§3 step 3a) adds the condition
that makes the exclusion load-bearing.

**Amended capture: verdict A.** With `dev-libs/w751base-2` gaining
`BDEPEND dev-libs/w751prov`, `prov` is build-time-used, `(prov,user)` is
uncuttable, and the cut moves onto the virtual's provider edge
(`cuts-amended.txt`: `Rebuild prov over [base, prov]` for the bought
`base <-> prov` pair, then `BackEdge virt -> prov` — the back edge because
the walk closes the cycle on `prov` through it; the control instead
rebuilds its `dev-libs/` middle, cutting nothing). Orders
(`probe-virt2.stdout`): `[user-1 N, user-1 N, prov-2 U, base-2 U, prov-2 U]`,
kept `[virtual/w751virt-1]` — `prov` twice (charter-2a breaker), `user`
twice (usability closure). Checker (`probe-virt2.check.json`): 17
findings, all fixture-tree background `pre_existing`, **zero
non-`pre_existing`, zero `cycle-noted`, zero excuses**. The control
(`probe-mid2.*`) is verdict-identical (backgrounds file-identical). The
consumer builds while the installed provider copy is still usable (base
upgrades after it), so the broken-at-build-time shape is unexhibited:
the hole is theoretical, pinned by two tests (Path A §6), no product code.

## First capture (superseded shape, NEEDS_CONTEXT — kept as the amendment's rationale)

**Verdict: NEEDS_CONTEXT — the probe cannot fail either way, so it attributes
nothing.** Two independent STOP reasons from the plan both fire:

1. The cut never lands on the kept virtual's provider edge (§0 STOP 1:
   "the walk never follows the kept virtual's provider edge" — here the walk
   follows it but the cut never takes it). Temporary `runtime_cut` dump
   (`cuts.txt`, `eprintln!` reverted before close-out, no product change):
   `Edge dev-libs/w751prov:0 -> dev-libs/w751user:0` on **all three** probe
   trees. `cycle_cut` takes the *first* cuttable edge in cycle order from the
   ancestor (`(prov,user)` before `(user,virt)` before `(virt,prov)`), and
   `(prov,user)` is cuttable with or without the exclusion (prov is never a
   build-time-used target here). The `virtual/*` exclusion is never consulted.
2. The verdict is clean only through the #745 second-build excuse (§0 STOP 3
   verbatim). The planner duplicates the consumer and the checker notes it:
   exactly one non-`pre_existing` note, `cycle-noted` on `dev-libs/w751user-1`
   ("met only after an identical rebuild of the same package"), zero
   failures. That is not verdict A (an excuse is present) and not verdict B
   (no `build-order` failure naming `w751prov-1` or the `virt→prov` edge).

**The positive control is indistinguishable.** The identical graph with a
`dev-libs/`-named middle (`probe-mid.*`) gives byte-identical row orders and
a verdict differing only in the middle's atom text (`BDEPEND
dev-libs/w751mid` vs `BDEPEND virtual/w751virt`). The middle's category —
the only thing the exclusion looks at — changes nothing observable.

### First-capture orders and verdicts (all four trees)

- Base installed at the new subslot (= the w746 precedent: base-2 kept):
  rows `[dev-libs/w751prov-2:0 U, dev-libs/w751user-1:0 N,
  dev-libs/w751user-1:0 N]` (`probe-virt.stdout`), kept
  `[dev-libs/w751base-2, virtual/w751virt-1]`. Checker: 18 findings, 17
  fixture-tree background `pre_existing`, plus the 1 `cycle-noted` above.
- Plan-literal base (visible only, no installed copy): rows
  `[dev-libs/w751base-2:0 N, dev-libs/w751prov-2:0 U, dev-libs/w751user-1:0 N,
  dev-libs/w751user-1:0 N]` (`probe-virt-nobase.stdout`), kept
  `[virtual/w751virt-1]`. Checker verdict identical to the base-kept variant
  (file-identical modulo order numbers).
- Control (`dev-libs/w751mid` middle): same rows and verdicts as its
  `virtual/` twin in both base variants.

## Amended capture files

- `probe-virt2.stdout` / `.json` / `.check.json`: rows `[user-1 N, user-1 N,
  prov-2 U, base-2 U, prov-2 U]`, kept `[virtual/w751virt-1]`; checker 17
  findings, all `pre_existing`, zero non-`pre_existing`, zero notes.
- `probe-mid2.*`: the `dev-libs/`-named control — same rows, verdict
  file-identical modulo the middle's atom text.
- `cuts-amended.txt`: the temporary cut dump (variant: `Rebuild prov` then
  `BackEdge virt -> prov`; control: `Rebuild prov` then `Rebuild mid`,
  cutting nothing — the exclusion off).
- `s0-log.md`: the scratch log, including the pre-run trace that predicted
  the first capture's cut.

## Reproduce (capture-only, no product code)

`mkprobe.sh <dest> <virt|mid> <kept|nobase>` rebuilds a probe tree from the
fixtures copy exactly as `check_world.rs` `add_ebuild`/`add_installed` do
(ebuild + sorted md5-cache entry with a real md5; VDB `CATEGORY`/`SLOT`/`EAPI`
/`RDEPEND`), appends `dev-libs/w751user` + `dev-libs/w751prov` to the world
file; `AMEND=1` builds the §3-step-3a variant (base-2 gains
`BDEPEND dev-libs/w751prov`, base installed at `0/1` instead of kept at
`0/2`). Then (frozen release binary, no source change):

```
portuale mrg --plan-world --sort depend --root <dest> --config-root <dest>
portuale mrg --plan-world --json --sort depend --root <dest> --config-root <dest> > plan.json
python3 ../pmtest/pytests-contract-suite/health_checker.py --vdb <dest>/var/db/pkg plan.json
```

Fill-in choices the plan leaves open, recorded here: the prov-2 ebuild carries
`RDEPEND "dev-libs/w751base dev-libs/w751user"` and the installed prov-1
records `RDEPEND "dev-libs/w751user dev-libs/w751base:0/1="` (the w746loc
idiom verbatim: plain dep plus stale `:slot/sub=` atom); the base-kept variant
additionally installs base-2 (the stale-binding target at the new subslot, as
`kept_middle_tree` does). Both base variants agree, so the choice is immaterial.

## Consequence

The amended probe routes through the exclusion: with `prov` build-time-used,
the cycle's only cuttable edge is the virtual's provider edge
(`BackEdge virt -> prov`), and the checker stays clean because the consumer
builds while the installed provider copy is still usable. The R2 residue
proposed after the first capture (a `virtual/`-category twin for the
`dev-libs/w74Xvirt` pins) is moot: the new `order.rs` unit test
(`the_virtual_provider_edge_is_the_only_cuttable_edge`) pins the exclusion
directly — `is_cuttable_runtime_edge` false for `(prov,user)` and true for
`(virt,prov)` — so the exclusion now has a unit-level pin either way #751
closes. No residue filed.
