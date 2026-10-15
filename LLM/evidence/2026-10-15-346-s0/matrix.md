# S0 matrix — 02.346/347/349/351 oracle captures (slice S0)

Evidence: this dir. Scripts: `scripts/` (`lib.sh` + one capture script per family;
`run-all.sh` replays all and cleans up). Oracle: Portage 3.0.82.2 ("Portage");
`target/release/portuale` at `f221c03f` (mtime 2026-10-15 19:18:07 UTC, never rebuilt).
Provenance + hermeticity notes (H1–H3, stock-gentoo/binrepos leakage): PROVENANCE.md.

Legend: **match** = §1 verdict confirmed as stated (fix/cut/stale stands).
**CONTRA** = capture contradicts a §0/§1 claim (listed in §7, all go to the owner).
**NEW** = behaviour the plan did not predict (also §7). `→ S<n>` = consuming slice.
`USE=` quotes the `-pv` flag column verbatim. All `emerge` cells carry
`--package-moves=n` on both sides except g00/l00 (dedicated coupling cells) and
c12 (dies in arg parsing).

## U — user profile as last profile node (346-UP → S1). Target `dev-libs/t341-dep` (IUSE `x`)

| cell | setup | Portage | portuale | verdict |
|---|---|---|---|---|
| u01-baseline | — | rc 0 `USE="-x"` | rc 0 `USE="-x"` | match |
| u02-useforce | `profile/use.force`=`x` | rc 0 `USE="(x)"` | rc 0 `USE="-x"` | match → S1 |
| u03-makedefaults | `profile/make.defaults` `USE="x"` | rc 0 `USE="x"` | rc 0 `USE="-x"` | match → S1 |
| u04-packageuse | `profile/package.use` `t341-dep x` | rc 0 `USE="x"` | rc 0 `USE="-x"` | match → S1 |
| u05-useforcemask | `package.use.mask`+`package.use` `… x` | rc 0 `USE="(-x)"` | rc 0 `USE="-x"` | match → S1 |
| u06-useforcefile | `profile/package.use.force` `… x` | rc 0 `USE="(x)"` | rc 0 `USE="-x"` | match → S1 |
| u07-usemask | `use.mask`=`x` + `package.use` `… x` | rc 0 `USE="(-x)"` | rc 0 `USE="-x"` | match → S1 |
| u08-acceptkeywords | `profile/package.accept_keywords` `bareacceptkeywordspkg ~amd64`, target it | rc 0 `[ebuild N ~]` | rc 1 `masked by: ~amd64 keyword` | match → S1 |
| u09-packages-atprofile | `profile/packages` `dev-libs/t341-dep`, `emerge -pv @profile` | rc 0 merges t341-dep | rc 2 `no package atoms to resolve` | match → S1 |
| u10-eapi5wild | `profile/eapi`=`5` + `package.use` `*/t341-dep x` | rc 0 `USE="-x"` + **one** `--- Invalid atom in …/profile/package.use: */t341-dep` on stderr | rc 0 `USE="-x"`, no warning | **CONTRA** (plan said the warning prints *twice*; capture has it once) → S1 (warning itself still missing in portuale) |
| u11-noeapiwild | no `eapi` + `package.use` `*/t341-dep x` | rc 0 `USE="x"` | rc 0 `USE="-x"` | match → S1 |
| u12-usestablemask | `use.stable.mask`=`x` + `package.use` `… x` (t341-dep is stable `amd64`) | rc 0 `USE="(-x)"` | rc 0 `USE="-x"` | match → S1 |
| u13-usestableforce | `package.use.stable.force` `… x` | rc 0 `USE="(x)"` | rc 0 `USE="-x"` | match → S1 |
| u14-chainmask-userunmask | scratch chain (relative parent → fixture default) `package.mask`s t341-dep; user `package.unmask` lifts it | rc 0 `[ebuild N #] … USE="-x"` (unmasked) | rc 0 `[ebuild N #] … USE="-x"` (unmasked) | **NEW**: portuale's bolted-on user-`package.unmask` DOES counter a chain mask here, although §0.1.1 says the flat reads "have no effect". S1 must preserve this. |
| u14b-chainmask-only | same chain mask, no user unmask (control) | rc 1 `All ebuilds … masked … (masked by: package.mask)` | rc 1 same shape (`for <root>` suffix missing — known convention) | match (both read the scratch chain incl. relative parent) |
| u15-nomakeprofile | no `make.profile` at all + user `use.force` | rc 1 `Your current profile is invalid … Allowed actions are limited to --help, --info, --search, --sync, and --version.` | rc 1 `masked by: missing keyword` | **NEW**: without `make.profile` Portage does not merely ignore the user profile — the whole run aborts (rc 1). Same rc, different reason. S1 pin documents both. |
| u16-parent-ignored | user profile holds ONLY `parent` → scratch profile with `package.use` `… x` | rc 0 `USE="-x"` (parent never walked) | rc 0 `USE="-x"` | match → S1 |
| u17-info | user `make.defaults` `USE="x"` + `FEATURES="test"`, `emerge --info` | `USE="… test x"`, `FEATURES="… test …"` | `USE="…"` (no `x`), `FEATURES="-news"` | match → S1 |
| u18-profile-starred | `profile/packages`: `*dev-libs/t341-dep` + `dev-libs/bareacceptkeywordspkg`, `@profile` | rc 1 all-masked bareacceptkeywordspkg (`*` lines excluded from `@profile`: `ProfilePackageSet.load` drops `str(x)[:1] == "*"`) | rc 2 `no package atoms to resolve` | match → S1 (starred exclusion proven: t341-dep NOT merged) |
| u19-system-unstarred | `profile/packages` `dev-libs/t341-dep`, `@system` | rc 0, `@system` = fixture 4-package set (unstarred line changes nothing) | rc 0, same 4 lines | match (rule, not fix: unstarred `packages` lines do not touch `@system`) |

## K — `package.keywords` (346-KW → S2). Target `bareacceptkeywordspkg` (`KEYWORDS=~amd64`)

| cell | setup | Portage | portuale | verdict |
|---|---|---|---|---|
| k01-user-keywords | user file `package.keywords` `… ~amd64` | rc 0 `[ebuild N ~]` + deprecation warning on stderr: `<…/KeywordsManager.py:89: UserWarning: <path>/package.keywords is deprecated, use <path>/package.accept_keywords instead` + `  warnings.warn(` | rc 1 masked | match → S2 (Q3: message-only, fires here) |
| k02-userprofile-amd64 | user-profile `package.keywords` `… amd64` | rc 0 `[ebuild N ]` (stable keyword added to own KEYWORDS) | rc 1 masked | match → S2 |
| k03-chain-amd64 | scratch chain `package.keywords` `… amd64` | rc 0 `[ebuild N ]` | rc 1 masked | match → S2 (chain node proved the reader for the chain too) |
| k04-profile-minus | user-profile `package.keywords` `… -~amd64` | rc 1 `masked by: missing keyword` | rc 1 `masked by: ~amd64 keyword` | match → S2 (reason text differs; both masked) |
| k05-cross-minus | user `package.keywords` `… ~amd64` + user `package.accept_keywords` `… -~amd64` | rc 1 masked + deprecation warning (fires: `package.keywords` yielded an entry) | rc 1 masked, no warning | match → S2 (read-`package.keywords`-first-then-extend order confirmed: stays masked) |

## B — build-ids in user `package.accept_keywords` / `package.use` (346-BID → S3)

Two-instance `PKGDIR` (`USE:`/`KEYWORDS:` per cell), `emerge -pv --usepkgonly=y`.

| cell | user file | Portage | portuale | verdict |
|---|---|---|---|---|
| b01-ak-bid2 | ak `=t341-dep-1-2 ~amd64` (both builds `~amd64`) | rc 0 `[binary N ~] …-1-2` | rc 1 both masked | match → S3 |
| b02-ak-bid1 | ak `=t341-dep-1-1 ~amd64` | rc 0 `[binary N ~] …-1-1` | rc 1 both masked | match → S3 |
| b03-use-bid2-respect | use `=t341-dep-1-2 x` (build1 `USE: x`, build2 empty), `--binpkg-respect-use=y` | rc 1 `masked by: use flag configuration mismatch` | rc 0 `[binary N ] …-1-2 USE="-x"` | match → S3 |
| b04-use-bid1-respect | use `=t341-dep-1-1 x`, respect=y | rc 0 `…-1-2 USE="-x"` | rc 0 same | match (same output both sides) |
| b05-range-ignored | ak `>=t341-dep-1-2 ~amd64` | rc 0 `…-1-2` | rc 0 same | match (build-id ignored in `>=` on both) |
| b06-use-bid-norespect | use `=t341-dep-1-2 x`, no respect flag | rc 0 `…-1-2 USE="-x"` (build-id line does not move displayed USE) | rc 0 same | match (same output both sides) |

## E — direct-`ebuild` configuration (349 → S4; 351-BIN Q5)

`t348-b-env-8 setup` (dumps env to `T348_LOGDIR`), `env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES`.

| cell | make.conf | Portage | portuale | verdict |
|---|---|---|---|---|
| e01-full | DISTDIR+TMPDIR+FEATURES(+test) | all from config; `FEATURES`/`PORTAGE_FEATURES` full list; `PORTAGE_CONFIGROOT=<cfg>/` (slash); `PORTAGE_REPO_NAME=t348`; `ARCH=""`; `USE="elibc_glibc"` | `FEATURES`/`PORTAGE_FEATURES="test"` only; `PORTAGE_CONFIGROOT=<cfg>` (no slash); no `PORTAGE_REPO_NAME`; `ARCH="amd64"`; `USE=""` | match → S4 (349-FEAT/CFG/REPO/ARCH/USE) |
| e02-nodistdir | no DISTDIR | `DISTDIR=<tmp>/portage/…/distdir` (per-package fake) | `DISTDIR=/var/tmp/portage/…/distdir` (**host** — make.conf TMPDIR ignored) | match → S4 (349-TMP/DIST) |
| e03-notmpdir | no TMPDIR | falls back to `/var/tmp` (`PORTAGE_TMPDIR=/var/tmp`, builddir under it) | same fallback path, i.e. writes host `/var/tmp/portage` (captured in `e03-host-var-tmp-portage.txt`, cleaned after) | match → S4 (the "`/portage/` infix" half is stale as stated: both have it) |
| e04-nofeatures | no FEATURES | full host-globals+conf list | `FEATURES=""` empty | match → S4 |
| e05-envwins | all keys + env `PORTAGE_TMPDIR/DISTDIR/FEATURES=-test` | env wins (builddir under env tmp; `FEATURES` env+globals) | env tmpdir/distdir win, but `FEATURES=""` (env `-test` + conf `test` cancel — incremental fold visible) | match → S4 (precedence data for the D3 plumbing) |
| e06-makeglobals-package | + `<cfg>/usr/share/portage/config/make.globals` copy, `ebuild package` | `FEATURES` incl. `binpkg-multi-instance`; layout `<pkgdir>/app-misc/t348-b-env/t348-b-env-8-1.gpkg.tar` + `Packages{,.gz}` | `FEATURES` full (globs copy IS read) yet layout flat `app-misc/t348-b-env-8.gpkg.tar`, no `Packages.gz` | match → S4 + 351-BIN (Q5 default "follow resolved FEATURES" confirmed needed: direct path uses raw `$FEATURES`) |
| e07-fetchonly-distdir | DISTDIR only; scratch `s346-fetch-1` (`SRC_URI` → distfiles proxy, hand-written `Manifest`) | rc 0, file lands in make.conf DISTDIR (+ `.layout.conf.*`/`.mirror-cache.json` sidecars) | rc 2 `emerge: option "--fetchonly" is … not yet implemented in portuale` | **NEW** (plan assumed an emerge-half comparison was possible): portuale has no `--fetchonly`; 349-DIST emerge half needs it or another vehicle. S4 input. |
| e08-die-repo | —, `envnoeapply-7 prepare` | die block on **stdout** with `::t348`, `The complete build log is located at '<T>/build.log'`, `build.log` + `environment` exist | die block on **stderr** with `::` (empty repo), no build-log line, no `build.log` | match → S4 (REPO) + S6 (DIE) |

## S — stamps / environment / WORKDIR / checksums (351-STAMP → S5)

`t348-b-phases-8`, hermetic exported+conf `PORTAGE_TMPDIR/DISTDIR/FEATURES` (sandbox off).

| cell | Portage | portuale | verdict |
|---|---|---|---|
| s01-unpack (clean) | `>>> Unpacking source...` / `>>> Source unpacked in …`; no checksum line, no Manifest complaint (no Manifest present) | silent rc 0 | match → S5+S6 |
| s02-unpack-again | `>>> WORKDIR is up-to-date, keeping...` + `It appears that '<phase>' has already executed …` / `Remove '<builddir>/.<phase>ed' to force <phase>.` per phase | silent rc 0 | match → S5 |
| s03-compile / s04-test / s05-install | skip pairs for done phases, then the phase markers (`>>> Test phase: …`, `>>> Install … into <image>`, `>>> Completed installing …`, `Final size` ×2) | silent (`Forcing test.` on stderr for s04 — the shipped #348 S3 bit); s05 stdout = raw `mkdir/cp` **set-x traces** of the install (stream detail for S6) | match → S5+S6 |
| s06-noauto-install | no skip: `>>> Install …` runs (under `noauto` the install skip is skipped, `doebuild.py:2395-2412`) | silent rc 0 | match → S5 (pins the noauto exception) |
| s07a-setup-unpack (clean) | `>>> Not marked as unpacked; recreating WORKDIR...` + unpack markers | silent rc 0 | match → S5 |
| s07b-setup-unpack-again | skip pairs for pretend+setup, `WORKDIR is up-to-date`, then all three skip pairs | silent rc 0 | match → S5 |
| s08-manifest-unpack (scratch copy WITH `Manifest`, `ebuild manifest` first) | `* checking ebuild checksums ;-) ... [ ok ]` + `* checking auxfile checksums ;-) ... [ ok ]` (gen step prints `>>> Creating Manifest for …`) | silent rc 0 (gen step: `ebuild: dry run -- … commands: ["manifest"]` — `manifest` unimplemented; no `.rc` captured for gen, auxiliary only) | match → S5 (checksum line conditional on Manifest; portuale has no check at all) |
| s09-nomanifest-unpack (same copy, Manifest removed) | rc 1 `* Manifest not found for '…/t348-b-phases-8.ebuild'` on stderr | rc 0 silent | match → S5 (exact text `* Manifest not found`, not `!!! Manifest`) |

## M — markers / streams / build.log (351-MARK+DIE → S6)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| m01-unpack | markers stdout (124 B); stderr 830 B = config warnings only | 0/0 | match → S6 (all phase output rides stdout through the tee) |
| m02-fresh-install | `>>> Test phase [not enabled]: app-misc/t348-b-phases-8` inside install chain | silent rc 0 | match → S6 (the `[not enabled]` trigger: chain without `test`) |
| m03-phasert-test (`RESTRICT=test`, `FEATURES=test`) | `einfo Skipping…` + `>>> Test phase [disabled because of RESTRICT=test]` stdout; `Forcing test.` **stderr** | `Forcing test.` + `Skipping…` both **stderr** | match → S6 (stream routing differs too) |
| m04-phasert-notest | same as m03 | same as m03 | match → S6 |
| m05-phases-minus-test (direct `ebuild test`, `FEATURES=-test`) | `>>> Test phase: …` runs (direct `test` forces via `EBUILD_FORCE_TEST`) | `Forcing test.` stderr, runs | match (forced-test path agrees) |
| m06-die (+`.tmpfind`/`.buildlogs`) | die stdout; `${T}/build.log` written; env file line | die stderr; no `build.log` | match → S6 |
| m07-port-logdir (+`.plogfind`) | log at `<PORT_LOGDIR>/app-misc:t348-b-envnoeapply-7:<ts>.log` + `The complete build log is located at '…'` convenience line | `plogs/` empty; no convenience line | match → S6 (G4 answered: capture exists; portuale has neither half) |
| m08-emerge-jobs1/2 `--oneshot` | `Calculating…` + `>>> Verifying ebuild manifests` + `>>> Running pre-merge checks` + `>>> Emerging (1 of 1) … for <root>/` + full `>>>` phase stream + `[not enabled]` + `Install/Completed installing` + `Final size` ×2 + `Installing/Completed (1 of 1)` + info-index line; jobs2 identical but paths/timing | `Calculating…` + `Dependency resolution took (backtrack: 0/20).` (**no timing**) + `>>> Emerging … for <root>` (no slash) + raw set-x traces + `>>> Installing/Completed` — no `Verifying/pre-merge` lines, no `>>>` markers, no `Final size`; **jobs2 output (442 B) is much shorter than jobs1 (862 B)**: parallel build output swallowed | match → S6 (foreground serial case per M cells; jobs2 delta is NEW data for S6) |
| m09-emerge-quietbuild | Jobs progress lines + Emerging/Installing/Completed, phase stream in `build.log` | same Jobs-line shape (closest-aligned output of the batch) minus markers/timing | match → S6 |

## A — masked aborts (351-ABORT → S7)

| cell | Portage stdout (rc 1) | portuale stdout | verdict |
|---|---|---|---|
| a01..a04 top-level all-masked (`bareacceptkeywordspkg`: `--oneshot`/`-p`/`-pv`/`--ask=n -v`) | `Calculating dependencies ... done!` + `Dependency resolution took …` (89 B); `-p` variants add `These are the packages …` preamble (146 B) | empty (0 B) in all four | match → S7 (the all-masked arm; Q6 default "non-pretend only" now looks too narrow — even `-p` aborts print it; owner call) |
| a05..a08 masked dep (`abort-masked-last` + user `package.mask dev-libs/maskeddep`) | same 89/146 B shapes | 82/138/139 B: `Calculating…` + `Dependency resolution took (backtrack: 0/20).` (no timing) | **CONTRA** (plan §0.4: "portuale's stdout is empty" for masked aborts — true only for the top-level all-masked arm; dep-level aborts already print). S7 scope narrows to the all-masked arm. |
| a09..a12 unsatisfied dep (`abort-unsat-last`) | same 89/146 B shapes | same as a05..a08 | same CONTRA as above |

Method note: the (b) premise needed a user mask (`maskeddep` is mask-free in a bare root — its "package.mask'd" DESCRIPTION refers to per-test masks); unmasked, Portage dies at `Verifying ebuild manifests` (fixture main repo has no Manifests; `thin-manifests` only in `repo-348`) while portuale really merges — that detour is recorded, not used.

## C — argparse errors (347-ERR → S8)

Full stderr captured whole (usage block + error tail). `PYTHONHASHSEED=0` (`*-seed`) and unset (`*-noseed`).

| cell | Portage (`emerge: error: …`, rc 2) | portuale (rc 2) | verdict |
|---|---|---|---|
| c01 `--jobs=x` | `Invalid --jobs parameter: 'x'` + blank line | `invalid --jobs parameter: "x"`, no blank | match → S8 |
| c02 `-j x` | NOT an error: `x` treated as atom → rc 1 `there are no ebuilds to satisfy "x"` | rc 1 same text (minus `for <root>.` suffix) | **NEW**: `-j` takes no separate argv on either side; S8 must decide the `-j` shape from this, not from a jobs-error assumption |
| c03 `--load-average=x` | `Invalid --load-average parameter: 'x'` + blank | `invalid …: "x"`, no blank | match → S8 |
| c04 `--deep=x` | `Invalid --deep parameter: 'x'` + blank | same lowercase/quote/no-blank trio | match → S8 |
| c05 `--backtrack=x` | `Invalid --backtrack parameter: 'x'` + blank | same trio | match → S8 |
| c06 `--color=maybe` | `argument --color: invalid choice: 'maybe' (choose from 'y', 'n')`, single `\n` | `--color: invalid choice: "maybe" (choose from 'y', 'n')` | match → S8 |
| c07/c08 `--with-bdeps(auto)=x` | `argument --with-bdeps(-auto): invalid choice: 'x' (choose from 'y', 'n')` | `option "--with-bdeps(-auto)": invalid choice: "x" (choose from "y", "n")` | match → S8 (note double-quoted choices) |
| c09/c10 `--onlydeps-with-(r\|i)deps=x` | `… (choose from 'True', 'y', 'n')` | `… (choose from "True", "y", "n")` | match → S8 |
| c11 `--autounmask-backtrack=x` | `… (choose from 'y', 'n')` | double-quoted variant | match → S8 |
| c12 `--frobnicate` | `unrecognized arguments: --frobnicate`, single `\n` | `unrecognized option "--frobnicate"` | match → S8; seed md5 `bca2…` vs noseed `2a11…` documents the 347-USG cut |
| c13 `--exclude =a/b-1` | `Invalid Atom(s) in --exclude parameter: '=a/b-1' (only package names and slot atoms (with wildcards) allowed)` + blank | **same text**, single `\n` | match → S8 (only the D4 trailing blank differs) |
| c14 `--buildpkg-exclude !dev-libs/t341-dep` | `Invalid Atom(s) in --buildpkg-exclude parameter: '!dev-libs/t341-dep'` + blank | same text, single `\n` | match → S8 |

Trailing-newline rule (D4) confirmed byte-exact: every `parser.error` message ends `\n\n`, argparse-native ones `\n`.

## G — binhost exclude/include + partly-excluded target (347-GB/PX → S9)

Scratch repo `s346g` (profiles only) + `file://` binhost, two remote-only binaries, `emerge -pv --getbinpkg`.

| cell | Portage | portuale | verdict |
|---|---|---|---|
| g00-nomoves (no `--package-moves`) | `Performing Global Updates` block (host gentoo updates!) then both `[binary N g]` | both `[binary N g]`, no block | the UPD-coupling demo for the 351-UPD cut (also forces `>>> Auto-cleaning…`+`No outdated…` on real merges — see l00) |
| g01-exclude (`--getbinpkg-exclude a`, targets a+b) | rc 1 `no ebuilds to satisfy "app-misc/s346-gbin-a"` (excluded index entry ⇒ unsatisfiable top-level arg) | rc 0 merges both (option unread) | match → S9 |
| g01b (exclude a, target only b) | rc 0 `[binary N g] b` | rc 0 same | match (positive side pinned) |
| g02-include (include a, targets a+b) | rc 1 `no ebuilds … "b"` (non-empty include = whitelist) | rc 0 merges both | match → S9 |
| g02b (include a, target only a) | rc 0 `[binary N g] a` | rc 0 same | match |
| g03-both (exclude+include a) | rc 0 merges both + `!!! The following atoms appear in both the --getbinpkg-exclude and --getbinpkg-include command line arguments:` + atom | rc 0 merges both, no warning | match → S9 |
| g04-repokey (`binrepos.conf getbinpkg-exclude = a`) | rc 1 same as g01 (per-repo key folds into the same filter) | rc 0 merges both (key unparsed) | match → S9 (Q7 default "yes" confirmed viable) |
| g05-repobadatom (`=bad/atom-1` in that key) | rc 0 merges both + `!!! The following atoms are invalid in getbinpkg-exclude attribute for binrepo [s346gbin] (only package names and slot atoms allowed):` | rc 0 merges both, no warning | match → S9 (exact warning text for `binrepo/config.py` validation) |
| g06-cli-overrides-repo (repo exclude b + CLI include b, targets a+b) | rc 1 on **a**: `!!! The following getbinpkg-exclude atoms for [s346gbin] have been overridden by the --getbinpkg-include option:` + `b`, then a unsatisfiable (whitelist) | rc 0 merges both | match → S9 (override direction + warning text; include is a whitelist) |
| g07-partly-excluded (`--exclude a`, b user-masked) | rc 1: preamble+Calculating stdout, masked-b report; excluded-a arg silently skipped (`depgraph.py:5582-5592`, `if excluded: continue` — verified in `3rdparty/portage/lib/_emerge/depgraph.py`) | rc 1 `no ebuilds to satisfy "…gbin-b"` (miss text, not masked-block, for the binary) | match → S9 (skip rule + binary mask-report gap) |

`--pretend` was not re-run per cell (all G runs are `-pv`, which refreshes the index the same way; no archive was ever opened — `PATH`s are fake).

## L — merge/unmerge listing (351-LIST → S10 decision) and CL — auto-clean (351-CLEAN → S11 decision)

`t348-b-phases-{7,8}` / `t348-b-phaseslim-8` (thin-manifests repo).

| cell | Portage | portuale | verdict |
|---|---|---|---|
| l00-nomoves (`emerge --oneshot`, no moves flag) | UPD block + forced `>>> Auto-cleaning packages...` / `>>> No outdated packages were found on your system.` (proves `actions.py:3671-3691`: default `AUTOCLEAN="no"` overridden by applied updates) | merge, no block | UPD-cut demo; S11 must implement user-set-`AUTOCLEAN=yes` ONLY |
| l01-ebuild-merge (direct, fresh ROOT) | `checking 3 files for package collisions` + `>>> Merging <cpv> to <root>/` + per-entry `>>>`/`---`/`===` lines + `>>> <cpv> merged.` | set-x traces only | → owner (S10). NOTE vs §0.4: the listing is **direct-path-only** (see l02). |
| l02-emerge-new | `>>> Installing (1 of 1)…` / `>>> Completed (1 of 1)…`, NO per-file lines, NO collision line, NO `Merging` line | same `Installing/Completed` shape minus markers | → owner (emerge path has no listing at default verbosity) |
| l03-replace (7 via `ebuild merge`, then `emerge` 8) | `Installing (1 of 1)` 8, no `Safely unmerging` / per-file lines on the emerge path | same shape | → owner |
| l04-`emerge -C` | selection table + `>>> Unmerging (1 of 1)…`, no per-file `<<<` lines; honors `CLEAN_DELAY=0` (no countdown) | same table + extra `>>> Waiting 5 seconds before starting...` / `>>> (Control-C to abort)...` despite `CLEAN_DELAY=0` (**NEW**: portuale ignores make.conf `CLEAN_DELAY` — S4-adjacent) | → owner |
| l05-qmerge (package 8, qmerge over installed 7) | collision line + `>>> Merging …` + per-entry `>>>`/`---`/`===` + `>>> Safely unmerging already-installed instance...` + `--- replaced obj/dir` + `<<<` removals + `>>> Original instance of package unmerged safely.` + `>>> … merged.` (full §0.4 shape) | empty stdout rc 0 (vdb shows 8 installed, 7 gone — replace happened silently) | → owner (full oracle for S10) |
| cl01-autoclean-outdated (7 installed, `AUTOCLEAN=yes`, merge 8) | `>>> Auto-cleaning packages...` + `No outdated…` (replace already consumed 7) | nothing | → owner (S11 trigger text) |
| cl01b-autoclean-unmerge (phaseslim-8 extra, unrelated) | `Auto-cleaning…` + `No outdated…` (`--clean` ≠ `--depclean`: an unneeded package is NOT "outdated") | nothing | → owner (defines "outdated": same-CP+slot older counter only) |
| cl01c-autoclean-leftover (reconstructed 7 leftover + world, unrelated final merge) | selection table (`selected: 7 / protected: 8`), `>>> Unmerging (1 of 1)…`, 7 gone from vdb | nothing (7 stays) | → owner (the unmerge arm oracle for S11) |
| cl02-autoclean-fresh | `Auto-cleaning…` + `No outdated…` | nothing | → owner |

`--clean` vs `--depclean` (manual probe on cl01b root, not a cell): `--clean -p` finds nothing while `--depclean -p` flags `phaseslim` — autoclean is strictly the outdated-instance pass.

## §1 table confirmation

346-UP→S1, 346-KW→S2, 346-BID→S3, 346-LIC stale ✓ (nothing captured against it),
347-GB→S9, 347-PX→S9, 347-ERR→S8, 347-USG cut ✓ (seed/noseed md5s),
347-MRG→S8 (no oracle cell: `mrg` is portuale-only by design, no Portage
counterpart exists; names/arity come from `_emerge/main.py` per the plan),
349-TMP/DIST/FEAT/ARCH/CFG/REPO/USE→S4 (349-USE reproduced in e01:
Portage direct-path `USE="elibc_glibc"`, portuale `USE=""`), 351-BIN Q5→S4, 351-STAMP→S5, 351-MARK/DIE→S6,
351-ABORT→S7 (scope narrowed to the all-masked arm), 351-FT stale ✓
(`Forcing test.` captured on both, m04/m05), 351-LIST→owner (evidence above),
351-CLEAN→owner (evidence above), 351-NEWS/UPD cuts ✓ (g00/l00 are the cut text),
346-X→#371/#372/#373 (empty-set rc reproduced in u09-portuale/u18-portage-contrast
and u18-portuale rc 2; bashrc/repos.conf keys not probed — filing only).
