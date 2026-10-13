# S0.4 + S0.5 (worker `b`): phase environment and phase sequence/skip captures

Plan: `LLM/02.348-eapi-phase-audit.opus.md`, Task S0 (captures + inventory only;
no product code, no change to existing fixtures).
Oracle: host Portage 3.0.82.2 (`/usr/lib/portage/python3.14/*.sh`,
`/usr/lib/python3.14/site-packages/portage`, `/usr/lib/python-exec/python3.14/ebuild`).
EAPIs in scope: 7 and 8.
portuale binary: `/home/vivo/repo/PORTUALE/portuale/target/release/portuale`
(prebuilt; never recompiled — shared with other workers).

## Probes (new fixtures, `../pmtest/fixtures/repo-348/`, prefix `b`)

- `app-misc/t348-b-env-7.ebuild` (`EAPI=7`) / `t348-b-env-8.ebuild` (`EAPI=8`):
  S0.4 probe. Every phase (`pkg_pretend`, `pkg_setup`, `src_unpack`,
  `src_prepare`, `src_configure`, `src_compile`, `src_test`, `src_install`,
  `pkg_preinst`, `pkg_postinst`, `pkg_prerm`, `pkg_postrm`) calls
  `_b_env_dump`, which writes `${T348_LOGDIR}/env-<EBUILD_PHASE_FUNC>.log`:
  `declare -p` of every variable of interest, then `declare -p` of all set
  `PORTAGE_*` vars, then `export -p` (sorted) and `env` (sorted), so
  exported-vs-shell-only is visible per variable. `src_install` installs
  `${T}/ver.txt` to `/usr/share/${PN}` (mergeable); `src_prepare` calls
  `eapply_user` (Portage requires it in every defined `src_prepare`,
  `phase-functions.sh:525`). No `SRC_URI` (no fetch).
- `app-misc/t348-b-phases-7/8.ebuild` (FULL): every hook appends its
  `EBUILD_PHASE_FUNC` to `${T348_LOGDIR}/phases-${PN}.log`; `src_*` call
  `default` (exercises `default_src_unpack` no-op on empty `A`,
  `default_src_prepare` with `PATCHES=( "${FILESDIR}/fix.patch" )`,
  `default_src_configure` via econf on the fake `FILESDIR/configure`,
  `default_src_compile`/`default_src_test`/`default_src_install` on the
  generated `Makefile`, `DOCS=( README.md )` + einstalldocs).
  `files/`: `configure` (records econf argv to `config.args`, writes a
  `Makefile` with `all`/`check`/`test`/`install` targets installing
  `built.txt`+`config.args` under `/usr/share/${PN}`), `hello.in`,
  `fix.patch` (`hello v1` -> `hello v2 patched`), `README.md`.
- `app-misc/t348-b-phaseslim-7/8.ebuild` (MINIMAL): defines only
  `pkg_setup`, `src_unpack`, `src_compile`, `src_install` (explicit
  `doins`, no defaults). NOT defined: `pkg_pretend`, `src_prepare`,
  `src_configure`, `src_test`, `pkg_preinst`, `pkg_postinst`, `pkg_prerm`,
  `pkg_postrm`.
- `app-misc/t348-b-phasert-7/8.ebuild` (= FULL + `RESTRICT="test"`),
  with its own `files/` copy (PN-adjusted `configure`).
- `app-misc/t348-b-envnoeapply-7.ebuild` (`EAPI=7`, S0.5 bonus): =
  `t348-b-env-7` but `src_prepare` WITHOUT `eapply_user` (must die).
- md5-cache entries for all 9 ebuilds generated with host
  `egencache --config-root /var/tmp/t348-b/cfg-portage --repo t348 --update
  app-misc/<pkg>` restricted to these atoms; `_md5_` verified against the
  ebuild bytes.

## Harness

- `scripts/b-capture.sh <portage|portuale>`: hermetic configroots
  `/var/tmp/t348-b/cfg-<pm>` (`repos.conf`: `testrepo` -> fixture `repo`,
  `overlay` -> fixture `overlay` prio 10 (needed by the fixture default
  profile's `parent`: `../base`, `../arch/amd64`,
  `overlay:crossrepo-parent`), `t348` -> fixture `repo-348` prio 50;
  `make.conf`: per-PM `PKGDIR`, shared `DISTDIR`, per-PM
  `PORTAGE_TMPDIR`, `T348_LOGDIR=/var/tmp/t348-b/stage`,
  `CLEAN_DELAY=0`, `FEATURES="-news"`; `make.profile` ->
  fixture `repo/profiles/default`), `ROOT=/var/tmp/t348-b/root-<pm>`,
  `PORTAGE_TMPDIR` additionally exported (same value; isolates D7).
  One cell = `<cell>.argv/.rc/.stdout/.stderr` + `<cell>.stage-*`
  (copies of `${T348_LOGDIR}` after the cell; stage cleared per cell except
  the explicitly cumulative `cell_noclear` chains). 64 harness cells per
  PM + 3 manual pairs (`-emerge-featurestest` full-7/FEATURES=test on fresh
  per-PM roots `root-ft-<pm>`; `-ebuild-test-featurestest`
  `FEATURES=test ebuild <f> test` + `<cell>.workdir-ls`; `-noeapply`
  `ebuild prepare` on the bonus probe).
- `scripts/b-compare.sh`: pairwise report; normalizes volatile staging
  paths, strips the Portage-only global-updates prelude + news tail from
  merge stdout and the deterministic masters/repo_name/invalid-news config
  noise from stderr (portuale implements neither updates nor news; both are
  EAPI-independent).
- Method notes: (a) the phases probes log `${EBUILD_PHASE_FUNC:-$1}` with
  no explicit tag, so on portuale (D1: FUNC unset) their log lines are
  EMPTY — line counts still attest executions; (b) `cell_noclear` chains
  accumulate stage content: P2 `emerge --pretend` runs NO hooks on either
  PM (attested: no new lines), P3/P4 snapshots are cumulative by design;
  (c) host default FEATURES has no bare `test` (`emerge --info`), so
  `src_test` runs only when forced (direct `ebuild … test`, bin/ebuild
  `EBUILD_FORCE_TEST`) or with `FEATURES=test`.

## Oracle facts (Portage 3.0.82.2, both EAPIs unless noted)

- O1. EAPI 7 vs 8 environments are identical except the `EAPI` value
  itself (and PV-derived paths): `AA`/`KV`/`PORTDIR`/`ECLASSDIR`/`LC_ALL`
  unset, `LANG=C.utf8`, in both. Grounds `eapi.py` thresholds
  (`exports_AA`/`exports_KV`/`exports_PORTDIR`/`exports_ECLASSDIR` <= 6,
  `posixish_locale` >= 6).
- O2. Direct `ebuild <f> <phase>` runs the dep chain (`actionmap_deps`,
  `doebuild.py:869-885`: setup->[pretend], unpack->[setup], …): single
  `unpack` with existing stamps STILL re-runs pretend+setup; multi-command
  `setup unpack … install` runs pretend+setup TWICE (10 hook executions
  for FULL). Each later command re-walks deps printing `>>> It appears
  that '<phase>' has already executed …; skipping.` (`doebuild.py:2395+`).
- O3. `emerge --oneshot` (fresh root, FULL-7): pretend, setup, unpack,
  prepare, configure, compile, install, preinst, postinst (no test:
  FEATURES). Upgrade (-8 over -7): same 9 + old-version teardown
  interleaved as preinst(NEW), prerm(OLD), postrm(OLD), postinst(NEW).
  LIM: setup, unpack, compile, install (prepare/configure/test run their
  silent `default`s — `>>> Preparing/Configuring source …` markers).
- O4. `emerge --pretend` executes NO hooks (not even `pkg_pretend`).
  `pkg_pretend` runs as the first phase of every real merge and of
  `ebuild <f> setup`/`pretend`.
- O5. Merge-time env: fresh install `MERGE_TYPE=source`,
  `REPLACING_VERSIONS=""` (exported, empty); upgrade
  `REPLACING_VERSIONS="7"`; `--usepkgonly` install `MERGE_TYPE=binary`;
  `emerge --unmerge` prerm/postrm: MERGE_TYPE/REPLACING/REPLACED all
  set-but-empty readonly UNEXPORTED. (`eapi.py`: `exports_merge_type`,
  `exports_replace_vars` >= 4.)
- O6. Defined-but-empty `src_prepare` (no `eapply_user`/`default`) dies
  rc 1 with `eapply_user (or default) must be called in src_prepare()!`
  (`phase-functions.sh:455`, via `__dyn_prepare:1145`,
  `__ebuild_main:838`), reported on stdout with the call-stack + support
  footer.
- O7. `ebuild <f> test` forces the test phase even with FEATURES lacking
  `test` (`EBUILD_FORCE_TEST`, `/usr/lib/python-exec/python3.14/ebuild:296-302`,
  `Forcing test.` on stderr; `>>> Test phase [not enabled]` when off).
- O8. `qmerge` after direct `install` merges the image (preinst, prerm,
  postrm, postinst) with verbose `--- replaced/<<< />>>` listing.
- O9. `emerge --buildpkgonly` builds without preinst/postinst (7 hooks);
  binpkgs are per-package dirs with build-id-suffixed `*.gpkg.tar`.
- O10. EPREFIX from make.conf alone is ignored (`EPREFIX=""`) without a
  prefix-configured profile: prefix rows are `not reproducible` here.

## Verdict table (67 cell pairs; Portage cite -> effect -> portuale cite)

Conventions: `same` = same rc, same hook executions (names on Portage,
counts on portuale given D1), same values modulo staging paths.
`diverges (fix)` = needs product-code change (S2/S3/S6 cluster).
Stage-file names below are the Portage side; portuale counterparts analyzed
after normalization (`${BDIR}`/`${EROOT}`/`${CFG}`/`${BDIR0}`).

### S0.4 direct-`ebuild` env cells (24): `b-env-{7,8}-X-ebuild-<phase>

| cell(s) | verdict | Portage -> portuale cites |
|---|---|---|
| pretend, setup, unpack, prepare, configure, compile, install, prerm, postrm x EAPI 7+8 (18 cells; logs exist both sides) | `diverges (fix)` | Per-variable rows below. Portage: `doebuild.py:doebuild_environment`, `_unexported_pms_vars:172-224`, `eapi.py` thresholds. portuale: `phase_environ.rs:117-209` (ENV_BLACKLIST/PORTUALE_COMPUTED), `ebuild_phases/phase_env.rs:293` (trailing slash EAPI<=6), `single_phase.rs:144-149,267`, `ebuild.rs:198-203` |
| ebuild-test x 7,8 | `diverges (fix)` D9 | Portage forces test: `/usr/lib/python-exec/python3.14/ebuild:296-302`. portuale: no forcing; `run_commands` skips (V1: zero hooks, rc 0). With `FEATURES=test` it runs (test-featurestest cell) |
| ebuild-preinst, ebuild-postinst x 7,8 | `diverges (fix)` D10 | Portage runs `pkg_preinst/postinst` standalone. portuale: not standalone (`ebuild_phases/mod.rs:163-170`), falls into `run_commands` (`ebuild.rs:393-405`) with empty prereq chain (`mod.rs:140-155`): silent rc-0 no-op, no log |
| ebuild-unpack x 7,8 (pretend+setup logs) | `diverges (fix)` D11 | Portage runs dep chain despite stamps (O2; `doebuild.py:869-885`). portuale runs unpack only (stamps from prior cells skip deps) |
| EAPI 7 vs 8 (all) | `same` shape | O1; portuale shows the identical divergence set for both EAPIs |

Per-variable rows (direct + emerge paths; `rx`=readonly+exported,
`r`=readonly unexported, `x`=exported):

| var | Portage | portuale | verdict |
|---|---|---|---|
| EAPI | `"7"/"8"` rx | same | `same` |
| EBUILD_PHASE | e.g. `"setup"` rx | same | `same` |
| EBUILD_PHASE_FUNC | `"pkg_setup"` rx | set-empty `r` (UNSET; `PHASE_FUNC=UNSET`) | `diverges (fix)` D1 |
| A | `""` x | UNSET in direct-ebuild path; `""` x in emerge path | `diverges (fix)` D2 |
| AA / KV | unset | unset | `same` |
| MERGE_TYPE | `"source"` rx (source merges) / `"binary"` rx (binpkg) | source merges: set-empty `r` (value missing); binpkg: `"binary"` rx, same | `diverges (fix)` D3a (source only) |
| REPLACING_VERSIONS | `""` rx (fresh) / `"7"` rx (upgrade) | fresh: set-empty `r`; upgrade: `"7"` rx, same | `diverges (fix)` D3b (fresh-empty only) |
| REPLACED_BY_VERSION | set-empty `r` | set-empty `r` | `same` |
| ROOT / EROOT | path, x/rx | same (mod root) | `same` |
| SYSROOT / ESYSROOT | `""` x | UNSET | `diverges (fix)` D4 |
| BROOT | `""` rx | set-empty `r` | `diverges (fix)` D4 |
| EPREFIX | `""` rx | same | `same` (prefix values: not reproducible, O10) |
| ED / D | no trailing slash, rx | same (mod base) | `same` |
| PORTDIR / ECLASSDIR | unset | unset | `same` |
| FILESDIR / DISTDIR / T / TMPDIR / HOME | set (x/rx) | same (mod base) | `same` |
| LC_ALL / LANG | unset / `C.utf8` | same | `same` |
| PORTAGE_TMPDIR / BUILDDIR | per config | per config when honored | see D7 |
| PORTAGE_CONFIGROOT | `${CFG}/` (trailing slash) | `${CFG}` (none) | `diverges (fix)` D8, cosmetic |
| PORTAGE_ACTUAL_DISTDIR | make.conf DISTDIR | `/var/cache/distfiles` host default (make.conf DISTDIR ignored) | `diverges (fix)` D6 |
| FEATURES / PORTAGE_FEATURES | full profile list, x | `""` x (direct path; process env still gates, cf. test-featurestest) | `diverges (fix)` D5 |
| USE | `elibc_glibc` rx (direct+emerge) | `""` rx direct; `elibc_glibc` emerge | `diverges (fix)` D5 (direct path) |
| ARCH | `""` x | `amd64` x | `diverges (fix)` D5 |
| KV | unset (`exports_KV`<=3) | unset | `same` |

### S0.4 config-isolation + emerge cycle (7)

| cell(s) | verdict | notes |
|---|---|---|
| b-env-7-X-ebuild-setup-notmpenv | `diverges (fix)` D7 | Same argv minus env TMPDIR. Portage builddir honors make.conf (`${BDIR}/portage/…`). portuale falls back to `/var/tmp/portage/app-misc/…` (NO `/portage/` infix), `PORTAGE_TMPDIR=/var/tmp`. portuale: `ebuild.rs:167-197` (env-or-default only; make.conf value never consulted in this path) |
| b-env-X-emerge-install7, -upgrade78 | `diverges (fix)` | Env rows as above (emerge path: A ✓, USE ✓; FUNC/D3a/D3b(fresh)/D4/D5/FEATURES/ARCH/DISTDIR still diverge; upgrade `"7"` value same). Counts/sequences same |
| b-env-X-emerge-pretend8 | `same` | No hooks either side; residual stdout diff is blank-line + `to ${EROOT}/` trailing slash (cosmetic) |
| b-env-X-emerge-unmerge8 | `diverges (fix)` D15 | Portage prerm env: MERGE_TYPE & co. unexported-empty. portuale: `MERGE_TYPE="binary"` EXPORTED (binary-merge env leaks into unmerge; cf. `single_phase.rs:144-149`) |
| b-env-X-emerge-buildpkg8 | `diverges (fix)` | Env rows as above |
| b-env-X-emerge-binpkg8 | `diverges (fix)` | `MERGE_TYPE="binary"` rx on BOTH (`same` value); REPLACING export-ness diverges (D3b). Binpkg layout: Portage per-package dir + `-<build-id>.gpkg.tar`, portuale flat `<pf>.gpkg.tar` (D18 observation) |

### S0.5 direct cells (16)

| cell(s) | verdict | notes |
|---|---|---|
| ebuild-pretend x FULL/LIM x 7,8 (6) | `same` | 1 hook each (LIM: no `pkg_pretend` -> silent rc 0 both). portuale line empty (D1) but count+rc+silence match |
| ebuild-seq x FULL/LIM/RT x 7,8 (6) | `diverges (fix)` D9+D12 | Portage FULL: 10 executions (pretend+setup DOUBLED, O2) incl. forced `src_test`; LIM: 5 (setup doubled). portuale: 7/5/7 empty lines — no doubling, no `src_test`, stdout has ZERO `>>>` phase markers (phases run silently), no `Forcing test.` on stderr. `portuale ebuild` routing: `ebuild.rs:365-415`, chains `commands.rs:93-94`, `mod.rs:140-155` |
| lim-ebuild-prepare, lim-ebuild-configure x 7,8 (4) | `same` | Dep chain runs defaults once (setup+unpack logged); configure reuses stamps silently. Counts match; stdout marker verbosity differs (D17) |

### S0.5 emerge cells (19)

| cell(s) | verdict | notes |
|---|---|---|
| emerge-pretend x 6 | `same` | No hooks either side (leftovers only); stdout residual is updates/news stripping + trailing slash (cosmetic) |
| emerge-oneshot x 6 (P3, fresh root) | `same` | Counts match exactly: FULL 9+11 (upgrade teardown prerm/postrm interleaved), LIM 4+4, RT 9+11; no `src_test` (FEATURES); buildpkgonly 7 without pre/postinst. Installed trees equivalent (`built.txt`, `config.args`, `README.md` via einstalldocs, `payload.txt` on both roots) |
| emerge-featurestest rt-8 (FEATURES=test) | `diverges (fix)` D13 | Test correctly skipped on both (RESTRICT; 9 build hooks). But portuale: 29 vs 31 lines — same-version reinstall runs NO old-version prerm/postrm (P3 different-version upgrade DOES: 20=9+11 both) |
| emerge-featuresnotest phases-8 (FEATURES=-test) | `diverges (fix)` D13 | Same teardown gap (29 vs 31). Portage prints `>>> Test phase [not enabled]`; portuale nothing (D17) |
| emerge-featurestest-full7 (mine, fresh roots) | `same` | 10 hooks both incl. `src_test` (`>>> Test phase:` / `>>> Completed testing` vs silent); FEATURES=test honored in merge path |
| emerge-buildpkgonly phases-7 | `same` | 7 hooks, no pre/postinst, both |
| ebuild-install phases-7 (direct) | `diverges (fix)` D9+D12 | 10 vs 7 executions (doubling + forced test, as seq) |
| ebuild-qmerge phases-7 | `diverges (fix)` D14 | Portage appends preinst, prerm, postrm, postinst (4) with verbose replaced/unmerged listing; portuale appends 4 silent executions (11-7 stage lines), empty stdout; both end with phases-7 in VDB (replace works) |
| ebuild-test-featurestest phases-7 (mine) | `same` | 7 hooks both (`FEATURES=test` honored direct path too); `checked.txt` in S on both; decision reads process env even though propagated `FEATURES=""` (nuance for S2) |

### S0.5 bonus + non-reproducible

| cell(s) | verdict | notes |
|---|---|---|
| envnoeapply-7-X-ebuild-prepare | `diverges (fix)` D16 | rc 1 both, identical die text (O6). Portage reports on STDOUT with host `phase-functions.sh:1145/455` trace; portuale on STDERR with vendored `:1216/525` trace, drops the `::t348` repo suffix (`:: failed`) and the `build.log is located at` line (no build.log written) |
| EPREFIX rows | `not reproducible` | O10: make.conf EPREFIX ignored without a prefix-configured profile; no prefix profile exists in the hermetic roots |

## Divergences in detail (fix clusters for S2/S3/S6)

- D1 `EBUILD_PHASE_FUNC` never set (both paths/phases/EAPIs). Portage:
  `eapi.py:exports_EBUILD_PHASE_FUNC` (>= 5) via `doebuild_environment`.
  portuale: `phase_environ.rs:129,201` (blacklist/computed plumbing
  present) but the value is never assigned in `phase_env.rs`/`run_phase.rs`.
  Visible everywhere: probe logs fall back to empty lines; `declare -r`
  (set, empty, unexported) vs `"pkg_setup"` rx.
- D2 `A` unset in the direct-`ebuild` path (emerge path correct `""`).
  Portage: `doebuild_environment` always sets `A` (empty when no
  distfiles). portuale direct path skips it.
- D3a `MERGE_TYPE` unset on source merges (Portage `"source"`; binary
  `"binary"` is correct), D3b `REPLACING_VERSIONS` unexported only when
  empty (upgrade `"7"` is exported, correct). Portage exports both
  whenever `eapi_exports_*` (>= 4). portuale: `single_phase.rs:144-149`
  (binary only), `:267` (replace value). Merge-time `pkg_*` phases
  re-source a saved `temp/environment` (cf. the `line 129: declare: …
  not found` trace), which explains the split.
- D4 `SYSROOT`/`ESYSROOT` unset (Portage: `""` exported;
  `eapi.py:sysroot/broot` >= 7) and `BROOT` unexported-empty (Portage:
  `""` rx).
- D5 Phase env misses profile/defaults depth in the direct path:
  `FEATURES`/`PORTAGE_FEATURES` empty, `USE` empty, `ARCH=amd64`
  (Portage: full FEATURES, `USE=elibc_glibc`, `ARCH=""`). Emerge path:
  USE ✓ but FEATURES/ARCH still diverge. Test gating still reads process
  env (test-featurestest), so only propagation diverges.
- D6 make.conf `DISTDIR` ignored (both paths):
  `PORTAGE_ACTUAL_DISTDIR=/var/cache/distfiles`. portuale:
  `ebuild.rs:201-203` (env-or-hardcoded-default). No fetch impact here
  (no SRC_URI) — env-string divergence only.
- D7 make.conf `PORTAGE_TMPDIR` ignored by direct `ebuild`
  (`ebuild.rs:167-197`); fallback `/var/tmp` + layout missing the
  `/portage/` infix (`/var/tmp/portage/app-misc/<pf>` vs
  `<tmp>/portage/<cat>/<pf>`). Emerge path honors it. notmpenv cells
  isolate this; the harness exports TMPDIR so all other cells are hermetic.
- D8 `PORTAGE_CONFIGROOT` trailing slash (`${CFG}/` vs `${CFG}`), cosmetic.
- D9 No `EBUILD_FORCE_TEST`: direct `ebuild <f> test` and multi-command
  seqs skip `src_test` (Portage forces it). Merge path honors
  `FEATURES=test`/`RESTRICT=test` correctly.
- D10 `preinst`/`postinst` are not standalone commands: silent rc-0
  no-op (`ebuild_phases/mod.rs:163-170`, `ebuild.rs:393-405` empty chain).
- D11 Single `ebuild <f> unpack` (and by extension any mid-chain command)
  does not re-run the dep chain (Portage does, O2).
- D12 No pretend+setup re-execution in multi-command invocations (Portage
  doubles, O2). D11/D12 share a root cause area (dep-spawn vs stamp
  checks, `doebuild.py:2370-2410` vs `commands.rs:93-94`); mechanism not
  fully root-caused — S2/S3 to decide which side is "correct".
- D13 Same-version reinstall skips old-version `pkg_prerm`/`pkg_postrm`
  (different-version upgrade runs them). Merge-engine replace path (S6
  cluster); hook counts 29 vs 31 in P4 cells.
- D14 `qmerge` runs the same 4 install-time hooks (11-7 stage lines) but
  prints nothing (Portage: preinst/prerm/postrm/postinst + verbose
  listing). S6 cluster (output parity only; replace itself works).
- D15 `emerge --unmerge` leaks `MERGE_TYPE="binary"` (exported) into
  prerm/postrm env (Portage: unexported-empty). S6 cluster.
- D16 Die-message fidelity: wrong stream (stderr vs stdout), dropped
  `::<repo>` suffix, missing `build.log` line (no build.log written at
  all), vendored-trace line numbers (expected local drift, S0.2 area),
  silent phase markers.
- D17 Output terseness (both paths): no `>>>` phase markers from
  `portuale ebuild`, no `Forcing test.` / `Test phase [not enabled]` /
  auto-clean / updates / news blocks from `portuale emerge`. EAPI-
  independent; fix-or-cut per slice (owner).
- D18 Binpkg layout: flat `<pf>.gpkg.tar` vs per-package dir +
  `-<build-id>.gpkg.tar` (S6 observation).
- D19 (method) `declare -p` of an unset probe variable prints
  `line 32: declare: <var>: not found` — that IS the absence record
  (AA/KV/PORTDIR/ECLASSDIR/LC_ALL unset on both PMs, O1).

## Not reproducible

- EPREFIX/ED-prefix rows: make.conf `EPREFIX` is ignored without a
  prefix-configured profile (O10); the hermetic roots have no prefix
  profile, and building one is outside S0. EAPI-side prefix plumbing
  (`___eapi_has_prefix_variables`, `ED`/`EPREFIX` export) is therefore
  unprobed; `EPREFIX=""` is identical on both PMs in all cells.

## Decisioni per il proprietario (default proposto)

1. D12/D11 (dep re-execution/doubling): Portage's re-execution looks
   accidental (stamps exist precisely to skip), but it IS the oracle.
   Default: replicate exactly (fix in S3), do not "improve".
2. D17 (terseness): keep phase-marker output out of the EAPI slices;
   propose a dedicated output-parity slice (or explicit cut) in S0.8 —
   default: fix `>>>` markers + `Forcing test.`/`Test phase` lines in S3,
   cut updates/news/auto-clean chatter (portuale never had them).
3. D13/D14/D15/D18 (merge engine): all point at the S6 cluster; default:
   one S6 slice covering replace-teardown hooks + unmerge env + qmerge
   parity, binpkg naming as cut unless trivial.
4. D5/D6/D7/D8 (config plumbing): default: fix in S2 alongside the
   EAPI-gated env rows (same files); D8 last (cosmetic).
5. EAPI 7 vs 8: NO EAPI-dependent difference found in S0.4/S0.5 scope —
   every divergence reproduces identically under both EAPIs. S2/S3 slices
   need only one EAPI's pins plus a same-shape assertion for the other.
6. `eapply --` (EAPI 8 `__eapi8_src_prepare` vs EAPI 7 `__eapi6_src_prepare`,
   `phase-helpers.sh`) is behaviorally identical for normal patch names
   (both apply `fix.patch`); no dedicated cell (S0.6 may add a
   dash-prefixed filename case).

## Files created

- Fixtures (in `../pmtest/fixtures/repo-348/`, i.e. this repo ONLY reads
  them through the `fixtures/` symlink): `app-misc/t348-b-env/`
  (`t348-b-env-7/8.ebuild`), `app-misc/t348-b-phases/` (2 ebuilds +
  `files/{configure,hello.in,fix.patch,README.md}`),
  `app-misc/t348-b-phaseslim/` (2 ebuilds),
  `app-misc/t348-b-phasert/` (2 ebuilds + `files/` copy),
  `app-misc/t348-b-envnoeapply/` (1 ebuild) + 9 real md5-cache entries
  under `metadata/md5-cache/app-misc/`. No existing fixture touched
  (`git status` in pmtest shows only the pre-existing dirty
  `fixtures/etc/profile.env` + `fixtures/var/lib/portage/world`, untouched
  by this worker).
- Evidence `LLM/evidence/2026-10-12-348-s0/b/`: 67 cell pairs
  (argv/rc/stdout/stderr + stage/env/phase logs + 2 `.workdir-ls`), 792
  files total. Verdicts: `same` 26 / `diverges (fix)` 41 /
  `diverges (cut)` 0 / `not reproducible` 1 (EPREFIX). No EAPI-7-vs-8
  difference anywhere: every divergence reproduces identically under both
  EAPIs.
...[truncated 1587 chars]
