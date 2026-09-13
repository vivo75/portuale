# L2 — portuale as builder — agent plan (deepseek draft)

Status: **scoped, not started.** Written 2026-09-13 against `main` @
`fdb244a`. Covers backlog #29 (`docs/backlog-tasks.md:53-55`),
`docs/scope-backlog.md` §I L2 bullet (`:597-599`), the layer design
`docs/real-world-testing.md` §5 (`:98-114`) and the tooling spec §4
(`:73-94`). Companion history: `docs/history/real-world-testing.md` §5
(`:448-473`) and §14 item 4 (`:895-898`).

**Read first:** `AGENTS.md` (steps 4/5/7/8), `docs/agent-context.md`,
[`TEST/README.md`](../TEST/README.md), `docs/real-world-testing.md` §2
(determinism controls), §4 (archive tooling), §5 (L2), §7 (risks),
`TEST/compare/normalize.md` (last section = the archive rules),
`TEST/findings/l1.md` (the L1 harness's hard-won gotchas),
`TEST/images/overlay/porttest/README.md` (fixtures), and the existing
engine `TEST/run/l1-merge-from-binpkg.sh` +
`TEST/layers/l1/{build,consume}.sh`. Format authority: real
`/usr/lib/python3.14/site-packages/portage/gpkg.py` (esp. `:763`,
`:1002-1054`, `:1297-1338`) and portuale's
`rust/portuale/src/{ebuild_package.rs,binpkg.rs}` (module docs first).

Model tiers, same convention as 022/023/024/025:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Opus 5 / Fable 5.1 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Opinion

### 0.1 Verdict

**This is the right next Tier-3 item, but it is not just a test-bed
task — it is a producer-qualification task, and the bill is in
portuale bugs, not in harness code.**

- L1 validates the *consumer* half of binpkgs (merge an archive Portage
  built). L2 validates the *producer* half: portuale runs ebuild phases
  from source, assembles a real `.gpkg.tar`, and other package managers
  consume it. A bug here silently ships broken archives — the worst
  possible failure mode for a package manager — and nothing currently
  tests it above three hand-picked packages
  (`app-arch/unzip`, `sys-fs/fuse`, `app-arch/xz-utils`,
  `agent-context.md` "Real ebuild phase execution"). L2's value is high
  and non-duplicative.
- The harness cost is bounded because the hard-won pieces exist: the
  L1 orchestrator/build/consume pattern, `snapshot.sh`, `normalize.py` +
  `normalize.md` (which already specifies the L2 archive rules),
  the `porttest` fixtures, and a persistent pkgcache with 58
  Portage-built archives to develop the checkers against *without
  rebuilding anything* (`TEST/logs/_l1-pkgcache/`).
- The real risk is that portuale's from-source build at L1-set scale
  (~18 packages incl. `app-admin/sudo`) is barely exercised. Expect
  fetch/phase/packaging bugs. That does not make L2 wrong — it makes it
  *the instrument that finds them* — but it means the slice must be
  planned as "harness first, bring-up second, fixes third", with a
  stop rule, not as a weekend script.
- Keep it narrow. L2 is structure + cross-install, not parity: no
  `SOURCE_DATE_EPOCH` plumbing, no xpak axis, no `@system`, no HTTP
  binhost, no `mrg`. Those are L3/L4/L5 and the plan's own build order
  (`real-world-testing.md:96`).

### 0.2 Frontier or cheap model?

**Hybrid, and the split is clean.** The work is ~55-60% mechanical
harness (S1-S3, S6) and ~40% bring-up/triage (S0, S4, S5). The
mechanical half can be executed by a mid model following this plan,
because every interface is frozen here and there is a known-good corpus
to test against. The bring-up half needs a frontier model: it is
cross-language debugging (bash phases → Rust packaging → real Python
Portage semantics) where the failure modes are *misclassification*
(allowlisting a real portuale bug, or blaming portuale for compiler
nondeterminism), not lack of code.

| Slice | Content | Tier | Why |
|---|---|---|---|
| S0 | recon spike: portuale `-B` one fixture, portage consumes it | **F** | unknown unknowns; decides G0.1/G0.2 |
| S1 | `gpkg-structure.sh` + host-side mutation tests | M | mechanical, with a 58-archive oracle |
| S2 | `gpkg-diff.sh` + `diff.py --layer` | M (F review) | mechanical; one design call (payload bucket) |
| S3 | L2 builders + orchestrator, fixture track green | M | copies the L1 pattern; fast feedback |
| S4 | cross-install A + tolerance wiring on the real set | **F** | triage, evidence bar |
| S5 | real-set bring-up; fix/file portuale bugs | **F** | real bugs, real semantics |
| S6 | docs closeout | S/M | mechanical |

If only a cheap model is available: it can land S1+S2+S3 and stop
cleanly; S0/S4/S5 must be reassigned or the task will produce a green
run that means nothing.

### 0.3 Difficulty, by axis

| Axis | 1-5 | Notes |
|---|---|---|
| Bash orchestration | 2 | L1 is a working template; copy, don't invent |
| Archive-format knowledge | 3 | nested tar + optional zstd + multi-instance naming; all verified below |
| Comparison semantics | 4 | compiled payloads legitimately differ; deciding what is "hard" is the judgment call |
| Triage (portuale vs portage vs environment) | 4-5 | the actual cost; needs real-source grounding |
| Environment | 4 | rootful podman, 1.7 GB image, network fetches, tens of minutes per run |
| **Overall** | **3.5** | plumbing easy, truth hard |

---

## 1. Ground truth

### 1.1 What exists (reuse, do not rebuild)

- **L1 engine**: `TEST/run/l1-merge-from-binpkg.sh` (orchestrator;
  `ensure_portuale_built` + `ensure_image`), `TEST/layers/l1/build.sh`
  (Portage build from source → shared `$PKGDIR`),
  `TEST/layers/l1/consume.sh` (one PM merges a `$PKGDIR` into a fresh
  container + snapshots) — the last two are directly reusable for L2.
- **Compare stack**: `TEST/compare/snapshot.sh` (`--paths`,
  `--vdb-list`), `normalize.py` (`norm_metadata`, `norm_environment`,
  `norm_contents` are importable module-level functions), `diff.py`
  (typed findings + allowlist), `normalize.md` (already contains the
  `gpkg / xpak archive (gpkg-diff.sh, L2)` ruleset section).
- **Fixtures**: `TEST/images/overlay/porttest/` — nine `EAPI=8`
  fixtures, no `SRC_URI`, seconds to build; live-mounted by the
  orchestrators. `atomlists/l1-porttest.txt` and `atomlists/l1-merge.txt`
  (10 real atoms) are the two sets.
- **A format oracle on disk**: `TEST/logs/_l1-pkgcache/` holds **58
  Portage-built `.gpkg.tar`s** incl. two multi-instance instances of
  most packages (`-1`/`-2` BUILD_IDs), a `Packages` index, and the
  `porttest/` fixtures. S1/S2 can be developed and mutation-tested with
  zero container runs.
- **Portuale's writer + tests**: `--buildpkg[only]` is a shipped
  action (`emerge_options.rs:245,299`; `emerge_build.rs:118`
  `run_buildpkgonly`, `:344` `entry_buildpkg_wanted`), multi-instance
  `BUILD_ID` naming (`ebuild_package.rs:683` `allocate_binpkg_build_id`),
  gpkg assembly (`binpkg.rs:1515` `build_gpkg`), and a real black-box
  round trip in `tests/test_portuale.py:1918,2185,2919`
  (`--buildpkgonly` gpkg + multi-instance + signed).
- **Volumes/network scaffolding**: `TEST/net/up.sh:21` already defines
  `porttest-{pkgdir,distfiles,bincache,snapshots}` (L1 ignores them; L2
  should use a host dir for distfiles instead — see S3).

### 1.2 What is missing

- `TEST/compare/gpkg-structure.sh` and `TEST/compare/gpkg-diff.sh`
  (spec'd, not written).
- A portuale source-builder layer and the L2 orchestrator
  (`TEST/layers/l2/*`, `TEST/run/l2-portuale-builder.sh`).
- L2 report handling (the `l2-report.{txt,json}` + symlink convention).
- Cross-install direction A (real Portage consuming a Portuale-built
  archive) — direction B (portuale consumes Portage-built) is L1 and
  already green.
- `TEST/findings/l2.md`.

### 1.3 Format facts the scripts must encode (verified on disk
2026-09-13)

Real gpkg layout, from `_l1-pkgcache/dev-libs/libbsd/libbsd-0.11.8-2.gpkg.tar`:

```
<cat>/<pn>/<pf>-<BUILD_ID>.gpkg.tar          # multi-instance layout
└── <pf>-<BUILD_ID>/
    ├── gpkg-1                                # 0-byte format-version marker
    ├── metadata.tar.zst                      # flat members `metadata/<KEY>`
    ├── image.tar.zst                         # members `image/...`
    └── Manifest                              # DATA lines: SHA512 + BLAKE2B
    [optional *.sig sidecars, e.g. metadata.tar.zst.sig, Manifest.sig]
```

- Compression is a *suffix*: `metadata.tar` / `metadata.tar.zst`
  (`gpkg.py:847-869`, `_get_inner_tarinfo`). Both forms must be
  accepted; the gate uses `BINPKG_COMPRESS` default = zstd
  (`ebuild_package.rs:50`).
- `metadata.tar` has **no `CONTENTS`** — the merged VDB `CONTENTS` is
  computed at install time. Do not require it.
- `Packages` stanza fields (from the same cache):
  `BUILD_ID`, `BUILD_TIME`, `CPV`, `DEFINED_PHASES`, `EAPI`,
  `KEYWORDS`, `MD5` (of the archive file), `PATH`, `SHA1`, `SIZE`,
  `USE`, `MTIME`, `REPO`.
- Multi-instance naming is active by default (the `Packages` header
  `FEATURES:` shows `binpkg-multi-instance`, `buildpkg`).

### 1.4 Traps found while scoping (encode these, don't rediscover them)

1. **`diff.py`'s allowlist filter is hardcoded to `"l1"`**
   (`TEST/compare/diff.py:155`: `if e.get("layer") not in (None,
   "l1")`), and the report header/json name are hardcoded
   (`:206`, `:236`). An L2 entry in `known-divergences.yaml` is
   silently ignored until this is parameterized.
2. **The layer design's language is xpak-flavoured.** `real-world-testing.md:83-86`
   says strip `build-info/BUILD_TIME` etc. — for gpkg those live as
   `metadata/<KEY>` members, not `build-info/`. Implement the *intent*
   (blank `BUILD_TIME`/`BUILD_ID`/`COUNTER`, normalize
   `environment.bz2` through the same rules as `normalize.py`) with the
   gpkg member names.
3. **`SOURCE_DATE_EPOCH` is not referenced anywhere** in `TEST/` or
   `rust/portuale/src/` today. It is an L3 concern; L2 does not need it
   because `BUILD_TIME`/`BUILD_ID` are normalised away (G0.4).
4. **No shared distfiles cache in L1.** Each `l1` build re-downloads.
   L2 builds the same set twice (once per PM) — share one distfiles
   dir or the second builder pays the network twice (and the two
   builders *must* see identical distfiles anyway).
5. **Payload hashes are not comparable for compiled packages.** Two
   Portage builds of the same package already differ in bytes; portuale
   vs portage will too. Strict (payload) comparison is only meaningful
   for the `porttest` fixtures. The real-set gate is
   structure/metadata/path parity (S4).

---

## 2. Rules and invariants for every slice

1. **Never weaken L1.** Any change to `diff.py` / `normalize.py` /
   `snapshot.sh` must keep
   `TEST/run/l1-merge-from-binpkg.sh TEST/atomlists/l1-porttest.txt`
   green (0 unexplained). Default behaviour stays L1; L2 is opt-in via
   flags/`--layer`.
2. **No silent allowlisting.** A hard finding is triaged into exactly
   one of: portuale bug (fix it), portage bug (entry with `ticket:`),
   environmental (entry with `reason:`). New `known-divergences.yaml`
   entries need evidence (the exact two archives/commands) and an
   `owner:`. `compiler-nondeterminism` as a blanket reason is
   forbidden; use the discriminator in S4.
3. **gpkg gate first.** The xpak (`.tbz2`) axis is out until gpkg is
   green (`real-world-testing.md:175-176`).
4. **Real-execution only, no Python mirror.** Everything here is
   TEST/-level; portuale features exercised (source builds, packaging)
   have no `emerge_pretend_reference.py` counterpart, so AGENTS.md's
   lockstep rule does not apply. If a portuale **bug fix** is needed:
   Rust fix + a Rust end-to-end test in the relevant crate, Rust-only.
   No contract `CASES` entry, no Python mirror (AGENTS.md step 4's own
   carve-out).
5. **Same-run comparisons only.** A green L2 report must compare
   snapshots/archives produced in the same run; cross-run comparisons
   are investigation aids, never the gate.
6. **Bounded scope.** Touch `TEST/` and, only for triaged bugs,
   `rust/`. Do not refactor the resolver/merge paths "while here".
7. **Environment discipline.** Heavy runs (container builds) are not
   part of every slice's pytest/cargo pass. S1/S2 must be fully
   testable on the host with `_l1-pkgcache`; only S0/S3/S4/S5 need
   podman.

---

## 3. Slices

### S0 — Recon spike: can portuale build and package a fixture? (F, 1-2 h)

**Goal:** de-risk the whole task before any harness code exists.
Decides G0.1 (`-b` vs `-B`) and G0.2 (tolerance model) with data.

Steps:

1. Control run: `TEST/run/l1-merge-from-binpkg.sh
   TEST/atomlists/l1-porttest.txt` → must be green (0 unexplained).
2. Scratch dirs: `TEST/logs/_l2-spike/{pkgs-portuale,pkgs-portage,distfiles}`.
3. In a `podman_run_portuale` container (see `TEST/run/lib.sh:23`), with
   the `porttest` overlay mounted and `PKGDIR=/pkgs`:
   ```sh
   emerge --buildpkgonly --oneshot --color=n porttest/docs porttest/phases
   ```
   Then a build+merge variant in a second fresh container:
   ```sh
   emerge -b --oneshot --color=n porttest/docs
   ```
4. Inspect the produced archives by hand:
   ```sh
   tar -tvf <pf>-<id>.gpkg.tar
   d=$(mktemp -d); tar -xf <archive> -C "$d"
   zstd -dc "$d"/*/metadata.tar.zst | tar -tf -
   zstd -dc "$d"/*/image.tar.zst    | tar -tf -
   ```
   Compare member names/compression against a Portage-built sibling in
   `TEST/logs/_l1-pkgcache/porttest/`.
5. Cross-consume: fresh container, real Portage:
   ```sh
   PKGDIR=/pkgs-portuale emerge -k --getbinpkg --oneshot porttest/docs porttest/phases
   ```
   Assert rc 0 and that `$ROOT` + `/var/db/pkg/...` look complete.
6. Write `TEST/findings/l2.md` (new): recipes, raw output, the archive
   shape actually produced, and the S0 verdict.

**Acceptance:** at least one portuale-built fixture archive is
produced, structurally matches the S1.3 shape, and is consumed by real
Portage with rc 0 — or a minimal portuale bug is filed (exact command,
expected vs actual) and S0 stops.

**Stop rule:** if portuale cannot package a *dep-free* fixture at all,
STOP: file the finding, do not build harness code on a broken
foundation, and schedule the fix as its own slice. This is the
frontier-model gate that makes S1-S3 safe to delegate.

### S1 — `TEST/compare/gpkg-structure.sh` (M, 3-5 h)

**Goal:** structural validator for one archive or a whole `$PKGDIR`.
Pure host tool; no container needed.

Interface:
```
gpkg-structure.sh <archive.gpkg.tar>...          # per-archive checks
gpkg-structure.sh --dir <PKGDIR> [--packages]    # walk + index cross-check
```
Exit 0 clean / 1 findings / 2 usage-IO. One `[CATEGORY] archive: detail`
line per finding.

Checks (S1.3 is the spec):

- outer tar readable; exactly one top-level `<pf>-<BUILD_ID>/`
  directory; members exactly `{gpkg-1, image.tar[.comp],
  metadata.tar[.comp], Manifest}` plus optional `*.sig`; `gpkg-1`
  zero bytes; no absolute or `..` member paths.
- inner tars decompress (`zstd -t` where compressed), are non-empty,
  and have a single root (`image/...`, `metadata/...` respectively).
- `metadata/`: required keys present (derive the exact required set
  empirically from `_l1-pkgcache`; at minimum `CATEGORY`, `PF`,
  `EAPI`, `SLOT`, `repository`, `BUILD_ID`, `BUILD_TIME`, `SIZE`,
  `environment.bz2`, one `*.ebuild`); `PF` matches the path;
  `BUILD_ID` matches both the `<prefix>/` name and the filename suffix.
- `Manifest`: parses; one `DATA` line per inner member; SHA512/BLAKE2B
  hex lengths valid; member set equals the container's member set.
- `--packages`: for each archive, a stanza exists with matching
  `PATH`/`BUILD_ID`/`BUILD_TIME`/`REPO`, `SIZE` == `stat`, `MD5` ==
  `md5sum` of the file itself; multi-instance siblings accepted, a
  duplicate `BUILD_ID` flagged.
- `--dir`: legacy flat `<pf>.gpkg.tar` accepted only when no
  multi-instance sibling exists (non-fatal note).

**Host tests (no podman):** run over all 58 archives in
`TEST/logs/_l1-pkgcache` → 58/58 pass. Then copy one archive and inject
five mutations, each must be caught: truncated file; a member removed;
a `Manifest` DATA line removed; `Packages` `MD5` spoofed; filename
`BUILD_ID` mismatched.

**Acceptance:** 58/58 clean + 5/5 mutations caught, commands recorded
in `TEST/findings/l2.md`.

### S2 — `TEST/compare/gpkg-diff.sh` + `diff.py --layer` (M, F review, 4-8 h)

**Goal:** normalised archive-vs-archive diff, and make the shared
`diff.py` usable for L2.

Interface:
```
gpkg-diff.sh [--mode strict|payload-tolerant] <a.gpkg.tar> <b.gpkg.tar>
```
Buckets, with a per-bucket count and a non-zero exit on any hard
finding outside the allowlist:

- `outer-layout` — member set / prefix / marker (hard).
- `metadata:<KEY>` — all `metadata/*` compared after blanking
  `BUILD_TIME`, `BUILD_ID`, `COUNTER`; `NEEDED`/`NEEDED.ELF.2`/
  `REQUIRES`/`PROVIDES` sorted line-wise; `environment.bz2`
  decompressed and normalised with the *same code* as
  `normalize.py:norm_environment` (import it — do not duplicate the
  ruleset); `repository` trimmed (hard).
- `image:paths` — path set equality, and type/mode/owner/xattr parity
  (hard in both modes).
- `image:payload` — sha256 per file. `strict` = hard (porttest
  fixtures); `payload-tolerant` = counted/reported only (real set).
- Manifest/`Packages` diffs are their own bucket, hard modulo
  normalised fields.

**`diff.py` changes** (default behaviour unchanged):
- add `--layer <l0|l1|l2|...>` (default `l1`) and use it in
  `explained()`'s filter (`:155`) and for the report header/json name
  (`:206`, `:236`);
- add `--tolerate-payload`: `CONTENT` findings (and `CONTENTS` `obj`
  md5 lines) become non-fatal but stay listed under an "expected
  payload (compiler nondeterminism)" section and counted in the JSON.

Implementation choice (G0.5): `gpkg-structure.sh` is bash;
`gpkg-diff.sh` is a thin bash wrapper over a new
`TEST/compare/gpkg_diff.py` that extracts and imports the
`normalize.py` functions, so the rules have exactly one home.

**Host tests:** two Portage-built instances of the same package
(e.g. `_l1-pkgcache/virtual/logger/logger-0-r3-{1,2}.gpkg.tar`) must be
0 hard in `strict` mode (BUILD_TIME/BUILD_ID blanked); a Portage-vs-
Portage `porttest` pair likewise. Inject mutations on `metadata/EAPI`,
`metadata/CFLAGS`, an image path, a file mode, a payload byte — each
caught in its bucket. Re-run L1 to prove `--layer l1` is a no-op.

**Acceptance:** pairs clean; 5/5 mutations caught; L1 green.

### S3 — L2 builders + orchestrator, fixture track (M, 3-6 h)

**Goal:** the runnable L2 bed, proven on the `porttest` set (fast, no
real source builds) where strict archive comparison is meaningful.

New files:
- `TEST/layers/l2/build-portage.sh` — `layers/l1/build.sh` + shared
  `DISTDIR`, `MAKEOPTS=-j1`, optional `SOURCE_DATE_EPOCH`
  (per G0.4), separate `$PKGDIR`.
- `TEST/layers/l2/build-portuale.sh` — same contract, `EM=/usr/local/bin/emerge`,
  `--buildpkgonly --oneshot` (per G0.1), porttest staging identical to
  `layers/l1/build.sh:46-56`, `BINPKG_FORMAT=gpkg`.
- `TEST/run/l2-portuale-builder.sh [atomlist]` — default
  `atomlists/l1-merge.txt`; caches
  `_l2-pkgcache-{portage,portuale}`, `_l2-distfiles`; env
  `L2_REBUILD`, `L2_SKIP_BUILD`, `L2_JOBS`, `PORTTEST_*`. Flow:
  1. build Portage set → `_l2-pkgcache-portage`;
  2. build portuale set → `_l2-pkgcache-portuale`;
  3. `gpkg-structure.sh --dir` both (control + candidate);
  4. `gpkg-diff.sh` each paired archive (strict for `porttest/`,
     payload-tolerant otherwise);
  5. cross-install: reuse `layers/l1/consume.sh` — reference =
     Portage ← Portage pkgdir, candidate A = Portage ← portuale pkgdir,
     plus the L1 control Portuale ← Portage pkgdir;
  6. `normalize.py` + `diff.py --layer l2` A vs reference;
  7. report `TEST/logs/l2-<ts>/l2-report.{txt,json}` + symlink
     `TEST/logs/l2-report.txt`.

Report must state per package: structure pass/fail, archive-diff
buckets, cross-install findings, and an explicit
`unexplained == 0` verdict.

**Acceptance:** `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`
runs end-to-end and is green (or every finding adjudicated); L1 still
green; the orchestrator is idempotent (`L2_SKIP_BUILD=1` reuses).

### S4 — Cross-install direction A + tolerance wiring, real set (F, 4-10 h)

**Goal:** real Portage consumes Portuale-built archives of the
`l1-merge.txt` set; structure/metadata/path parity holds.

Steps:
1. Run the S3 orchestrator on `l1-merge.txt` (depends on S5 for the
   portuale build to succeed).
2. Triage `diff.py --layer l2` findings by the §3 triage table in
   `docs/real-world-testing.md:53-71`: `MISSING`/`MODE`/`OWNER`/
   `XATTR`/`SYMLINK`/`VDB` must be **zero**; `CONTENT` + `CONTENTS`
   md5 diffs on compiled payloads are expected and must be *classified*.
3. **Discriminator for payload diffs (recommended, G0.2):** build the
   reference set twice with Portage (`_l2-pkgcache-portage-r2`) and
   compute the set of paths whose payload already differs between two
   Portage builds. A portuale-vs-portage payload diff on a path
   *outside* that set is a real finding; inside, it is compiler
   nondeterminism. This is evidence, not a blanket allowlist.
4. Reverse control: Portuale ← Portage pkgdir must stay clean (it is
   the known-good L1 case; a regression here means the harness broke
   something).
5. Record the verdict table in `TEST/findings/l2.md`.

**Acceptance:** candidate vs reference has zero unexplained hard
structural findings; payload findings are a subset of the
nondeterminism set; both controls green.

### S5 — Real-set bring-up: portuale builds the L1 closure (F, time-boxed 8-16 h)

**Goal:** make `build-portuale.sh` succeed on `l1-merge.txt` (10 atoms
+ installed-dep closure, incl. `app-admin/sudo`, `sys-apps/miscfiles`,
`app-shells/bash-completion`), fetching real distfiles with manifest
verification.

For every failure:
- capture the exact command + expected (real `emerge`, same container)
  vs actual (portuale) in `TEST/findings/l2.md`;
- decide: **portuale bug** → fix in `rust/` with a Rust e2e test, then
  re-run; **gap already in `scope-backlog.md`** → file with the repro
  and continue if not blocking;
- never paper over it with `--usepkg`, `-k`, `FEATURES=-buildpkg`, or a
  smaller atomlist. The set is the test.

**Time-box rule:** if more than three distinct portuale bugs block the
build, STOP after filing them (with repros), do not land L2 as green,
and propose the follow-up ordering. Escalate to the user.

**Acceptance:** every `l1-merge.txt` atom builds from source under
portuale; archives pass S1; S4's cross-install is green.

### S6 — Docs closeout (S/M, 1-2 h)

- `TEST/README.md`: replace the "L2+ (not yet implemented)" stanza
  (`:99-106`) with the L2 runbook (commands, outputs, env, host vs
  container split).
- `docs/real-world-testing.md` §5 L2: mark shipped with the live
  evidence pointer; §7 risks updated (HTTP server still L3+).
- `docs/scope-backlog.md` §I: L2 bullet (`:597-599`) closed/updated
  with real status; L3-L5 untouched.
- `docs/backlog-tasks.md:55` #29 → DONE with a one-line pointer.
- `docs/what-this-proves.md`: append one slice paragraph with a
  runnable, live-verified example (AGENTS.md step 7).
- `TEST/findings/l2.md`: final verdict table.

---

## 4. Routing summary

| Slice | Tier | Effort | Depends on | Deliverable |
|---|---|---|---|---|
| S0 | **F** | 1-2 h | image present | findings + G0.1/G0.2 data |
| S1 | M | 3-5 h | none (host only) | `gpkg-structure.sh` + tests |
| S2 | M (F review) | 4-8 h | S1 helper | `gpkg-diff.sh` + `diff.py --layer` |
| S3 | M | 3-6 h | S1, S2 | `layers/l2/*`, `l2-portuale-builder.sh`, porttest green |
| S4 | **F** | 4-10 h | S3, S5 | real-set cross-install green |
| S5 | **F** | 8-16 h | S3 | portuale builds the closure; findings fixed/filed |
| S6 | S/M | 1-2 h | all | docs + verdict |

Total: 24-49 agent-hours, realistically 4-6 sittings. Cheap models can
run S1→S2→S3 in sequence once S0 has produced a portuale-built archive
to test S2's portuale half against; without S0 they can still do S1/S2
host-side against `_l1-pkgcache`.

---

## 5. Gates (owner decisions before the dependent slice)

- **G0.1 `-b` vs `-B` for `builder-portuale`.** Recommendation: `-B`
  (`--buildpkgonly`) for archive production — L2's gate is archives, and
  `-b` adds a self-merge whose root-state comparison belongs to L3.
  Keep a `-b` smoke run on `porttest` only. Owner: user. (S0 supplies
  the data; asked before S3.)
- **G0.2 Payload-tolerance mechanism.** Recommendation: structure is
  always hard; payload diffs are reported and classified via the
  two-Portage-builds nondeterminism set (S4.3), never via a blanket
  allowlist. Owner: user. Asked before S4.
- **G0.3 xpak axis.** Recommendation: defer (gpkg first,
  `real-world-testing.md:175-176`). Not part of L2 v1.
- **G0.4 `SOURCE_DATE_EPOCH`.** Recommendation: not required for L2
  (`BUILD_TIME`/`BUILD_ID` normalised); L3's item. Do not implement
  portuale-side without a slice.
- **G0.5 tool implementation.** Recommendation: keep the spec'd `.sh`
  names; `gpkg-structure.sh` bash; `gpkg-diff.sh` a wrapper over
  `compare/gpkg_diff.py` importing `normalize.py`'s functions.
- **G0.6 allowlist ownership.** A human/user adjudicates every new
  entry; agents propose with evidence (two archives + commands).

---

## 6. Definition of done

- [ ] S0 findings recorded and G0.1/G0.2 answered.
- [ ] `gpkg-structure.sh` passes all Portage archives and catches all
      injected mutations.
- [ ] `gpkg-diff.sh` clean on same-package Portage pairs (modulo
      normalised fields) and catches all injected mutations.
- [ ] `diff.py --layer l2` in place, L1 unchanged/green.
- [ ] `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`
      green.
- [ ] `TEST/run/l2-portuale-builder.sh` (real set) green or every
      finding adjudicated and filed, with the verdict in
      `TEST/findings/l2.md`.
- [ ] Full verification pass (AGENTS.md step 8) still green:
      `cargo fmt --check`, `cargo clippy --release --all-targets`,
      `cargo test --release`, `python3 -m pytest tests -q`.
- [ ] L1 `porttest` run still green after compare-stack changes.
- [ ] S6 docs updated.

## 7. Review checklist (attach to each slice)

- Does the change keep L1's default path bit-identical in behaviour?
- Is every new finding class justified against
  `docs/real-world-testing.md:53-71` triage categories?
- Any new allowlist entry: evidence, `reason:`, `owner:`, `layer: l2`?
- Does any "expected diff" hide a structural difference (path, mode,
  owner, symlink, missing file)? If yes, it is not expected.
- Are the archive rules in `normalize.md` still in sync with the code?
- Are commands/results appended to `TEST/findings/l2.md`, not just in
  the chat?

## 8. Risks

1. **Portuale source build at scale is the unknown.** Three packages
   live-verified vs ~18 here. Budget is dominated by this; the S5
   time-box and stop rule exist for it.
2. **Fetch.** Real distfiles, mirrors, `->` renames, digests. A shared
   `_l2-distfiles` avoids repeat downloads but not portuale fetch bugs.
3. **Compiled-payload nondeterminism.** Mitigated by strict-only-for-
   fixtures + the S4 discriminator; a cheap model that "fixes" this by
   allowlisting is the failure mode to watch.
4. **`-B` semantics.** Portuale's `buildpkgonly` gate
   (`emerge_build.rs:101-118`) may behave subtly differently; S0 must
   confirm before S3 hard-codes it.
5. **Heavy environment.** Rootful podman + 1.7 GB image + network;
   runs are serial and tens of minutes. Cache aggressively
   (`_l2-*` dirs), keep host-only testing for S1/S2.
6. **Harness drift.** Duplicating `normalize.py`'s rules in bash would
   rot; G0.5's shared-module approach is the mitigation.
7. **Allowlisting pressure to "go green".** Rule 2 + the review
   checklist; the user adjudicates.

## 9. Non-goals

- L3 source parity, `@system`/`@world`, `SOURCE_DATE_EPOCH` plumbing.
- xpak (`.tbz2`) axis; HTTP binhost; `mrg`; L5 lifecycle/fault
  injection; profile matrix; metrics/trend (§8 of the design is for
  the L3 soak — L2 may emit a metrics file opportunistically, but does
  not gate on it).
- Any portuale feature work beyond fixing bugs this bed surfaces.
- Rewriting L1 scripts: L2 must reuse `layers/l1/consume.sh` and the
  compare stack.

## 10. Findings filed while executing

(append here as S0-S5 run; one entry per finding: command, expected,
actual, root cause, fix ref / backlog id)
