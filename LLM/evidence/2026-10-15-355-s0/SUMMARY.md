# S0 SUMMARY — plan `LLM/02.355-reinstall-unmerge-parity.opus.md`, slice S0 only

Oracle: Portage 3.0.82.2 (host `emerge`/`portageq`). portuale at
`f221c03f53b69ac686dc270cbae995cfc329f40e` (binary mtime
`2026-10-15 19:18:07 UTC`, never rebuilt during the slice). ~29 Portage-vs-portuale
cells with the #354 harness (copied to `scripts/`, `t354`→`t355`). Full per-cell
rows in `matrix.md`; this file is the brief-required verdict list. No sudo used.
No product code, tests, backlog or plan files touched.

Provenance per run: `cells/<tag>.{argv,env,rc,stdout,stderr}` (tag
`<cell>-<pg|pl>-<seq>`), `<root>-{before,after}.snap`, `<root>.snapdiff`,
`*.contents` where noted. 894 files in `cells/`.

## Per-cell verdicts vs the plan's "expect" column

- **P1** — MATCHES prediction. Portage: `libt355mid.so.1` preserved
  (`>>> needed obj`), `libt355base.so.1` kept, registry holds both.
  portuale: `<<< !needed obj …/libt355base.so.1`, base.so.1 deleted, registry
  loses the base entry (mid.so.1 preserved under mid-1). #355 confirmed.
- **P1c** — MATCHES (control, both keep). **P2** — MATCHES (control, identical).
- **U1a** — MATCHES mechanism, CONTRADICTS the expect shorthand: Portage keeps
  `ign/x` (`--- cfgpro`) but removes `m.ko` — the literal `UNINSTALL_IGNORE`
  drops the make.globals default, exactly as the plan text (not the table)
  says. portuale removes both. Still discriminating.
- **U1b** — MATCHES (Portage keeps both `--- cfgpro`; portuale removes both).
- **U1d** — MATCHES (Portage keeps `m.ko` via make.globals default; portuale
  removes it).
- **U2** — not run (plan: source-only). Source verdict recorded in matrix.md.
- **L1** — MATCHES (Portage `--- cfgpro sym /lib`, `/usr/lib`; portuale removes).
- **E1** — CONTRADICTS the cell design (not the rules): Portage's *merge* backs
  the symlink aside (`link.backup.0000`), so `-C A` prints `--- !mtime sym` and
  never reaches R4/R4b; portuale's B-merge hard-fails (`File exists`, rc=1).
  E1-as-designed cannot pin S5 (see stop rule).
- **E2** — MATCHES (Portage `--- !empty sym` + full R4b elog + `<<< sym`;
  portuale removes the link by another route). Outcome same, mechanism differs.
- **E3** — MATCHES Portage side (`<<< sym/obj`, no elog); portuale same outcome
  via mtime arm. Outcome-coinciding, rule still absent (see stop rule).
- **E3b** — MATCHES Portage side (unlinked, no elog); CONTRADICTS the
  parenthetical (portuale prints no per-file unmerge lines at all, so no R4b
  elog is possible). Outcome same.
- **D1** — MATCHES (Portage `<<< dir` removes the symlink; portuale leaves it).
- **D2** — MATCHES (Portage `--- !empty dir` + R4b elog + `<<< sym`; portuale
  leaves it).
- **C1a** — MATCHES (`/bin` survives on both; S4 failing-first baseline holds).
- **C1b** — CONTRADICTS §0 text, capture wins: Portage REMOVES `/bin`
  (`<<< sym /bin` after the R4b elog) when no other package records it;
  portuale keeps it. Discriminating, opposite direction.
- **M1** — MATCHES (Portage `--- !md5` kept; portuale removed).
- **M2** — MATCHES (Portage `--- !md5` kept under `-unmerge-orphans`; portuale
  removed).
- **T1a** — MATCHES (Portage `--- !mtime` rc=0; portuale rc=1 EISDIR abort,
  stale vdb left).
- **T1b** — MATCHES (Portage `--- !obj` rc=0; portuale rc=1 EISDIR abort).
- **T2** — MATCHES (Portage `--- !sym` kept; portuale removes it).
- **T3a** — MATCHES, stronger than predicted (Portage orphan-unlinks the file;
  portuale rc=1 ENOTDIR abort, stale vdb — not merely "kept").
- **T3b** — MATCHES Portage (`--- !dir` kept); portuale keeps the file but via
  rc=1 abort + stale vdb. Outcome-coinciding, rule still absent.
- **F1a** — MATCHES (Portage `<<< fif` removed; portuale keeps).
- **F1b** — MATCHES Portage (`--- fif` kept); portuale keeps too
  (outcome-coinciding).
- **F2** — not run (plan: source-only; needs CAP_MKNOD). Source verdict in
  matrix.md (dev unlinked by orphan arm by default, kept under `-orphans`).
- **B1a** — MATCHES (Portage reinstall keeps both `--- !mtime/!md5`; portuale
  removes both). R8 confirmed on the reinstall path.
- **B1b** — MATCHES (Portage `-C` keeps both; portuale removes both). R8 on
  the `-C` path.
- **S1** — MATCHES (Portage removes all three space entries, decoy survives;
  portuale removes the decoy and leaks all three). `portageq owners` claims the
  package on both sides while installed (supplement `S1q-*`; the in-cell
  `S1-*-owners` ran post-`-C` and are superseded).
- **S2** — MATCHES (Portage `<<< obj a b.txt` via replace loop; portuale leaks
  it and removes the decoy).
- **S3** — MATCHES as oracle: Portage CONTENTS line
  `sym /usr/share/t355/arrow -> x -> y <mtime>`; portuale writes the same bytes.
  Pins the greedy-split input for the S3 unit test.

## Stop rule

FIRED in refine-and-continue form; no item premise is gone, so nothing stops
for the owner and no rule is dropped from S3–S5 (details in matrix.md §
"Stop-rule evaluation"):

1. E1-as-designed does not reach R4/R4b on Portage (merge-time backup aside);
   S5 needs a redesigned "another owner" probe. E1-portuale is additionally
   blocked on an out-of-scope merge-through-symlink bug (`EEXIST`).
2. C1b: Portage removes `/bin` with no other recorder — beyond §0 text.
3. Outcome-coinciding rules (E3/E3b, F1b, T3b, C1a) STAY: each Portage rule is
   still absent in portuale by mechanism (hard errors, F1a, missing elog
   machinery), not just by outcome.
4. Text corrections to carry: U1a shorthand, E3b parenthetical, and the #354
   matrix fif/dev "Not established" line is now established (F1a/F1b + F2
   source verdict).

## Out-of-scope observations (reported only)

1. portuale `dobin` fails unprivileged merges (chown EPERM); worked around with
   `doexe` (identical images; P cells re-run on both sides).
2. portuale merge through a live dir symlink → `File exists (os error 17)`,
   rc=1 (`E1-pl-2`).
3. portuale `emerge -C` prints no per-file `<<<`/`---` lines (02.346 351-LIST
   family).
4. portuale vdb dirs lack `NEEDED`/`NEEDED.ELF.2`; preserve-libs still worked
   (fs-scanned linkmap).

## Cleanup

`/var/tmp/t355` removed after the captures were verified (see final report).
`scripts/ebuilds/` + `scripts/setup.sh` rebuild the overlay deterministically.
