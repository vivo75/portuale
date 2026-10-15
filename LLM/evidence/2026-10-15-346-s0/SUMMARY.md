# S0 SUMMARY — 02.346 ebuild/config/options parity oracle (slice S0 only)

104 scenarios × 2 PMs (240 capture quads + stage/log/find sidecars; 1015 files),
all under `LLM/evidence/2026-10-15-346-s0/`. Scripts: `scripts/{lib,u,k,b,e,s,m,a,c,g,l,run-all}.sh`
(one capture script per family as planned; `run-all.sh` replays everything).
No product code touched, no fixtures added (synthetic packages live in scratch
overlays), no commits/branches/pushes. Binary `target/release/portuale`
(`f221c03f`, mtime 2026-10-15 19:18:07 UTC) never rebuilt. Full provenance:
PROVENANCE.md. Cell→verdict→slice table: matrix.md (normative).

## Verdicts vs plan predictions (§1 table)

Matches prediction (proceed as planned): 346-UP→S1 (u01–u09, u11–u13, u16–u19),
346-KW→S2 (k01–k05), 346-BID→S3 (b01–b06), 346-LIC stale, 347-GB→S9 (g01–g07),
347-PX→S9 (g07 + `depgraph.py:5591-5592` verified in 3rdparty),
347-ERR→S8 (c01, c03–c14 + D4 byte-exact), 347-USG cut (c12 seed/noseed md5s),
349-TMP/DIST/FEAT/USE/ARCH/CFG/REPO→S4 (e01–e08), 351-BIN Q5→S4 (e06),
351-STAMP→S5 (s01–s09), 351-MARK/DIE→S6 (m01–m09), 351-FT stale,
351-LIST→owner S10 (l00–l05 full oracle), 351-CLEAN→owner S11 (cl01–cl02, cl01b–cl01c),
351-NEWS/UPD cuts (g00/l00 are the cut text), 346-X→#371–#373 (filing only).

Contradicts prediction (owner attention before S1, per S0 "Done when"):
1. u10: `profile/eapi=5` wildcard warning prints **once**, plan said twice.
2. A-family: plan §0.4 "portuale's stdout is empty" on masked aborts holds ONLY
   for the top-level all-masked arm (a01–a04: 0 B). Dep-level aborts (a05–a12)
   already print `Calculating…` (82–139 B). **S7 scope narrows to the all-masked
   arm**; Q6 default ("non-pretend only") looks too narrow — even `-p` prints it.

New behaviour (not in plan, same owner list):
3. u14: user `package.unmask` DOES counter a chain mask in portuale today —
   S1 must preserve it (plan §0.1.1 says the flat reads "have no effect").
4. u15: no `make.profile` ⇒ Portage aborts the run (rc 1, invalid-profile),
   not merely "ignores the user profile".
5. c02: `-j x` is not a jobs error on either side (`x` becomes an atom, rc 1) —
   S8 must shape `-j` from this cell.
6. e07: portuale has no `--fetchonly` (rc 2) — 349-DIST's emerge half needs a
   vehicle decision in S4.
7. l04: portuale prints a 5-second countdown despite `CLEAN_DELAY=0` (ignores
   the key — S4-adjacent) and the merge listing is **direct-path-only**
   (`ebuild merge`/qmerge list; `emerge` path never does at default verbosity —
   refines §0.4 "merge-engine-wide").
8. m08: portuale `--jobs=2` swallows most build output (442 B vs jobs1 862 B).
9. l01 portuale `ebuild merge` prints raw set-x traces and no listing at all.

## STOP RULEs

No S0 sub-part was stopped: every planned cell (U/K/B/E/S/M/A/C/G/L+CL) is
captured on both sides with full argv/provenance/verbatim output. The slice's
own completion rule ("every surprise goes to the owner before S1") is this
file + matrix.md §7-equivalents (items 1–9 above). S6's code STOP RULE, Q6/Q9/Q10
and the commit grant are downstream and untouched. No corpus/bless, no bed, no
merge-path gate (no product code changed).

## Follow-ups for the owner/coordinator

- Findings mirror for `../pmtest/differential-test-bed/findings/` is staged as
  `findings-mirror.md` (S0 worker writes portuale-side only; copy-over is a
  pmtest-side act).
- Suggested S1-first review order per plan §4: S1 → S4 → S5 → S6(code) → review.
- Scratch deleted (`/var/tmp/s346`, `/var/tmp/s346/probe`, `/var/tmp/dbg`,
  `/var/tmp/dbg-l`); host `/var/tmp/portage` restored to pre-run empty state.
  Repo tree verified clean except this evidence dir + sibling workers' dirs.
