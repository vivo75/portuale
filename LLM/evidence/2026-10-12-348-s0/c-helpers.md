# S0.6 Helpers, available and banned — captures & inventory (worker `c`)

Oracle: **Portage 3.0.82.2** (host `/usr/bin/ebuild`, scripts under
`/usr/lib/portage/python3.14/`, Python under
`/usr/lib/python3.14/site-packages/portage/`). Portuale:
`/home/vivo/repo/PORTUALE/portuale/target/release/portuale`
(mtime 12 ott 22.37, **not rebuilt** per brief), default `--shell bash`.
EAPIs in scope: 7 and 8.

- Fixtures: `../pmtest/fixtures/repo-348/app-misc/t348-c-*-<phase>/`
  (43 packages, 86 ebuilds: one package per family+phase, versions `-7`
  (EAPI 7) / `-8` (EAPI 8) of the same package for row-by-row comparison),
  each with a REAL md5-cache entry
  (`repo-348/metadata/md5-cache/app-misc/<pf>`, all 86 md5-verified;
  no `Manifest` files: overlay uses `thin-manifests = true`, the md5-cache
  IS the manifest).
- Configroot/ROOT: `/var/tmp/t348-c/cfg`, `/var/tmp/t348-c/root`
  (`PORTAGE_TMPDIR=/var/tmp/t348-c/tmp` exported so both PMs share the
  builddir layout; repos: fixture `repo` + `overlay` + 3 aux + `t348`
  prio 50; `make.profile` → fixture default profile; `make.conf` adds
  `T348_SENTINEL=present` + `ENV_UNSET=T348_SENTINEL`). Never `ROOT=/`,
  no `SRC_URI` anywhere (no fetch), host system untouched.
- Captures: `LLM/evidence/2026-10-12-348-s0/c/` —
  per run `<cell>.argv/.rc/.stdout/.stderr` (+ `<cell>.plog`, copy of the
  probe's `${T}/t348.log`); cell name
  `c-<pf>-<phase>-<pm>`. 86 cells (= 86 ebuilds × 1 target phase each,
  Portage vs portuale) = 172 runs = 860 files.
- Scripts: `../scripts/c-gen-a.sh`, `c-gen-b.sh` (hand-authored probe
  bodies, quoted heredocs), `c-run-cell.sh`, `c-run-all.sh`.
- Probes begin each test phase with `: > ${T}/t348.log` and
  `unset EBUILD_MASTER_PID` (see method note M3); `die`-class calls are
  subshell-contained with `rc` recorded via `tlog` (log + `einfo`).

## Method notes (binding for reading the table)

- **M1 — one phase per ebuild.** Portage `ebuild <f> <phase>` runs the
  doebuild chain up to the named phase and STOPS at the first nonzero
  phase; portuale chains too (its `compile` cell runs `src_prepare` +
  `src_compile`). Every probe therefore tests in exactly one phase; all
  other phases are death-free (`src_prepare` runs bare `eapply_user` for
  the tagfile `__dyn_prepare` enforces —
  `phase-functions.sh:450-453`, reproduced by portuale verbatim).
- **M2 — run-rc on contained-die cells.** A contained `die()` still runs
  `ebuild-ipc exit 1` (`isolated-functions.sh:200`), killing Portage's IPC
  daemon; the end-of-phase `ebuild-ipc exit $?`
  (`phase-functions.sh:1198-1202`) then fails → Portage run-rc = 1 while
  portuale (no daemon, `PORTAGE_IPC_DAEMON` unset in all its cells) ends
  0. Systematic, identical mechanism in every death cell; the verdict
  compares helper rc + message + `${D}` listing (in `.plog`), not run-rc.
  The `die`-family cells (`t348-c-die-*`) are rc=1 on BOTH (top-level
  `nonfatal die` aborts the phase on both — genuine `same`).
- **M3 — `unset EBUILD_MASTER_PID`.** Without it a subshell-contained
  `die` SIGTERMs the master (`isolated-functions.sh:203-204`,
  `ebuild.sh:562`, `phase-functions.sh:1005-1008`) and aborts the run,
  losing later sub-cases. The die *text* is unaffected. Harness choice,
  documented, same for both PMs.
- **M4 — stream funnel.** Portage funnels ALL phase output to stdout
  (`.stderr` carries only config-time repo warnings); portuale splits
  (`echo`→stdout, `einfo`/`eerror`/`eqawarn`→stderr). Comparisons use
  stdout ∪ stderr. Related: portuale hardcodes `PORTAGE_QUIET=1`
  (`crates/portuale/src/ebuild_phases/phase_env.rs:169`) so `__vecho`
  progress lines (`>>> Unpacking…`, `=== Skipping unpack…`) are
  suppressed in portuale cells (S2/env question, not a helper verdict).
- **M5 — die footers.** Same text except portuale prints an empty repo
  name (`::` vs `::t348`) and omits the `build.log` convenience line
  (`isolated-functions.sh:175-196` logic vs portuale's renderer).
  Cosmetic; noted once, not per row.

## Verdicts by family (one row per cell; cites: Portage site → portuale site)

Legend: `same` / `diverges (fix)` / `diverges (cut, owner decides)` /
`not reproducible (why)`. Portuale `bin/` is vendored Portage except the
5 known files (plan §0 fact 1), so helper *code* cites are the same file
unless stated.

| cells (4 unless noted) | verdict | Portage cite | portuale cite |
|---|---|---|---|
| eapply file patch, `src_prepare`+`src_compile`, 7+8 | same | `phase-helpers.sh:1034-1100` (no phase gate on `eapply`) | vendored identical |
| `eapply_user`, `src_prepare` rc=0 both EAPIs | same | `phase-helpers.sh:1141-1145` | vendored identical |
| `eapply_user` in `src_compile`: `eapply_user() called during invalid phase: compile`, rc=1, 7+8 | same | `phase-helpers.sh:1143-1144` | vendored identical |
| `edo` absent, `declare -F` empty, call rc=127, 7+8, compile+install | same | `eapi.sh:38` (`___eapi_has_edo` = `!= [0-8]`, true only ≥9); defined `phase-helpers.sh:1231` | vendored identical |
| `ver_cut`/`ver_rs`/`ver_test` incl. false/bad-range rcs, 7+8 | same | `version-functions.sh:42+`; gated `eapi.sh:51`, `save-ebuild-env.sh:298` | vendored identical (only `declare` path differs) |
| `in_iuse foo` (IUSE=`+foo bar`): Portage rc=0, portuale rc=1, 7+8, compile+install | **diverges (fix)** | `phase-helpers.sh:1201-1210` reads `${IUSE_EFFECTIVE}`; Portage fills it `config.py:2181` | `phase_environ(&config, None)` drops declared IUSE in standalone (`crates/portage-profile/src/phase_environ.rs:265+`); phase env misses it |
| `use`/`usex`/`use_enable` on unknown flag: Portage `die "USE Flag … not in IUSE"` (empty out), portuale silent default (`no`/`B`/`--disable-…`), 7+8 | **diverges (fix)** | `phase-helpers.sh:269-285` strict branch needs `___in_portage_iuse`, generated `config.py:2185` (`BASH_FUNC____in_portage_iuse%%`) | no `BASH_FUNC____in_portage_iuse%%` anywhere in `crates/` → `declare -F` fails → strictness skipped |
| `usev` 2nd arg: EAPI 7 rc=1/empty, EAPI 8 rc=0/`altout`, identical both PMs | same (EAPI contrast captured) | `eapi.sh:86`, `phase-helpers.sh:224` | vendored identical |
| `get_libdir` plain + `ABI/LIBDIR_amd64`, 7+8 | same | `phase-helpers.sh:961-970` | vendored identical |
| `dosym -r` EAPI 7: rc=1 (`two arguments needed`, `-r` kept literally), EAPI 8: rc=0 relative link (`myapp`), both PMs | same (EAPI contrast captured) | `ebuild-helpers/dosym:8-12`, gated `eapi.sh:71` | vendored identical |
| `dostrip`/`dostrip -x`/`docompress`/`-x`: rcs + `PORTAGE_*` arrays identical, 7+8 | same | `phase-helpers.sh:163-215` | vendored identical |
| `dodoc` / `dodoc -r`, `${D}` listings identical, 7+8 | same | `ebuild-helpers/dodoc` | vendored identical |
| `doins` file+symlink preserved, listings identical, 7+8 | same | `ebuild-helpers/doins`, gated `eapi.sh:62` | vendored identical |
| `newins` file + `newins -` (stdin), contents+listings identical, 7+8 | same | `ebuild-helpers/newins`, gated `eapi.sh:73` | vendored identical |
| `einstalldocs` with `DOCS`/`HTML_DOCS` arrays, listings identical, 7+8 | same | `phase-helpers.sh:972-1002` | vendored identical |
| `econf` argv: EAPI 7 lacks `--datarootdir`/`--disable-static`, EAPI 8 has all; argv char-identical both PMs; `econf` in `src_compile` → `QA Notice: econf called in src_compile instead of src_configure` both PMs | same (EAPI contrast captured) | `phase-helpers.sh:514-560` flags, `:533-544` QA gate | vendored identical |
| `unpack`: tar.gz/txz/xz/abs/`./`-relative rc=0 both; uppercase `.TGZ` skipped rc=0 both (case-sensitive); `.7z`/`.rar` EAPI 7 attempt→die (rc=1 / unrar rc=13), EAPI 8 `=== Skipping unpack` rc=0; bare name DISTDIR-miss die rc=1; uout identical | same (EAPI contrast captured) | `phase-helpers.sh:332-430` suffix table (`eapi.sh:74-80`), `:404-412` path gates | vendored identical (+ M4 quiet-note for the skip line) |
| `has_version`/`best_version` plain/`-b`/`-d`/`-r`, `--host-root` die, invalid-atom die: ALL rcs identical (incl. `-b` rc=0 host-vdb hit, `-r`/plain rc=1 hermetic miss, `best_version` rc=0 empty) | same | `phase-helpers.sh:852-959`; `--host-root` gated `eapi.sh:54` (EAPI 5/6 only), `-b/-d/-r` gated `eapi.sh:55` | `portageq-wrapper` path (no `PORTAGE_IPC_DAEMON` in portuale cells); `bin/ebuild-ipc` (exits 127, #326 D6) unreachable here — see owner decision O1 |
| `die`/`assert`/`pipestatus`/`die -n`/`nonfatal die -n`/top-level `nonfatal die` (fatal both): all rcs identical; `pipestatus` absent both (rc=127, `eapi.sh:46` = `!= [0-8]`) | same | `isolated-functions.sh:78-128`, `phase-helpers.sh:31-39`, `:1213-1230` | vendored identical |
| `ENV_UNSET` (config-declared): sentinel unset both, 7+8; ebuild-declared layer inert in `ebuild` flow on BOTH | same | `ebuild.sh:159-163`, gated `eapi.sh:91`; `ENV_UNSET` is config-incremental (`const.py:129`, `special_env_vars.py:108`, `doebuild.py:224`) | vendored `ebuild.sh` identical; standalone config forwards it |
| `keepdir` new/existing rc=0 both BUT `.keep` filename: Portage `.keep_<cat>_<pn>-0`, portuale `.keep_<cat>_<pn>-` (empty SLOT), 7+8, compile+install | **diverges (fix)** | `ebuild-helpers/keepdir` uses `${SLOT%/*}`; Portage exports SLOT (`config.py:185`) | standalone phase env never sets/exports SLOT (no `SLOT` entry in `phase_env.rs:106-160` var list; only merge/entry paths own it — `phase_environ.rs:224-234`) |
| strict-keepdir effect | phase-level same; merge/QA-time only → see `not reproducible` row | `install-qa-check.d/95empty-dirs:22` | n/a (no merge in S0.6) |
| failglob global-scope non-match: `line 6: no match: …` twice, sourcing CONTINUES, phase reached, rc=0 — identical both PMs, 7+8 | same (oracle: tolerant, not fatal) | `ebuild.sh:631-632,690-691` | vendored identical |
| banned `dohtml`/`dosed`/`dohard`: `'…' has been banned for EAPI '7/8'`, identical, compile+install | same | `ebuild-helpers/dohtml:7-10`, `dosed:7`, `dohard:7`; gates `eapi.sh:31,34,29` | vendored identical |
| `hasq`/`hasv`/`useq`: EAPI 7 works + deprecation `eqawarn`, EAPI 8 `'…' banned in EAPI 8` — identical both PMs | same (EAPI contrast captured) | `isolated-functions.sh:488-499`, `phase-helpers.sh:217-222`; gates `eapi.sh:42,43,47` | vendored identical |
| `einstall`: banned in **7 and 8** (`'…' has been banned for EAPI '7/8'` identical) — brief said "in 8"; predicate is `== [0-5]` | same (brief corrected) | `phase-helpers.sh:679-683`, `eapi.sh:39` | vendored identical |
| `doconfd`/`doinitd`/`doenvd` + `insopts`/`exeopts`: EAPI 7 honors (600/750), EAPI 8 ignores (644/755) — identical both PMs | same (EAPI contrast captured) | `ebuild-helpers/doconfd` etc.; gates `eapi.sh:57,60,61` | vendored identical |

Tally: **86 cells → same 78, diverges (fix) 8** (`useflag-*` 4:
`c-t348-c-useflag-compile/install-{7,8}`;
`keepdir-*` 4: `c-t348-c-keepdir-compile/install-{7,8}`),
**diverges (cut) 0**, **not reproducible 1 aspect**
(strict-keepdir merge/QA effect: lives in `install-qa-check.d/95empty-dirs`,
not reachable via `ebuild <f> <phase>`; phase-level keepdir covered).

## EAPI-contrast oracle facts both PMs already agree on (no slices needed)

`usev` 2nd arg (8-only); `dosym -r` (8-only, 7 keeps `-r` literally);
`unpack .7z/.lha/.rar` (7-only; 8 skips) + case-sensitive suffixes (7+8);
`econf --datarootdir/--disable-static` (8-only); insopts/exeopts honored
(7) vs ignored (8); `hasq/hasv/useq` deprecated (7) vs banned (8);
`einstall` banned in 7 AND 8 (not just 8); `edo`/`pipestatus` absent in
7+8 (future-EAPI, rc=127); `best_version --host-root` in neither (5/6
only); `ENV_UNSET` honored 7+8 (config layer); failglob tolerant 7+8.

## Decisions for the owner (recorded, not asked — with default proposal)

- **O1 — `ebuild-ipc` 127 observability (per-helper port-or-cut, plan §4
  risk).** Portuale runs no IPC daemon (`PORTAGE_IPC_DAEMON` unset in all
  172 runs); `has_version`/`best_version` answer via `portageq-wrapper`
  with byte-identical results here (hermetic miss, host-root hits,
  `--host-root`/invalid-atom dies). `bin/ebuild-ipc` exiting 127 is thus
  currently UNREACHABLE through these helpers. Default proposal: keep the
  wrapper (cut the daemon) for `-b/-d/-r`/plain queries; record that any
  future helper calling `ebuild-ipc` directly inherits rc=127 +
  `die "… unexpected ebuild-ipc exit code: 127"`
  (`phase-helpers.sh:928-935`). S4 slice only if a real tree ebuild shows
  a gap.
- **O2 — `use()` strictness vs `IUSE_EFFECTIVE` (the 8 divergent cells).**
  Two faces of one gap: standalone phase env lacks (a) declared-IUSE in
  `IUSE_EFFECTIVE` and (b) `BASH_FUNC____in_portage_iuse%%`. Default
  proposal: fix (one S4 slice populating both from md5-cache IUSE +
  profile implicit set), since silent-false `use()` can flip real
  configure flags. Alternative (cut): document as known standalone-only
  gap if emerge-path env already carries both (unverified here).
- **O3 — SLOT export (keepdir `.keep` name).** Default proposal: fix
  (export md5-cache SLOT in standalone phase env); one-line-class change
  with visible `${D}` effect. No cut alternative beyond documenting.
- **O4 — `PORTAGE_QUIET=1` suppressing `__vecho` (M4).** Cosmetic;
  propose leaving to the S2 env slice (it also owns the stdout/stderr
  funnel and die-footer framing), no S4 action.
- **O5 — ebuild-declared `ENV_UNSET` inert in `ebuild` flow (both PMs).**
  Propose: no action (matches oracle; config layer verified working).

## Files created

- `LLM/evidence/2026-10-12-348-s0/c-helpers.md` (this file).
- `LLM/evidence/2026-10-12-348-s0/c/`: 860 capture files (172 runs ×
  argv/rc/stdout/stderr/plog).
- `LLM/evidence/2026-10-12-348-s0/scripts/`: `c-gen-a.sh`, `c-gen-b.sh`
  (probe generators), `c-run-cell.sh`, `c-run-all.sh`.
- `../pmtest/fixtures/repo-348/app-misc/t348-c-*/`: 43 packages, 86
  ebuilds + 86 verified md5-cache entries (no `Manifest`: thin-manifests).
  NOTE for the committer: pmtest `.git/info/exclude` ignores `/fixtures`
  (local rule), so these files are git-invisible — commit with
  `git add -f` as plan §1 prescribes.
- Configroot `/var/tmp/t348-c/` (outside both repos, kept for reruns).
- Removed one stray untracked `./--/var/…` dir (portage news/mtimedb
  state, likely an `egencache` side effect from this session;
  regenerable, no evidence value; neither PM's phase runs recreate it —
  verified by re-running a cell of each after removal).

## Repro

```sh
export PORTAGE_CONFIGROOT=/var/tmp/t348-c/cfg ROOT=/var/tmp/t348-c/root \
  PORTAGE_TMPDIR=/var/tmp/t348-c/tmp
./LLM/evidence/2026-10-12-348-s0/scripts/c-run-all.sh t348-c-dosym-install
diff <(sed -e 's|/usr/lib/portage/python3.14|BIN|' \
  LLM/evidence/2026-10-12-348-s0/c/c-t348-c-dosym-install-8-install-portage.plog) \
 <(sed -e 's|.*/portuale/bin|BIN|' \
  LLM/evidence/2026-10-12-348-s0/c/c-t348-c-dosym-install-8-install-portuale.plog)
```
