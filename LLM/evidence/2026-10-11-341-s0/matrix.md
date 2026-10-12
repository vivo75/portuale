# S0 matrix: Portage vs portuale atom-grammar captures (`#341`)

Status: S0 evidence, 2026-10-11. No product code (plan §S0).
Plan: `LLM/02.341-eapi-atom-grammar.opus.md`. Oracle:
`LLM/evidence/2026-10-11-341-oracle/report.md` (Portage 3.0.82.2 + native C parser).

## 0. Provenance and method

- Image: `localhost/test-portuale:latest`
  (`5616d41d00cdbef0aa8933476973dc5b9ca07e897b635bd7167dedf61c09cb5f`,
  created 2026-09-30). `emerge --version` in image: `Portage 3.0.82.2
  (python 3.14.6-final-0, default/linux/amd64/23.0/systemd, …)`.
  Native parser on (default): the G-cells below re-prove the C-vs-PY
  split live in this image (same texts as oracle Q2c).
- portuale: `/pm/portuale` = host `target/release/portuale`, rev
  `ea93910b` (branch `backlog/341-eapi-atom-grammar`), binary mtime
  2026-10-11 04:35 UTC. Invoked as `/pm/portuale emerge …`
  (argv[0]-dispatch equivalent of the `emerge` symlink).
- Base argv every cell (memory "getbinpkg ⇒ bdeps off probe trap"):
  `emerge -pv --ignore-default-opts --getbinpkg=n --usepkg=n …`
  (binary cell uses `--usepkgonly=y` instead).
- Isolation: per-cell `PORTAGE_CONFIGROOT`+`ROOT` in container
  `/s0/out/work/cfg-*` (ad-hoc-root precedent, pmtest
  `test_emerge_pretend_contract.py:7868 _binscan_configroot` — Step 1:
  `ls fixtures/repo-341*` absent + `grep -rl t341 fixtures` empty
  before creation; precedent reused, no new helper committed).
- Fixtures: `../pmtest/fixtures/repo-341/` (17 ebuilds + `profiles/t341`
  with `eapi`=5 and `profiles/t341/noeapi/` without one, each carrying
  `package.mask`, `package.use` (`<atom> x`), `package.accept_keywords`,
  `packages` with oracle-run4's eight lines, t341-adapted) and
  `../pmtest/fixtures/repo-341-formats/` (same `profiles/`, layout with
  `profile-formats = portage-2 profile-repo-deps build-id
  profile-default-eapi` + `profile_eapi_when_unspecified = 5`).
  md5-cache generated with the image's `egencache --update --repo t341`
  (Step 2, RC=0); spot check: `t341-good-1` entry `_md5_` =
  `md5sum` of the ebuild (`da57fed9…`). `repo-341` added to
  `test_fixture_caches.py` `FIXTURE_ROOTS` (only it: `-formats` has no
  ebuilds/md5-cache, and the `assert checked` guard would fail it).
- Layout: this dir holds `portage/<cell>.{argv,rc,stdout,stderr}`,
  `portuale/…` likewise, `merged-B1.txt`/`merged-C2.txt` (2>&1 position
  proof), `scripts/` (the four in-container drivers that built every
  cell: `s0-capture.sh`, `s0-sectionC.sh`, `s0-sectionE.sh`,
  `s0-sectionX.sh`). Scratch configroots/PKGDIR/vdb root used by the
  runs are container-path-bound (`/s0/out/work/…`) and intentionally
  NOT committed; the scripts rebuild them.
- Reading guide: `<cell>.argv` = full argv (portuale cells show
  `/pm/portuale emerge …`); `.rc` = exit code; `.stdout`/`.stderr` =
  raw split streams. "Stream" column below is per Portage line;
  "Dup" = how many times the identical line occurs in that cell's
  Portage capture (miss/warm = mtimedb state, see §1).

## 1. Global findings (apply to every row)

1. **Streams.** Every Portage `--- Invalid atom …` /
   `--- Atom is not allowed …` warning goes to **stderr**, printed
   while the config loads, i.e. **before `Calculating dependencies`**
   (`merged-B1.txt`: 46/46 warning lines precede line 50
   `Calculating dependencies ... done!`). Merge list, `Total:`,
   eselect-news notes → stdout. `invalid:` masks, `!!!`, `emerge:
   error:` (argparse exclusion text + usage), `emerge: there are no
   ebuilds…`, `Validation failed …`, `ParseError`, `SyntaxWarning`,
   tracebacks → stderr. portuale's `masked by:` lines are also stderr
   (verified: `A-ebuild-t341-badcond` portuale stdout 0 B). —
   So S4/S5/S6 print warnings to stderr at config-load position.
2. **Cache-warmth doubling (mtimedb).** A *fresh* configroot (no
   `$ROOT/var/cache/edb`) prints every config-load warning **twice**;
   the *same* configroot re-run prints each **once**. Proven:
   `B-prof-nopackages` (70) vs its rerun (35); `cfg-fresh` with
   `var/` deleted (70) vs warm (35); stored `B-prof-eapi5-noeapi`
   (92 = 46×2) vs warm rerun (46); stored `C-userconfig` (66 =
   27×2 + 4×3) vs warm rerun (35). Contract pins use fresh per-test
   configroots, so pins will see the **doubled** (miss-state) output
   the stored cells show — deterministically, as long as each test
   runs emerge once on a fresh root. S5/S6 must pin miss-state text.
3. **User-profile `packages` is re-read mid-resolve.** Warm
   `C-userconfig`: every warning ×1 **except**
   `profile/packages` lines (×2); `merged-C2.txt` shows the second
   copy emitted *during* resolve (line 35 starts `Calculating
   dependencies ...` and the packages warnings interleave before
   `done!`). Configroot miss-state: ×3 (2 at load + 1 mid-resolve).
   Repo-profile `packages` does NOT get the extra read (warm B1 =
   exact ×1, 46 lines). Ambient `/etc/portage` run (`C-ambient.*`,
   same files, no configroot): user/profile files ×1,
   `profile/packages` ×2 — same rule, so it is not a configroot
   artefact. (First S0 draft wrongly blamed the `packages` file for
   whole-profile doubling; `s0-sectionX.sh` + `B-prof-nopackages`
   isolate it to mtimedb miss. The doubled `ParseError` in
   `B-prof-eapi10` (×2, miss-state) is the same miss effect.)
4. **Per-file stack order.** Profile readers walk the whole stack per
   file, parent before child: `packages`, `package.use`,
   `package.accept_keywords`, `package.mask` (B1 stderr order), each
   `t341/` then `t341/noeapi/`. `packages` strips a leading `*`
   (`*/t341-dep` → `/t341-dep`, then invalid).
5. **Noise (never pin).** First run per fresh configroot prints
   `Performing Global Updates …` (stdout) and eselect-news counts
   vary boot to boot (`7` vs `15 news items`). `ROOT` leaks into
   output (`for <root>/`, `to <root>/`, `in <path>`): pins must
   normalise it. `--exclude` failures dump argparse `usage:` text to
   stderr (4.7 KB) with exit **2**.
6. **portuale cosmetic deltas (out of scope, all rows):** merge lines
   lack the trailing `0 KiB` size (`USE="-x" ` + trailing space);
   otherwise byte-identical where the verdict is `same`.

## 2. Group A — ebuild / binary `*DEPEND` (`A-ebuild-*`, cfgA)

Config: `repo-341` in repos.conf, `make.profile` → gentoo default
(no t341 profile files read — no `---` warnings in any A cell).

| cell (`app-misc/…`) | Portage (rc / stderr text) | portuale (rc) | verdict |
|---|---|---|---|
| t341-badrepo (`::gentoo`) | 1 / `invalid: RDEPEND: Invalid atom (dev-libs/t341-dep::gentoo), token 1` | 1 / `there are no ebuilds to satisfy "dev-libs/t341-dep::gentoo"` | S4 fixes |
| t341-badbid (`=…-1-2`) | 1 / `… Invalid atom (=dev-libs/t341-dep-1-2), token 1` | 1 / `there are no ebuilds … "=dev-libs/t341-dep-1-2"` | S2+S4 |
| t341-badcond (`[x?]`, IUSE="") | 1 / `… USE flag 'x' referenced in conditional 'x?' … is not in IUSE` (bare, no wrapper) | 1 / identical | same |
| t341-badslot (`:0*`) | 1 / `… Invalid atom (dev-libs/t341-dep:0*), token 1` | **0 / merges** | S4 fixes |
| t341-built (`:0/1=`, unbuilt) | 1 / `… Improper context for slot-operator "built" atom syntax: dev-libs/t341-dep:0/1=` | **0 / merges** | S4 fixes |
| t341-token (`x? ( … ) …::gentoo`, IUSE=x) | 1 / `… Invalid atom (dev-libs/t341-dep::gentoo), token 5` | 1 / `there are no ebuilds … ::gentoo` | S4 fixes |
| t341-twokeys (DEPEND `::a` + RDEPEND `:0*`) | 1 / `… invalid: DEPEND: Invalid atom (dev-libs/t341-dep::a), token 1, invalid: RDEPEND: Invalid atom (dev-libs/t341-dep:0*), token 1` | 1 / `there are no ebuilds … ::a` (RDEPEND reason lost) | S4 fixes |
| t341-good (`:=` + `!!…-badslot`) | 0 / merges dep (`USE="-x"`) + self | 0 / identical (modulo §1.6) | same |
| t341-eapi7 (`[x(+)]`, EAPI 7) | 1 / merge list + stderr USE-change prompt (`>=dev-libs/t341-dep-1 x`) — `(+)` itself valid | 1 / merge list + **byte-identical** prompt | same |
| t341-mixcat (BDEPEND `y?` + DEPEND `::a` + RDEPEND `z?`, IUSE="") | 1 / `… BDEPEND: USE flag 'y' … is not in IUSE, RDEPEND: USE flag 'z' … is not in IUSE, DEPEND: Invalid atom (dev-libs/t341-dep::a), token 1` — **category-grouped, BDEPEND/RDEPEND/DEPEND** | 1 / BDEPEND+RDEPEND only (DEPEND `::a` silently skipped) | S4 fixes |
| t341-twobuilt (PDEPEND 2× `:s/ss=`) | 1 / two `… PDEPEND: Improper context … :0/1=` + `:0/2=` | 1 / `there are no ebuilds … ":0/2="` | S4 fixes |
| t341-emptygrp (`x? ( )`) | 1 / `… expected: dependency string, got: ')', token 2` | **0 / merges** | S4 fixes |
| t341-glued (`…dep(`) | 1 / `… missing whitespace around '(' at 'dev-libs/t341-dep(', token 1` | **0 / merges** | S4 fixes |
| t341-xor (`^^ ( … )`) | 1 / `… Invalid atom (^^), token 1` | **0 / merges dep + good + self** (`^^` ignored) | S4 fixes |
| t341-qq (`?? ( … )`) | 1 / `… USE flag '?' referenced in conditional '??' is not in IUSE` | 1 / identical | same |
| t341-condfirst (`y? ( …::a )`, IUSE="") | 1 / `… Invalid atom (dev-libs/t341-dep::a), token 3` (atom beats conditional) | 1 / `… USE flag 'y' … is not in IUSE` (conditional first) | S4 fixes (order) |
| dev-libs/t341-dep (control) | 0 / merges `USE="-x"` | 0 / identical | same |

All Portage mask lines: stderr ×1 (A cells have no profile warnings;
ebuild cells are not subject to the §1.2 doubling).

## 3. Group B — repo profiles (`B-prof-*`)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| eapi5-noeapi (repo-341, profile → `noeapi`) | rc 1. stderr: 92 warn lines miss / 46 warm (§1.2; order §1.4). EAPI-5 node keeps `:=`, `:0`, `[x(+)]`; noeapi node drops them with EAPI-0 messages; `::t341`/`-1-2` dropped in both; `*/…` dropped (`packages` strips `*` → `/t341-dep`); use-carrying lines in `package.use`/`accept_keywords` → `not allowed`. End state: `t341-dep-1` **masked by package.mask** (valid `:=` line) | rc 1, **zero warnings**, `masked by: package.mask` (raw-line match, wrong line set) | same end-state; S5 fixes |
| formats (`-formats` repo: `profile-repo-deps build-id profile-default-eapi` + key=5) | rc 1. `::t341` and `=…-1-2` accepted everywhere **except** `package.accept_keywords` (build-id never passed there — oracle table confirmed); noeapi node reads as EAPI 5 (no EAPI-0 messages); only `*/…`, `…-1`, use-in-usefiles still warned. End state: `there are no ebuilds to satisfy "dev-libs/t341-dep"` + `searching for similar…`/`Maybe you meant…` (stderr) — valid masks hide the only ebuild | rc 1, zero warnings, `there are no ebuilds…` (no suggestions) | same end-state; S5 fixes |
| nokey (scratch: key present, `profile-default-eapi` **absent** from formats) | rc 1. Python `SyntaxWarning` (stderr, ×1 — warnings-module dedup): `…/config.py:1694: SyntaxWarning: Repository named 't341' has profile_eapi_when_unspecified setting in '…/layout.conf', but 'profile-default-eapi' is not listed in the profile-formats field. …` + `warnings.warn(` frame line; noeapi falls back to EAPI 0 (Slot-deps/Use-deps messages); end state `there are no ebuilds…` + suggestions | rc 1 `there are no ebuilds…` (nokey layout not modelled; scratch paths unresolvable outside container) | S5 fixes |
| eapi10 (scratch: `noeapi/eapi` = `10`) | rc 1. `!!! ParseError: Profile contains unsupported EAPI '10': '…/noeapi/eapi'` ×2 (stderr, miss-state) + `Your current profile is invalid … Allowed actions are limited …` abort | rc 1 `masked by: package.mask` (no ParseError path — reads the node as valid) | S5 fixes |
| nopackages (scratch: no `packages` files; §X control) | rc 1, 70 miss / **35 warm** — isolates §1.2 (file set, not `packages` presence, drives the count) | n/a (same reader as eapi5-noeapi) | — (method cell) |

## 4. Group C — user config (`C-*`, run6-shaped per-file content)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| userconfig (mask/unmask bare; use/accept `+ x`; license `+ MIT`; env minus-`[` `+ t341.conf`; properties bare; profile `{mask,use,use.force,packages}`) | rc 0, merges `USE="(x)"`. stderr: 66 warn lines miss / 35 warm; per-file outcomes: `=*/…`, `=…-02`, `…-1` dropped everywhere; `::t341`/`=…-1-2` kept except license (`=…-1-2` needs build-id → dropped); `[x]` → `not allowed` in use/accept files, kept in mask/unmask/license/env/properties/profile-mask+packages. `profile/packages` ×3 miss / ×2 warm (§1.3) | rc 0, merges `USE="x"`, **zero warnings** (all bad lines silently kept-or-dropped without a word) | S6 fixes |
| userenv-crash (`package.env`: only `dev-libs/t341-dep[x] t341.conf`) | rc 1, `AssertionError: setcpv recursion detected` + full traceback (stdout 39 lines incl. merge header attempt; traceback stderr) — reproduces oracle run6's crash cell | rc 0, merges quietly | out of scope: Portage bug, S6 documents the cut (plan §S6) |
| ambient (same files, `/etc/portage`, no configroot) | rc 0; user/profile files ×1, `profile/packages` ×2 (27 lines). Tainted file set (image `package.accept_keywords` dir collision, §8.3) — qualitative only | n/a | — (method cell) |

## 5. Group D — command line and `--*-exclude` (`D-*`, cfgD)

Bad package atoms (Portage `run_action` text): two stderr lines
`!!! '<x>' is not a valid package atom.` /
`!!! Please check ebuild(5) for full details.`, rc 1, stdout empty.

| cell | Portage | portuale | verdict |
|---|---|---|---|
| cmd-bid `=…-1-2` | 1 / stderr `emerge: there are no ebuilds to satisfy "=dev-libs/t341-dep-1-2" for <root>/` (accepted as build-id; ebuilds carry none) | 1 / same line | same (parse differs — portuale rejects the atom — but line+rc coincide; S2 makes the reason match) |
| cmd-wild `*/t341-dep` | 1 / two `!!!` lines | 1 / stderr `emerge: invalid atom "*/t341-dep"` | S7 fixes |
| cmd-blocker `!…` | 1 / two `!!!` lines | **2** / `… is a blocker, not a valid emerge target` | S7 fixes |
| cmd-ver `…-1` | 1 / two `!!!` lines | 1 / `emerge: invalid atom …` | S7 fixes |
| cmd-cond `…[x?]` | 0 / merges `USE="-x"` | 0 / identical | same |
| cmd-shortrepo `t341-dep::t341` | 0 / merges `USE="-x"` | 0 / identical | same |
| cmd-plain (control) | 0 / merges | 0 / identical | same |
| cmd-badbid0 `=…-1-02` | 1 / two `!!!` lines | 1 / `there are no ebuilds … "=…-1-02"` (line coincides, reason differs) | S7 (+S2) |
| excl-op `--exclude '>=…-1'` + 4 siblings (repo/bid/use/blocker) | 2 / stderr `emerge: error: Invalid Atom(s) in --exclude parameter: '<a>' (only package names and slot atoms (with wildcards) allowed)` + 4.7 KB argparse `usage:` | 1 / `there are no ebuilds …` (option **not validated**, dep resolved, then target missing) | S7 fixes |
| excl-two (two `--exclude`) | 2 / `… parameter: 'dev-libs/t341-dep::t341,>=dev-libs/t341-dep-1' (only …)` — comma-joined | 1 / unvalidated (as above) | S7 fixes |
| excl-wild-ok / short-ok / slot-ok (`:0/1`) / allslot-ok (`*/*:0`) | 0 / `Total: 0 packages` (dep excluded) | wild-ok/short-ok/allslot-ok: **0 / `Total: 1`** (exclusion ignored); slot-ok: 1 / `there are no ebuilds` | S7 fixes |
| usepkg-excl-repo | 2 / `… in --usepkg-exclude parameter: '…' (only …)` | 0 / merges (ignored) | S7 fixes |
| bpkgexcl-bid-ok / repo-ok / op-ok | 0 / `Total: 1` (accepted) | 0 / `Total: 1` | same (texts coincide; acceptance rule needs a unit test in S7, not deducible from acceptance alone) |
| bpkgexcl-blocker `!…` | 2 / `emerge: error: Invalid Atom(s) in --buildpkg-exclude parameter: '!dev-libs/t341-dep'` — **no** `(only …)` suffix | 0 / merges (ignored) | S7 fixes |

## 6. Group E — sets and world (`E-*`, cfgE2, run5's eight lines)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| sets-s341 `@s341` | rc 1. stdout merge list (`USE="x"`); stderr USE-change prompt (`# required by @s341`) **then** six `…/sets/s341: Validation failed at line: N, data X` (lines 1,2,4,5,6,7; 3 `::t341` and 8 `[x]` accepted) — failures print **after** the merge list | rc 1 / aborts `emerge: invalid atom "*/t341-dep"` | S8 fixes |
| sets-selected `@selected` (world = same eight lines) | rc 1. stdout merge list; stderr USE-change prompt; **zero** `Validation failed` lines — world drops silently | rc 1 / aborts (as above) | S8 fixes |

## 7. Group F — binary (`F-*`)

Build: `BINPKG_FORMAT=xpak … emerge --buildpkg=y --oneshot app-misc/t341-good`
(rc 0, `F-build.*`); `Packages` + xpak `RDEPEND` rewritten to
`dev-libs/t341-dep::gentoo` (oracle-run3 procedure; `s0-capture.sh` F section).

| cell | Portage | portuale | verdict |
|---|---|---|---|
| binary-rewritten (`--usepkgonly=y app-misc/t341-good`) | 1 / `masked by: invalid: RDEPEND: Invalid atom (dev-libs/t341-dep::gentoo), token 1` (binary deps validated with `eapi`, same as ebuilds) | 1 / `there are no ebuilds to satisfy "dev-libs/t341-dep::gentoo". (dependency required by "app-misc/t341-good-1::t341" [binary]) …` (atom accepted, match fails) | S4 fixes (+S9 for the positive build-id match) |

## 8. Group G — installed vdb (`G-*`; S10 evidence)

Install: `ROOT=$S0W/root-inst emerge --oneshot app-misc/t341-good`
(rc 0, `G-install.*`); vdb `RDEPEND` rewritten per cell, cache
invalidated (`rm -f /var/cache/edb/vdb_metadata.pickle`), then
`pkgprobe.py` (oracle's, `scripts/`-adjacent in `/var/tmp/pmtest-341`,
copied into work dir at run time) with C and `PORTAGE_NATIVE_DEP_PARSER=0` parsers.

| vdb RDEPEND | C (`*.stdout`) | PY (`*-py.stdout`) | verdict |
|---|---|---|---|
| `dev-libs/t341-dep::gentoo` | `invalid: {'dependency.syntax': ["RDEPEND: expected '?' in '…/RDEPEND'"]}`, visible False | `invalid: False`, visible True | S10 (owner cut, §9 Q1) |
| `=dev-libs/t341-dep-1-2` | `… "RDEPEND: expected atom or group in '…/RDEPEND'"`, False | True | S10 (cut) |
| `dev-libs/t341-dep[nonexistent?]` | False / True (IUSE check skipped, both parsers) | same | same (no behaviour to port) |
| `dev-libs/t341-dep:0*` | `… "RDEPEND: unexpected token in '…/RDEPEND'"`, False | `… "RDEPEND: Invalid atom (dev-libs/t341-dep:0*), token 1 in '…/RDEPEND'"`, False | S10 (cut) |

Traversal: with poisoned vdb (`::gentoo`), `emerge -pv dev-libs/t341-dep`
(`G-inst-poisoned-emerge`, ROOT=root-inst): C rc 0 `[ebuild R …]`
(installed entry invisible → reinstall), PY identical; **portuale rc 0
identical `[ebuild R …]` line** → same. The depgraph `eapi=pkg.eapi`
traversal rule (depgraph.py:4352-4379) is source-level, checked by
reading, either way.

## 9. Step 5 — owner questions (RESOLVED per brief; recorded, not asked)

- **Q1 (S10): document as a deliberate cut, no code.** The C-scanner
  quirk (table §8: `expected '?'` / `expected atom or group` /
  `unexpected token` for `::repo`/build-id atoms under `eapi=None`)
  is NOT replicated. S10 writes one "Deliberate cuts" entry in
  `LLM/backlog-tasks-2026-10.md` citing oracle run3 (report §Q2c) and
  the G-cells above (`G-vdb-*`). Portage never writes such atoms into
  the vdb itself, so the reachable impact is nil. The traversal rule
  (§8, last row) behaves the same on both sides.
- **Q2 (warning placement): stderr, config-load position.** §1.1 is the
  record: every Portage warning line in every cell is stderr-only;
  load-time warnings precede `Calculating dependencies`
  (`merged-B1.txt`); the user-profile `packages` re-read lands
  mid-resolve (`merged-C2.txt:35-38`). S4/S5/S6 print to stderr at
  config load (before `Calculating dependencies`); per-line stream
  data is in the `.stdout`/`.stderr` splits.
- **Q3 (doubled warnings): portuale prints each warning ONCE.**
  Neither repo-profile doubling (miss-state §1.2) nor user-profile
  `packages` re-read doubling (§1.3) is reproduced; each distinct
  warning is emitted once. Documented as a cut in the S5/S6 slices
  (pins assert single warnings against Portage's miss-state doubles —
  the pin docstring cites this matrix).

## 10. Residue and anomalies vs the plan

1. **First-draft user-config cell crashed Portage and was redone.**
   Bare atoms in `package.env`/`package.use` (my invention, not
   oracle run6) put a use-dep atom in `package.env`, which trips
   `AssertionError: setcpv recursion detected` during *every* resolve
   (traceback in the superseded first `C-userconfig` capture —
   overwritten by the faithful redo; mechanism: `setcpv` →
   `ordered_by_atom_specificity` → `best_match_to_list` →
   `match_from_list` → `x.use` → `setcpv`). Lesson kept: `package.env`
   lines need the ` t341.conf` suffix and `[`-lines excluded, exactly
   like `run6.sh`. The faithful `C-userenv-crash` cell (only
   `dev-libs/t341-dep[x] t341.conf`) still crashes — that one is
   oracle-faithful and stays.
2. **The plan's "eight lines" differ per oracle run.** Profiles use
   run4's set (in fixtures, `U8`); user config needs run6's set (with
   `dev-libs/t341-*`, `=…-1-02`, `!…`, plain `[x]`); sets/world need
   run5's bare set. The main driver and the sectionC/E redos encode
   all three (`scripts/`).
3. **`C-ambient` is tainted, not normative.** Same files in
   `/etc/portage` without configroot failed with `cp: cannot overwrite
   directory …/package.accept_keywords` (image ships that path as a
   dir), so its file set differs; recorded for the qualitative rule
   only (user files ×1 ambient).
4. **News/updates noise** (§1.5) and the `ROOT` path leak must be
   normalised by every pin. `B-prof-nokey`/`B-prof-eapi10` scratch
   repos live only in the drivers (`scripts/s0-capture.sh`); S5
   re-creates them per-test.
5. **No `portage-dep` unit-test battery here** — S0 is captures-only;
   the nine oracle flag modes belong to S2's TSV generator, which
   re-runs the probes (plan §S2 Step 1), not to this matrix.
6. **pmtest dirt untouched.** `fixtures/etc/profile.env` and
   `fixtures/var/lib/portage/world` were already `M` before S0 (noted
   in the review); S0 neither includes nor reverts them. S0's own
   pmtest-side changes: new `fixtures/repo-341/`,
   `fixtures/repo-341-formats/`, +1 `FIXTURE_ROOTS` entry
   (additive). No commit (no grant).

## 11. Verdict tallies (Portage → portuale)

- `same`: A-badcond, A-good, A-eapi7, A-qq, A-dep, D-cmd-bid(line+rc),
  D-cmd-cond, D-cmd-shortrepo, D-cmd-plain, D-bpkgexcl-{bid,repo,op}-ok,
  G-inst traversal, G `[nonexistent?]` pair — 13 cells.
- `S2 fixes` (with S4/S7 where noted): A-badbid, D-cmd-bid(reason),
  D-cmd-badbid0 — 3 cells.
- `S3 fixes`: none directly (wildcard user-config lines surface in
  S5/S6 pins: B `*/…` drops, C `dev-libs/t341-*` handling).
- `S4 fixes`: all other A cells (11), F-binary — 12 cells.
- `S5 fixes`: all B cells (end-state same, warnings+rules differ) — 4 cells.
- `S6 fixes`: C-userconfig — 1 cell.
- `S7 fixes`: D-cmd-{wild,blocker,ver}, D-excl-{op,repo,bid,use,blocker,two},
  D-excl-{wild,short,slot,allslot}-ok, D-usepkg-excl-repo,
  D-bpkgexcl-blocker — 12 cells.
- `S8 fixes`: E-sets-{s341,selected} — 2 cells.
- `S9`: no divergent cell yet (positive build-id match needs binpkg
  builds with BUILD_ID; F cell covers the negative side).
- `out of scope` (cuts): C-userenv-crash (Portage bug), G C-vs-PY
  quirk rows (S10 cut), §1.6 cosmetics.
