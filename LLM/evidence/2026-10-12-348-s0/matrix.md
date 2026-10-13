# #348 S0 — matrix, tally and slice decisions (S0.8)

Captures: Portage 3.0.82.2 (this host) vs `target/release/portuale`, hermetic roots, overlay `pmtest/fixtures/repo-348`. Details per part: `a-attrs-drift-predicates.md` (S0.1-S0.3), `b-env-phases.md` (S0.4-S0.5), `c-helpers.md` (S0.6), `d-lifecycle.md` (S0.7). Scripts in `scripts/`. Every verdict below is a worker's capture, re-read by the coordinator; spot-checked cells: `d-eapi99-install`, `d-requse8-setup`, `d-eapi99-pret` (rc and first lines match the claims).

## Tally

| Part | Cells | same | diverges (fix) | diverges (cut) | not reproducible |
|---|---|---|---|---|---|
| A  attributes (S0.1) | 38 fields | 21 implemented + 1 vendored bash | 14 missing | 2 n/a at floor 7 | – |
| A  vendored-script drift (S0.2) | 74 files | 68 identical, 1 intentional change, 5 portuale-owned | 0 unintended | – | – |
| A  predicates (S0.3) | 222 | 148 | 0 | – | 74 (EAPI 99 under brush: bash ≥5.3 check, not EAPI semantics) |
| B  environment + phases (S0.4/5) | 67 pairs | 26 | 41 | 0 | 1 (EPREFIX) |
| C  helpers (S0.6) | 86 | 78 | 8 | 0 | 1 aspect (strict keepdir lives in merge-time QA) |
| D  lifecycle (S0.7) | 69 | 29 | 23 | 5 | 7 (`ebuild depend`: Portage itself tracebacks) |

Headline finding: **no EAPI 7 vs 8 behavioural difference was found in any captured cell** (B: every divergence reproduces identically under both). The vendored bash (predicates, helpers, `econf` flags, `unpack`, `die`) is Portage's own and matches. What diverges is portuale's Rust side of doebuild/config.environ and the merge engine, mostly independent of EAPI value; the EAPI-specific pieces are (a) unsupported-EAPI handling and (b) the exports gated by `_eapi_attrs`.

## Divergences, grouped into clusters

| Id | Divergence (Portage → portuale) | Cluster | Decision |
|---|---|---|---|
| A-attrs | 14 `_eapi_attrs` fields missing from `EapiAttrs` (BROOT/SYSROOT/ESYSROOT, exports_*, symlink_rewrite, pkg_pretend, src_prepare_src_configure, idepend, selective_src_uri_restriction, …) | S1 | fix |
| U1 | `emerge -p/--oneshot/--buildpkgonly` on EAPI 99 / `7_alpha`: `(masked by: EAPI 99)` block + upgrade text → `no ebuilds to satisfy` (14 cells) | S5 | fix |
| U2 | `ebuild <f> install/pretend/clean` on unsupported EAPI: rc 1 `Unable to do any operations … EAPI is higher` → runs all phases / silent rc 0 | S5 | fix (safety) |
| U3 | binpkg with EAPI 99 merged by portuale (VDB gets EAPI=99); Portage masks it | S5 | fix (safety) |
| U4 | cache/file EAPI skew (`eapimm`): Portage trusts IDEPEND string presence, portuale gates on cache EAPI | S5 | fix |
| U5 | `EAPI=" 8"` (spaces): Portage builds; portuale dies `eapply_user: command not found` | S5 (investigate root cause) | fix if EAPI-normalisation, else residue |
| E1 | `EBUILD_PHASE_FUNC` never set (D1) | S2 | fix |
| E2 | `A` unset in direct `ebuild` path (D2) | S2 | fix |
| E3 | `MERGE_TYPE=source`, `REPLACING_VERSIONS=""` not exported for source merges; `REPLACED_BY_VERSION` (D3) | S2 | fix |
| E4 | `SYSROOT`/`ESYSROOT`=`""` unset, `BROOT` unexported (D4) | S2 | fix |
| E5 | `SLOT` not exported in standalone phases (keepdir `.keep_<cat>_<pn>-0` vs `-`) (C) | S2 | fix |
| E6 | `IUSE_EFFECTIVE` missing in standalone env → `in_iuse` rc 1; `___in_portage_iuse` function missing → `use/usex/use_enable` do not die on unknown flag (C; `config.py:2185`) | S2 | fix |
| P1 | `ebuild <f> test` does not force the test phase (`EBUILD_FORCE_TEST`, D9) | S3 | fix |
| P2 | `preinst`/`postinst` not runnable standalone: silent rc-0 no-op (D10) | S3 | fix |
| P3 | `ebuild <f> setup` ignores `REQUIRED_USE` (emerge path correct) | S3 | fix |
| M1 | same-version reinstall skips old `pkg_prerm/postrm` (D13) | S6 | fix |
| M2 | `emerge -C` leaks `MERGE_TYPE="binary"` into prerm/postrm (D15) | S6 | fix |
| M3 | merge-time `IDEPEND` installed into target ROOT; Portage uses the running root (`depgraph.py:4252`) | S6 | fix |
| X1 | `has_version/best_version` served by `portageq-wrapper`, results identical to Portage in all captured cells (incl. `-b/-d/-r`, `--host-root`), `ebuild-ipc` exit 127 unreachable from them | – | **cut** (no observable consequence at EAPI 7/8) |
| X2 | `>>>` phase markers, `Forcing test.`, auto-clean/news chatter (D17) | – | cut → residue |
| X3 | direct-`ebuild` config plumbing: make.conf `DISTDIR`/`PORTAGE_TMPDIR`, `FEATURES`/`USE`/`ARCH` in env, `PORTAGE_CONFIGROOT` slash (D5-D8) | – | cut → residue (EAPI-independent, `ebuild` CLI only) |
| X4 | direct `ebuild` dep-chain re-run / pretend+setup doubling (D11/D12) | – | cut → residue (EAPI-independent, `ebuild` CLI only) |
| X5 | die-message stream/`::repo` suffix/`build.log` line (D16), `qmerge` verbosity (D14), binpkg layout names (D18), `-K` with no binaries fails silently | – | cut → residue |
| X6 | `clean`/`depend` accepted as dry-run no-ops | – | keep (documented) |
| X7 | EPREFIX/prefix rows, strict `keepdir` (merge-time QA), EAPI 99 predicates under brush | – | not reproducible / recorded |

## Slice decisions (S0.8 — coordinator's defaults, owner asleep; flagged for review)

Confirmed clusters: **S1** (attributes), **S5** (unsupported EAPI), **S2** (environment + the helper-environment items from part C), **S3** (phase sequence), **S6** (merge-time). **S4 (helpers) is dissolved**: the vendored helpers match, the helper divergences (E5, E6) are environment exports and live in S2; `has_version/best_version` are a cut (X1). Order: S1 → S5 → S2 → S3 → S6 → Z.

Owner questions (answered with the default so the night's work could proceed; revisit on waking):
1. IPC-served queries: cut (X1). Default taken.
2. Output terseness, `ebuild` CLI plumbing/dep-chain doubling, cosmetic die/qmerge/layout items: filed as residues, not fixed (X2-X5).
3. Whether the vendored script drift or brush-vs-bash EAPI-99 gap needs action: no (recorded).
