# batch-2026-09-26 close-out ledger (coordinator facts, 2026-09-27)

> Copied 2026-09-27 (batch-2026-09-27 H0 step 4) from the gitignored
> `.superpowers/sdd/batch-2026-09-26/closeout-ledger.md` so the residue entries it
> backs (#178–#188, #190) point at tracked evidence. Content unchanged below.

All shas below are reachable from `main` in the named repo. Beds ran through the owner-approved
`podman-lxc` shim; controls re-established under it: small-case oracle `l0-fx-20260926T215430Z`
(probe-identical to old host), L0 `l0-20260926T215525Z`, `l31` control `l31-20260926T220542Z` 0/0,
`l32` control `l32-20260926T220733Z` 0/0 all 7 cells. Integration gates on `main`: gate2
(1517/0 cargo, suite 1957 + 4 known), gate3 (1605/0, suite 1957 + 4 known), combined glibc+bash
gate `l1-20260927T055315Z` green; gate 4 + P-C3 pending (see bottom).

## Items to flip DONE (date 2026-09-27 unless noted)

| # | what | portuale merge | pmtest merge | gate / beds / evidence |
|---|---|---|---|---|
| 157 | keyword-masked installed → best visible (fix `a8014af5`, merged 09-25) | (09-25) | pin `8a7b111` (09-25) | Z0 L0 `l0-20260926T215525Z`: `MULTI_emptytree-system` version+flags rows gone; corpus bless `aec53e6` (9 `--emptytree` rows, real `depgraph.py:7888-7899`) — fix is broader than the entry: it fixed `-e` installed selection generally |
| 158 | OWNER of byte-identical helper-managed files (`awk.1`) (fix `72eba1e7`, 09-25) | (09-25) | (09-25) | merge-path gate on `main` `l1-20260926T220155Z` green (Z0 step 2); `info/dir` half split to #176; l3-core row check → P-C3 |
| 160 | (already DONE at Z0) | | | |
| 162 | lib.rs cluster 2 tests | `3042fdaf` | `9188ab3` | 68 killed, 2 proven equivalent |
| 163 | lib.rs cluster 3 tests | `08ed2ab2` | `39fae63` | 74 killed; 1 equivalent, 1 unreachable |
| 164 | lib.rs cluster 4 tests | `9d6483a7` | `3969750` | 73 missed → 1 equivalent |
| 165 | lib.rs cluster 5 tests | `92c55577` | `d9d9ee8` | 70 killed; timeout 7627:12 inspected (infinite loop on cyclic input), not pinned around; review caught one false "equivalent" → killed |
| 166 | lib.rs cluster 6 tests | `c5af5193` | `3b466a4` | 56 → 0 missed; surfaced #184 |
| 167 | preserved_libs_registry like real | `2d2246ec` | — | gate `l1-20260926T224350Z`; l32 `l32-20260926T230337Z` SIZE row gone C1–C4+F2, C2 content equal |
| 168 | resume list saved up front, shrunk per merge | `8f60c891` | — | gate `l1-20260926T224828Z`; C4 proof on probe main+#168+#177 `l32-20260927T052653Z` and on `main`-equivalent #177 tip `l32-20260927T061016Z` (resume key present, `--resume` rc 0, 0 unexplained) |
| 169 | mrg keeps setuid/setgid/sticky | `171f2abb` | — | gate `l1-20260926T223731Z`; l31 `l31-20260926T225952Z` `[MODE]` rows gone |
| 172 | mrg exports MERGE_TYPE | `33637422` | — | gate `l1-20260926T224047Z`; l31 `l31-20260926T230107Z` phase.log SIZE row gone |
| 173 | BINPKG_FORMAT default gpkg + config chain (owner N1) | `fc600a75` | `aeb20bd` | not merge-path; `main` l32 `l32-20260926T231851Z` F2: the #173 row gone |
| 174 | binpkg size/digest verified at merge (index-trusted scan) | `7f039eaf` | `e2eb820` | gate `l1-20260927T074345Z`; l32 F2 `l32-20260927T074644Z` 0 unexplained, corrupt-merge log byte-equal from the digest block on |
| 175 | binhost fetch failure non-fatal | `de555da2` | `a278174` | gate `l1-20260927T065535Z`; l32 F3 `l32-20260927T070047Z` 0 unexplained |
| 176 | post-emerge GNU info regeneration, full port (owner N3) | `42bf7afe` | `02918af` | gate `l1-20260926T225636Z`; l3-core `info/dir` OWNER row → P-C3 |
| 177 | NEW: real `(N of M) cpv::repo` progress lines, Installing after build | `8ef5de3d` | `878f5a7` | gate `l1-20260927T060519Z`; l32 C4 `l32-20260927T061016Z` |
| 184 | NEW: direct-solve shadowed instance (enabled, declared) swap | `827924e9` | — | oracle `l0-fx-20260927T073759Z` + L0 `l0-20260927T073846Z` probe-identical to `main` |
| 52 | metamorphic widening (P-O3 step 1) + Track U (#161–#166) → flip DONE | — | `991414a` | 210 passed, 35 validity skips, 0 divergences |
| 50 | DONE-PARTIAL update: batch 2 `test_circular_dependencies` | — | `4c5f424` | 8 CASES, oracle = real ResolverPlayground on host |
| 31/32 | DONE (owner Q2) — already flipped `5cc8b641` | | | |
| 30 | l3-core re-run P-C3 → DONE (owner confirmed) | — | — | `l3-20260927T080452Z`: candidate 347/345/2 (was 520/346/174), control 0 unexplained; the 2 rows are #159-residue-class SIZE rows over tolerated payload (python, icu), folded into P-Z notes per owner Q1 |
| Q6 | gpkg tests set BINPKG_COMPRESS in make.conf (real probe confirmed) | — | `bf0692f` | real 3.0.82.2: env gzip → .zst, make.conf gzip → .gz |
| 129 S0 | R4 answered (backtrack trial state) | docs `edff1111` | — | `l0-fx-20260927T080515Z` |

## Not done (stay open, say why)

- **#170 / #171** — on branches `backlog/170-remote-vdb-aux` / `backlog/171-remote-env-regen`
  (stack tip `2d4fbd7a`), bed-verified (`l31-20260927T061805Z`: env equal except FEATURES), **held
  for owner Q10** (client profile resolution on the server).
- **#129, #135, #25, #107, §A** (Track R) — not started this batch.
- **#49** (P-O1) — not started.

- **Q6 branch** `backlog/q6-gpkg-compress-tests` (pmtest `596a03a`) — held for a real probe.

## New residues to file (numbers reserved; re-scan all branches first)

- **#178** #167 residue: real `register()` overwrites the `cp:slot` record at the new package's
  treewalk; portuale re-attributes the old entry → a second consecutive soname bump may keep a
  stale path list.
- **#179** #168 residues: unit pin for "no resume write on `--pretend`/`--ask`-declined"; an
  all-noop plan leaves a stale 1-item resume (real assigns unconditionally).
- **#180** #173 residue: `PKGDIR`, `BINPKG_COMPRESS*`, `PORTAGE_BZIP2_COMMAND` still env-only at the
  same call sites.
- **#181** #50 residue: circular-dep "solution attribution" text needs a real-text oracle.
- **#182** pmtest: `loopback_sshd` fixture leaks its `sshd` when a run is killed.
- **#183** portuale writes no real-style `-MERGING-<pf>` in-progress vdb entry during a merge.
- **#185** a non-pretend `emerge` prints the merge-list line (`[binary N] …`) where real prints
  `Calculating dependencies ... done!` (+ the repo news-count notice) and no list.
- **#186** #177 residue: a resumed *binary* entry's `::repo` shows the ebuild repo, not the binhost.
- **#187** #175 residue: case (a) abort wording (`Tried to use non-existent binary …`) is portuale's own.
- **#188** #174 residues: portuale-written index stanzas lack `SLOT`; fixed digest-key list.
- **#159 residue** (vdb `SIZE` hard while file rows are tolerated payload) — owner Q1: **fold into the P-Z notes** (no number). P-C3's 2 remaining rows are this class.
- **#189** the `l31` reference side runs the container's portage 3.0.81.3, not the 3.0.82.2 pin (bed version skew; last #171 env FEATURES row).
- **#190** real dies in `src_install` on fixture `dev-libs/packagepkg` (no sources, no `S=${WORKDIR}`: "The source directory '${S}' doesn't exist", `phase-functions.sh:638`); portuale builds it — S handling diverges.
