# #38 — Packaging transforms: dostrip / splitdebug / docompress — agent plan (claude draft)

Status: **plan only, not executed. Blocked on #37 (S1+S2 at least).**
Written 2026-09-13 against `main` @ `ec13936`, every citation re-read in
this checkout. Backlog: `docs/backlog-tasks.md:74`, `scope-backlog.md` §K
(`:669-672`). Findings: `TEST/findings/l2.md` `l2-gpkg-docompress`
(`:121-132`), `l2-gpkg-dostrip-splitdebug` (`:255-260`). Sibling drafts:
`038_Packaging-transforms.{deepseek,musespark}.md`; merged canonical plan:
`038_Packaging-transforms.plan.md`.

Model tiers: F = frontier (Opus 5 / Fable 5.1), M = mid (Sonnet 5), S =
small (Haiku 4.5); "F review" = frontier reads the full diff before the
user is asked to commit.

---

## 0. Opinion

### 0.1 Verdict — this is mostly #37's tail; do not write a stripper

Three facts, verified in this checkout, decide the shape:

1. **The transform call sites are in `install_qa_check`, and portuale
   already runs it unmodified.** `bin/misc-functions.sh:147-152`
   (`ecompress` under `binpkg-docompress`) and `:239-250` (`estrip` under
   `binpkg-dostrip`) live inside `install_qa_check()` (`:77`).
   `ebuild_phases::run_commands_async` runs `install_qa_check
   install_symlink_html_docs install_hooks` right after every successful
   `install` phase (`ebuild_phases.rs:2996-3029`), on every source path:
   `emerge <atom>` merge (`ebuild_merge.rs:2719`), scheduler builds,
   `--buildpkgonly` (`ebuild_package.rs:455`), and `ebuild <file>
   install|merge|package`. **musespark's draft is wrong that the call
   site is `__dyn_package` (`:540`) and that the merge path "gets
   neither"** — the default-on path is already wired everywhere; it is
   inert only because the gate variables are wrong.
2. **The gates are false today for #37 reasons only.** `FEATURES` in the
   phase env is the raw process env (`phase_features_value`,
   `ebuild_phases.rs:1581`) — the L2 harness exports `buildpkg
   binpkg-multi-instance splitdebug xattr …` (`TEST/layers/l2/
   build-portuale.sh:28`) with **neither** `binpkg-dostrip` nor
   `binpkg-docompress`; real gets both from `make.globals` because
   `FEATURES` is incremental. `PORTAGE_COMPRESS="bzip2"`
   (`cnf/make.globals:108`) is never exported by portuale (whitelisted
   at `ebuild_phases.rs:1469`, never set), so even with the token
   present `ecompress` would `die` (`ecompress:228-229`). Both are
   delivered by #37's builder (`FEATURES` via `resolved_incremental`,
   `PORTAGE_COMPRESS*` via `other_vars` ∩ real's `environ_whitelist`).
3. **The `-binpkg-*` complement is a merge-time phase real runs from
   `dblink.treewalk()`** (`vartree.py:4440-4450`: `doebuild_environment
   (…, "instprep")` + `EbuildPhase(phase="instprep")` → `__dyn_instprep`,
   `misc-functions.sh:265-308`) — for **both** source merges and binpkg
   merges (`EMERGE_FROM == "binary"`). Portuale's treewalk mirror
   (`ebuild_merge.rs`) never runs it (`ebuild.rs:38` lists `instprep` as
   unsupported). With the default `binpkg-*` on, `__dyn_instprep` is a
   near no-op (`chflags` handling + touching `.instprepped`), so the
   default-on story is complete without it; the `-binpkg-*` story is not.

So: **#38 = (verify the default-on path fires once #37 lands) + (pin the
outcome with the two existing fixtures against the real oracle archives)
+ (decide `instprep` explicitly)**. Expected code delta in Rust: near
zero for the default path — perhaps an `extra_env` pair or two that the
recon proves missing — plus one optional new misc-function invocation
for `instprep`. The risk is not implementation; it is (a) misclassifying
integration failures (missing tool, version skew between vendored `bin/`
and the image's portage 3.0.82.2) as transform bugs, (b) double-transform
if `instprep` is wired without real's complement condition, and (c)
declaring victory on the allowlist rather than on the oracle.

### 0.2 Where I disagree with the sibling drafts

- **musespark's `__dyn_package`/merge-path claim** — factually wrong, see
  0.1 (1). Its G0.2 "invoke `__dyn_instprep` before `merge_tree`" is
  right in spirit, but it must be positioned as the `-binpkg-*`
  complement, not as the mechanism that makes the default path work.
- **deepseek slices B/C "fix env gaps through #37's builder"** — agree,
  but if the recon shows a gap it is a #37 bug (its allowlist missed a
  whitelisted var); fix it in #37's builder and *test it there*, not in
  #38's diff.
- **deepseek G2.5 "strip/objcopy must be byte-deterministic"** — over
  scoped for #38. L2 strict mode compares portuale-vs-real *of the same
  fixture in the same image*; two builds of the same input with the same
  binutils are deterministic enough for that. Determinism *claims*
  belong to L3; #38 only needs "same bytes as real for the same input".
- **musespark's `RESTRICT`/`FEATURES` matrix as fixtures** — good, but
  only cells that the real oracle can grade. A matrix cell needs a real
  archive built with the same setting; the existing `_l1-pkgcache` has
  default settings only. Limit v1 to: default strip, `FEATURES=
  splitdebug`, `RESTRICT=strip` (one new tiny fixture, both PMs build
  it), `FEATURES=nostrip` (same). Skip `installsources`/`dedupdebug`/
  `compressdebug` (file if touched).

### 0.3 Frontier or cheap model?

| Slice | Content | Tier | Why |
|---|---|---|---|
| S0 | recon: build the two fixtures under #37's env, read the logs, list tools, classify every non-firing branch | **F** | unknown-unknowns; classification quality decides the rest |
| S1 | docompress: close `l2-gpkg-docompress` against the oracle | M | one script, one fixture, payload bytes either match or not |
| S2 | dostrip + splitdebug: close `l2-gpkg-dostrip-splitdebug*` | M (F review) | `estrip` is dense; tool-absence semantics; `.build-id` layout |
| S3 | `RESTRICT=strip` / `nostrip` fixtures | M | two tiny fixtures both PMs build; oracle-graded |
| S4 | `instprep` decision (implement or file #38b) | **F** | new merge-path phase incl. binpkg merges (L1 blast radius); user call |
| S5 | closeout: allowlist deletion, real-set re-run, docs | M/S | mechanical + one container run |

If only a cheap model is available: S1, S2, S3, S5 after an F-written S0
table. S0 and S4 stay frontier.

### 0.4 Difficulty

| Axis | 1-5 | Notes |
|---|---|---|
| Rust code | 1-2 | near zero for default path; one misc-function call if S4 implements |
| Bash comprehension | 3 | `estrip` ~700 lines, `ecompress` ~260; read to *classify*, not to port |
| Integration/triage | 4 | tool presence, version skew, env provenance, `QA_PRESTRIPPED` noise |
| Test design | 3 | oracle-driven; new fixtures need both PMs to build them |
| **Overall** | **3** | low volume, high observation cost; hinges on #37 |

Effort: **~12-24 agent-hours, 3-4 sittings** after #37 S2; +4-8 h if S4
implements `instprep`.

---

## 1. Ground truth (verified at `ec13936`)

- Gates: `misc-functions.sh:149` `[[ ${PORTAGE_COMPRESS} ]] &&
  contains_word binpkg-docompress "${FEATURES}"` → `ecompress --queue
  "${PORTAGE_DOCOMPRESS[@]}"` / `--ignore "${PORTAGE_DOCOMPRESS_SKIP[@]}"`
  / `--dequeue`; `:243` `contains_word binpkg-dostrip "${FEATURES}"` →
  `estrip --queue "${PORTAGE_DOSTRIP[@]}"` / `--ignore` / `--dequeue`
  (EAPI 7+; else `--prepallstrip`). Complements at `:282` and `:290` in
  `__dyn_instprep`.
- Arrays: `PORTAGE_DOCOMPRESS=(/usr/share/{doc,info,man})`,
  `PORTAGE_DOCOMPRESS_SKIP=(/usr/share/doc/${PF}/html)`,
  `PORTAGE_DOCOMPRESS_SIZE_LIMIT=128`, `PORTAGE_DOSTRIP=(/)` for EAPI 7+
  (`phase-helpers.sh:21-28`) — bash defaults sourced by `misc-functions.sh`
  via `ebuild.sh`; ebuilds extend them with `docompress [-x]` /
  `dostrip [-x]` (`:164-220`). They survive from `src_install` into
  `install_qa_check` through `$T/environment` (only the *final*
  `--exclude-init-phases` save drops them, `save-ebuild-env.sh:20-29`).
  Nothing for Rust to do.
- `estrip` (`:404-460`): `has_feature[compressdebug dedupdebug
  installsources nostrip splitdebug xattr]` from `FEATURES`;
  `has_restriction[binchecks dedupdebug installsources splitdebug strip]`
  from `PORTAGE_RESTRICT`; `RESTRICT=strip` or `FEATURES=nostrip` →
  banner off, skip (unless `installsources`). Tools: `debugedit`, `dwz`,
  `${CHOST}-`{`objcopy`,`ranlib`,`readelf`,`strip`} (`:462-470`), `scanelf`
  (`:393`). `debugedit` absent → `eqawarn` once and continue without
  build-ids (`:176-181`). Reads `PORTAGE_STRIP_FLAGS` (optional),
  `STRIP_MASK` (exported by `misc-functions.sh:244`), `KERNEL`, `CHOST`,
  `ED`/`D`/`T`, `SLOT` (in `installsources` path).
- `ecompress`: dies without `PORTAGE_COMPRESS` (`:228-229`); default flags
  per compressor when `PORTAGE_COMPRESS_FLAGS` is *unset* (`:232-247` —
  `-v` unset test, so an exported empty value would be honoured as
  empty: export it only if the config sets it); skips already-compressed
  suffixes (`:87`); needs `find0`/`___parallel` from
  `isolated-functions.sh` and the compressor on `PATH`.
- Image tools: `dev-util/debugedit-5.3`, `app-misc/pax-utils-1.3.10`
  (`TEST/logs/l1-20260913T013910Z/portage.installed-before.txt`),
  binutils, `bzip2` (stage3). Real oracle archives:
  `TEST/logs/_l1-pkgcache/porttest/{docs,splitdebug,setuid}/*.gpkg.tar`.
- Fixtures: `TEST/images/overlay/porttest/porttest/{docs,splitdebug,
  setuid}` (`README.md:32-36` documents expected compress/no-compress
  split and the `.debug`/`.build-id` tree). `KNOWN_FINDINGS` regexes:
  `TEST/run/l2-portuale-builder.sh:65-71`; yaml: `known-divergences.yaml:
  98-134` (`l2-gpkg-dostrip-splitdebug{,-contents,-libptsd,-dirs}`).
- Ordering inside `install_qa_check`: the `scanelf` NEEDED writer
  (`:154-236`) runs before `estrip` (`:239`) — strip after capture (bug
  749624); portuale's own `NEEDED.ELF.2` writer (`needed_elf.rs`) is #39's
  concern and unaffected by strip (`DT_NEEDED` unchanged).
- `PORTAGE_RESTRICT` in portuale is USE-reduced with an *empty* USE for
  non-`depend` phases (`ebuild_phases.rs:2079-2086`) — a `RESTRICT="!x?
  ( strip )"` real package would diverge; after #37's `USE` lands this
  should use the same effective set (file against #37 if the recon shows
  it, do not patch `estrip`).

---

## 2. Scope

**In:** default-on `binpkg-dostrip` + `binpkg-docompress` firing on all
source paths via the existing `install_qa_check` call; `FEATURES=
splitdebug` output parity (`.debug` + `.build-id`); `RESTRICT=strip` and
`FEATURES=nostrip` pinned by two tiny oracle-graded fixtures; `CONTENTS`
recording post-transform paths/md5s on a source merge; deleting the
temporary allowlist entries; an explicit decision on `instprep`.

**Out (file, don't absorb):** `packdebug` (`__generate_packdebug`),
`installsources`/`dedupdebug`/`compressdebug` beyond non-regression,
xattr/selinux/chflags branches, `NEEDED.ELF.2` field count and gpkg
metadata members (#39), reimplementing `estrip`/`ecompress` in Rust,
editing vendored `bin/*` to make a test pass.

---

## 3. Gates

- **G1 Trigger source is the resolved config only.** No Rust-side switch,
  no hardcoded `PORTAGE_COMPRESS=bzip2`. If the branch doesn't fire, the
  bug is in #37's builder; fix and test it there. Owner: agent.
- **G2 `instprep`.** Recommendation: **file #38b in S4 unless S0 shows the
  L2/L3 harness configures `-binpkg-*`** (it does not today). Implementing
  it means a new phase invocation in `ebuild_merge`'s treewalk mirror on
  *every* merge including binpkg merges (L1 must be re-run), gated by
  real's own `__dyn_instprep` conditions (idempotent via `.instprepped`).
  Owner: user; S0 presents the call-order table.
- **G3 Shell backend.** Prove on `bash` (default); run `porttest/
  splitdebug` once under `--shell brush` and file (not fix) if the
  external `estrip`/`ecompress` subprocesses misbehave under the brush
  parent. Owner: agent.
- **G4 Expected outputs come from the real oracle archives**, never from
  the fixture's own README (it is a claim, not a proof). Owner: agent.
- **G5 New fixtures must be built by both PMs** (the L2 bed does this)
  and must isolate one cell each (`porttest/restrict-strip`, `porttest/
  nostrip`). Names checked for collisions under `fixtures/repo/` and the
  overlay first. Owner: agent.

**Evidence bar:** `gpkg-diff.sh --mode strict` reports no `BIG.txt`,
`usr/lib/debug`, `libptsd`, `pt-` rows for `docs`/`splitdebug`/`setuid`
with the corresponding `KNOWN_FINDINGS`/yaml entries *deleted*; a green
run that still matches an allowlist row is not acceptance.

---

## 4. Slices

### S0 — Recon under #37's env (F, 3-5 h, no product code)

Precondition: #37 S2 landed (resolved `FEATURES`, `PORTAGE_COMPRESS*`,
`USE`, `SLOT` in the phase env; visible as `metadata/FEATURES` containing
`binpkg-docompress binpkg-dostrip`).

1. Build `porttest/{docs,splitdebug,setuid}` with `--buildpkgonly` and
   with `-b` in the L2 container; keep the build logs.
2. For each: did `install_qa_check` reach `ecompress`/`estrip` (grep the
   log for the `>>> Compressing`/`strip:` banners and any `eqawarn`)?
   Which tools were found (`estrip` names them)? Diff the archive image
   path set against the oracle.
3. Table into `TEST/findings/l2.md` (or `l2-transforms.md` if long):
   fixture × branch → fired?/output equal?/cause class
   `{portuale-env, container-tool, version-skew, real-divergence,
   fixed}`; tool presence table; the real `instprep` call order with
   citations (§1) for G2.
4. If a `portuale-env` row exists: file it against #37 (its builder
   missed a whitelisted var) and stop #38 until it lands.

### S1 — docompress (M, 2-4 h)

1. Confirm `PORTAGE_COMPRESS`, `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES`
   (and `_FLAGS` only if set in config) reach the misc-functions env
   (S0 table); confirm `BIG.txt.bz2`, `small.txt` uncompressed (<128 B),
   `html/` skipped, `doman` compressed, `doinfo` not — exactly as the
   oracle archive shows.
2. Check `${D}` has no `.ecompress` residue and `CONTENTS` md5s match
   the compressed files on a source merge (`emerge porttest/docs`).
3. Delete `KNOWN_FINDINGS` `l2-gpkg-docompress|BIG\.txt` + yaml entry;
   re-run the porttest track.
4. Rust e2e: `tests/test_portuale.py`-style build of a `fixtures/`
   doc-bearing ebuild with a seeded `make.conf` (`PORTAGE_COMPRESS=
   bzip2`, `FEATURES=binpkg-docompress`) asserting the `.bz2` suffix in
   `${D}` — host-side, no container (needs `bzip2` on the host; skip
   with a clear reason otherwise).

### S2 — dostrip + splitdebug (M, F review, 4-8 h)

1. Confirm `estrip` fires and finds `strip`/`objcopy`/`readelf`/
   `debugedit`/`scanelf`; setuid binary sizes match the oracle (15424 →
   14384); `/usr/lib/debug/usr/bin/pt-*.debug`, `/usr/lib/debug/usr/lib64/
   libptsd.so.0.0.0.debug`, `/usr/lib/debug/.build-id/xx/yyyy.debug` +
   symlinks — same set as the oracle (`tar tf` both, diff).
2. `CONTENTS` on a source merge records the stripped md5 and the debug
   objects; `NEEDED.ELF.2` unchanged in content (field count is #39).
3. Once under `--shell brush` (G3).
4. Rescope/delete `l2-gpkg-dostrip-splitdebug*` yaml + `KNOWN_FINDINGS`
   rows; porttest track to 0 unexplained.

### S3 — `RESTRICT=strip` / `FEATURES=nostrip` fixtures (M, 2-4 h)

1. Add `porttest/restrict-strip` (compiled binary, `RESTRICT="strip"`)
   and have the L2 bed build it with both PMs; assert unstripped size
   parity and no `/usr/lib/debug`. Add a `nostrip` *setting* variant
   only if the bed can pass per-fixture `FEATURES` cheaply (else file).
2. Keep each ebuild trivial (README rule). Update `README.md` table.

### S4 — `instprep` decision (F, 1 h to file / 4-8 h to implement)

1. Container repro: `FEATURES="-binpkg-dostrip -binpkg-docompress"`,
   `emerge porttest/docs porttest/setuid` with both PMs; real strips/
   compresses at merge (`vartree.py:4440`), portuale does not.
2. Decide with the user (G2). If implementing: one `run_misc_function
   (..., "__dyn_instprep", ...)` at the treewalk position real uses
   (before the copy loop, after the `install` chain, also on binpkg
   merges), env = the same phase env; re-run L1 + L2. If filing: #38b in
   `backlog-tasks.md` + `scope-backlog.md` §K with the repro, and a note
   in the `ebuild.rs:38` stub comment.

### S5 — Closeout (M/S, 2-3 h + one container run)

1. Porttest track clean; real set (`L2_BUILD_MODE=deep`) as far as it
   goes; record the stop point; classify new findings (env → #37,
   metadata → #39, else new).
2. L1 porttest re-run (archive consumer must be unaffected).
3. Docs: `what-this-proves.md` paragraph (live command: build
   `porttest/docs`, `tar tf` shows `BIG.txt.bz2`; `porttest/splitdebug`
   `.debug` tree), `scope-backlog.md` §K bullet closed, `backlog-tasks.md:
   74` DONE (+ #38b if filed), fixture README rows.
4. Full verification pass (`cargo fmt --check`, clippy zero warnings,
   `cargo test --release`, `pytest tests -q`), then L2 porttest, L1.

---

## 5. Risks, traps, stop rules

1. **Version skew** — vendored `bin/estrip`/`ecompress` vs the image's
   portage 3.0.82.2; classify every diff against both before calling it
   a bug.
2. **Double-transform** — never wire `instprep` without real's `!
   contains_word binpkg-*` complement; a diff adding both call sites
   unconditionally is wrong on sight.
3. **Allowlist theatre** — green because a `KNOWN_FINDINGS` regex still
   matches is not green; delete first, then run.
4. **Tool absence is behaviour** — real warns and continues without
   `debugedit`; no Rust pre-check, no test skip that hides it.
5. **`PORTAGE_COMPRESS=""` disables** — must flow as empty; `_FLAGS` must
   *not* be exported when unset (the `-v` test).
6. **`QA_PRESTRIPPED`** warnings are log noise unless payload differs.
7. **Scope pull** — `NEEDED.ELF.2` fields, `SIZE`/`IUSE` members, `packdebug`
   → #39 / file. **Stop rule:** more than three systemic non-env blockers
   in S0 → stop, file, re-sequence with the user.

---

## 6. Delegation brief

State that #37 S2 has landed (commit hash); pass this file, the G1-G5
answers, the S0 table, the oracle paths, and the rules: never edit
vendored `bin/*`; never add a transform switch outside the resolved
config; derive expectations from the oracle archives; return diff,
commands, `gpkg-diff.sh` output, and the exact allowlist rows deleted. No
container → implement/unit-test only and label **unverified end-to-end**.
