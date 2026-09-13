# Plan: backlog #38 — Packaging transforms: dostrip / splitdebug / docompress (merged)

Status: **proposed, not executed. #37, its blocker, landed 2026-09-13**
— resolved `FEATURES`, `PORTAGE_COMPRESS*`, `USE`, `SLOT` are in the
phase env now (`037_Build-phase-env-completeness.plan.md`, S0–S5
complete). Written 2026-09-13 against `main` @ `ec13936`.

This is the **merge** of three independent drafts —
`038_Packaging-transforms.{deepseek,musespark,claude}.md` — keeping the
most promising approach and the safest route. §0 records where they
disagreed and what in the checkout settled it. **This file is the scope
authority for agents**; the drafts are history.

Backlog: `docs/backlog-tasks.md:74`; `scope-backlog.md` §K (`:669-672`).
Findings: `TEST/findings/l2.md` `l2-gpkg-docompress` (`:121-132`),
`l2-gpkg-dostrip-splitdebug` (`:255-260`). L3 contract: `docs/030_L3-
source-build-parity.deepseek.md` §S3 + G0.5.

**Read first:** `AGENTS.md` (steps 1, 4's real-execution carve-out, 5,
7, 8), `docs/agent-context.md`, `TEST/findings/l2.md`,
`037_Build-phase-env-completeness.plan.md` (§2 ground truth + G1-G3
answers as landed), then the code under test:
`rust/portuale/src/ebuild_phases.rs` (`run_commands_async`'s post-install
misc-functions call `:2996-3029`, `run_misc_function` `:2889`,
`phase_features_value` `:1581`), `rust/portuale/src/ebuild_merge.rs`
(`run_merge` → `run_commands(["install"])` `:2719`, the treewalk mirror),
`rust/portuale/src/ebuild_package.rs` (`run_package` `:448`,
`package_after_install` `:487`, `invoke_dyn_package` `:773`),
`rust/portuale/src/ebuild.rs:38` (the `instprep` stub note),
`TEST/run/l2-portuale-builder.sh` (`KNOWN_FINDINGS` `:57-71`),
`TEST/compare/known-divergences.yaml` (`:98-134`). Real authority (all
vendored, unmodified, already executed by portuale):
`3rdparty/portage/bin/misc-functions.sh` (`install_qa_check` `:77`;
`ecompress` gate `:147-152`; scanelf/NEEDED block `:154-236`; `estrip`
gate `:239-250`; `__dyn_instprep` `:265-308`; `__dyn_package` `:540`),
`bin/estrip` (`has_feature`/`has_restriction` `:404-430`, tool names
`:462-470`, `save_elf_debug` `:98-210`, `debugedit` warning `:176-181`),
`bin/ecompress` (`PORTAGE_COMPRESS` die `:228-229`, default flags
`:232-247`, disable note `:254`), `bin/phase-helpers.sh:21-28` (array
defaults) + `:164-220` (`docompress`/`dostrip` helpers),
`bin/save-ebuild-env.sh:20-29` (arrays dropped only on the *final* save),
`lib/portage/dbapi/vartree.py:4440-4450` (`instprep` from `treewalk`),
`cnf/make.globals:77-84,107-111`. Line numbers correct at `ec13936`;
**re-locate by symbol before editing**.

Model tiers: **F** frontier (Opus 5 / Fable 5.1), **M** mid (Sonnet 5),
**S** small (Haiku 4.5); "F review" = frontier reads the full diff before
the user is asked to commit.

---

## 0. Adjudications

| Topic | deepseek | musespark | claude | **Merged decision** | Evidence |
|---|---|---|---|---|---|
| Where the transform call sites are | `install_qa_check` (`:147-152`, `:239-250`), which portuale already runs after every `install` | `__dyn_package`, run by `invoke_dyn_package` | `install_qa_check`, already run on every source path incl. merge | **deepseek/claude**. musespark is factually wrong: the gates are in `install_qa_check()` (`:77-263`), not `__dyn_package()` (`:540`); `run_commands_async` runs `install_qa_check` unconditionally after `install` on `emerge <atom>` merges, scheduler builds, `--buildpkgonly`, and `ebuild <file>` | `grep -n binpkg-do misc-functions.sh` → `:149,:243,:282,:290`; `ebuild_phases.rs:2996-3029`; `ebuild_merge.rs:2719`; `ebuild_package.rs:455` |
| Does the merge path get transforms today? | yes, once env is right | "neither package-path nor instprep-path" | yes (default-on) once env is right | **yes for the default-on path** once #37 lands; **no for the `-binpkg-*` complement** (`__dyn_instprep`), which real runs from `treewalk` on *every* merge incl. binpkg merges | `vartree.py:4440-4450` |
| Why inert today | `FEATURES` raw env + `PORTAGE_COMPRESS` unset | same, plus "no instprep caller" | same | **#37 reasons only** for the default path: harness exports `FEATURES="buildpkg binpkg-multi-instance splitdebug xattr …"` with neither `binpkg-*` token; real gets them from `make.globals` (incremental) | `TEST/layers/l2/build-portuale.sh:28`; `make.globals:77-84`; `ecompress:228` |
| Shape of the work | recon → fix env gaps → pin → delete allowlist | three transforms as three slices, `instprep` as the merge mechanism | verify default path fires → pin against oracle → decide `instprep` | **claude's shape with musespark's per-transform slicing**: S1 docompress, S2 dostrip+splitdebug (one script, one fixture — not two slices), S3 RESTRICT/nostrip fixtures, S4 `instprep` decision | — |
| `instprep` | file #38b unless cheap (G2.2) | implement via `run_misc_function` before `merge_tree` (G0.2) | file #38b unless harness needs `-binpkg-*`; user decides | **explicit user decision in S4**, default *file #38b*: implementing adds a new phase to every merge including binpkg merges (L1 blast radius); real's complement gating must be inherited (never wire both sites unconditionally) | `misc-functions.sh:265-308` idempotent via `.instprepped` |
| Reimplement in Rust? | rejected | rejected | rejected | **rejected** — env + invocation of vendored scripts only; never edit `bin/*` to make a test pass | — |
| Determinism (deepseek G2.5) | byte-deterministic strip/objcopy, build twice | payload bytes change *by design*; grade vs real, not vs old portuale | over-scoped for #38; L3's concern | **musespark/claude**: #38 grades portuale-vs-real *same fixture, same image*; determinism claims belong to L3 | — |
| RESTRICT/FEATURES matrix | one `dostrip -x` case if the oracle proves it | full matrix (`nostrip`/`strip`/`binchecks`/`splitdebug` × tokens) | only oracle-gradable cells: default, `splitdebug`, `RESTRICT=strip`, `FEATURES=nostrip` | **claude**: a cell needs a real archive built with the same setting; `_l1-pkgcache` has defaults only → two tiny new fixtures both PMs build; the rest filed | `TEST/logs/_l1-pkgcache/porttest/` |
| Env gaps found during recon | fix through #37's builder | file into 037/S4 | fix **and test** in #37's builder, not #38's diff | **claude/musespark**: a missing whitelisted var is a #37 bug; it lands as a #37 follow-up commit | — |
| Tool absence | record presence; classify | real's own warn/skip path is the spec; no Rust pre-gating | same | **kept** — `debugedit` absent → `eqawarn` once, continue (`estrip:176-181`) | image has `dev-util/debugedit-5.3`, `app-misc/pax-utils-1.3.10` |
| `PORTAGE_COMPRESS_FLAGS` | export if config carries it | — | export only if *set* (script uses `-v` test) | **claude** — an exported empty value would suppress the per-compressor defaults | `ecompress:232` |
| Tiering | F recon+instprep, M rest | M throughout, F review (b)/(c) | F recon + instprep decision, M rest | **F: S0, S4. M (F review): S2. M: S1, S3, S5** | — |

---

## 1. Opinion

### 1.1 Verdict — mostly #37's tail; do not write a stripper

Three verified facts decide the shape:

1. **Portuale already runs the real code containing the transforms.**
   `install_qa_check` (`misc-functions.sh:77`) holds the `ecompress` gate
   (`:149`) and the `estrip` gate (`:243`); `run_commands_async` runs
   `install_qa_check install_symlink_html_docs install_hooks` after every
   successful `install` phase (`ebuild_phases.rs:2996-3029`), with the
   same `extra_env`, on every source path.
2. **The gates are false today only because of #37.** `FEATURES` in the
   phase env is the raw process env (no `binpkg-dostrip`/`binpkg-
   docompress` — real gets both from `make.globals` because `FEATURES`
   is incremental), and `PORTAGE_COMPRESS="bzip2"` (`make.globals:108`)
   is never exported (whitelisted at `ebuild_phases.rs:1469`, never set)
   so `ecompress` would `die` (`:228-229`). #37's builder delivers both
   (`FEATURES` via `resolved_incremental`, `PORTAGE_COMPRESS*` via
   `other_vars` ∩ real's `environ_whitelist`).
3. **The `-binpkg-*` complement is a separate merge-time phase.** Real
   runs `instprep` from `dblink.treewalk()` (`vartree.py:4440-4450`) on
   every merge, source or binary; `__dyn_instprep` (`:265-308`) applies
   the transforms iff the token is *absent*, is idempotent
   (`.instprepped`), and is otherwise a near no-op (`chflags` handling).
   Portuale's treewalk mirror never runs it (`ebuild.rs:38`).

So **#38 = verify the default-on path fires once #37 lands → pin the
outcome with the existing fixtures against the real oracle archives →
add two tiny oracle-graded fixtures for `RESTRICT=strip`/`nostrip` →
decide `instprep` explicitly → delete the temporary allowlists.**
Expected Rust delta for the default path: near zero (perhaps an env pair
the recon proves missing — which is a #37 fix). The risk is not
implementation; it is (a) misclassifying integration failures (tool
missing, version skew between vendored `bin/` and the image's portage
3.0.82.2) as transform bugs, (b) double-transform if `instprep` is wired
without real's complement, (c) declaring victory on an allowlist match
instead of the oracle.

### 1.2 Model tiers and split

| Slice | Content | Tier | Why |
|---|---|---|---|
| S0 | recon under #37's env: build fixtures, read logs, tool table, classify every non-firing branch, real `instprep` call order | **F** | unknown-unknowns; classification quality decides everything after |
| S1 | docompress: close `l2-gpkg-docompress` against the oracle | M | one script, one fixture; bytes match or don't |
| S2 | dostrip + splitdebug: close `l2-gpkg-dostrip-splitdebug*` | M (F review) | `estrip` is dense; tool-absence semantics; `.build-id` layout; `CONTENTS` interaction |
| S3 | `RESTRICT=strip` / `FEATURES=nostrip` fixtures | M | two tiny fixtures both PMs build; oracle-graded |
| S4 | `instprep` decision (file #38b or implement) | **F** | new merge-path phase incl. binpkg merges; user call |
| S5 | closeout: allowlist deletion, real-set re-run, L1 re-run, docs | M/S | mechanical + container runs |

If only a cheap model is available: S1, S2, S3, S5 after an F-written S0
table with expected path sets frozen. S0 and S4 stay frontier.

### 1.3 Difficulty

| Axis | 1-5 | Notes |
|---|---|---|
| Rust code | 1-2 | near zero for the default path; one misc-function call if S4 implements |
| Bash comprehension | 3 | `estrip` ~700 lines, `ecompress` ~260; read to *classify*, not to port |
| Integration/triage | 4 | tool presence, version skew, env provenance, `QA_PRESTRIPPED` noise |
| Test design | 3 | oracle-driven; new fixtures need both PMs to build them |
| **Overall** | **3** | low volume, high observation cost; hinges entirely on #37 |

Effort: **~14-26 agent-hours, 3-4 sittings** after #37 S2; **+4-8 h** if
S4 implements `instprep`.

### 1.4 Order

**#37 → #38.** #38 S0 may start as soon as #37 S2 is on a branch
(resolved `FEATURES` visible in `metadata/FEATURES`); S1-S3 wait for it
to land. Inside #38: S0 → S1 → S2 → S3 → S4 → S5 (ascending tool
dependence; each later slice's expectations assume the earlier
transform's bytes).

---

## 2. Ground truth (verified at `ec13936`)

- **Gates** (`misc-functions.sh`): `:149` `[[ ${PORTAGE_COMPRESS} ]] &&
  contains_word binpkg-docompress "${FEATURES}"` → `ecompress --queue
  "${PORTAGE_DOCOMPRESS[@]}"`, `--ignore "${PORTAGE_DOCOMPRESS_SKIP[@]}"`,
  `--dequeue`; `:243` `contains_word binpkg-dostrip "${FEATURES}"` →
  `estrip --queue "${PORTAGE_DOSTRIP[@]}"`, `--ignore`, `--dequeue`
  (`___eapi_has_dostrip`, EAPI 7+; else `--prepallstrip`). Complements
  `:282`/`:290` in `__dyn_instprep`. Default `FEATURES`
  (`make.globals:77-84`) has both `binpkg-*` on; `splitdebug`,
  `compressdebug`, `installsources` off (the L2 harness and L3 block turn
  `splitdebug` on).
- **Arrays**: `PORTAGE_DOCOMPRESS=(/usr/share/{doc,info,man})`,
  `PORTAGE_DOCOMPRESS_SKIP=(/usr/share/doc/${PF}/html)`,
  `PORTAGE_DOCOMPRESS_SIZE_LIMIT=128`, `PORTAGE_DOSTRIP=(/)` for EAPI 7+
  (`phase-helpers.sh:21-28`); ebuild helpers `docompress [-x]`/`dostrip
  [-x]` extend them (`:164-220`). They survive from `src_install` into
  `install_qa_check` through `$T/environment` — only the final
  `--exclude-init-phases` save drops them (`save-ebuild-env.sh:20-29`).
  **Nothing for Rust to do.**
- **`estrip`** (`:404-430`): `has_feature[compressdebug dedupdebug
  installsources nostrip splitdebug xattr]` from `FEATURES`;
  `has_restriction[binchecks dedupdebug installsources splitdebug strip]`
  from `PORTAGE_RESTRICT`; `RESTRICT=strip` or `FEATURES=nostrip` →
  banner off, skip (unless `installsources`). Tools: `debugedit`, `dwz`,
  `${CHOST}-`{`objcopy`,`ranlib`,`readelf`,`strip`} (`:462-470`), `scanelf`
  (`:393`). `debugedit` absent → `eqawarn` once, continue without
  build-ids (`:176-181`). Reads `PORTAGE_STRIP_FLAGS` (optional),
  `STRIP_MASK` (exported by `misc-functions.sh:244`), `KERNEL`, `CHOST`,
  `ED`/`D`/`T`, `SLOT`.
- **`ecompress`**: dies without `PORTAGE_COMPRESS` (`:228-229`); per-
  compressor default flags when `PORTAGE_COMPRESS_FLAGS` is *unset*
  (`-v` test, `:232-247`); skips pre-compressed suffixes (`:87`); needs
  `find0`/`___parallel` (`isolated-functions.sh`) and the compressor on
  `PATH`. `PORTAGE_COMPRESS=""` is the documented disable (`:254`).
- **Ordering inside `install_qa_check`**: scanelf NEEDED writer
  (`:154-236`) runs *before* `estrip` (`:239`) — bug 749624; strip does
  not change `DT_NEEDED`. Portuale's own `NEEDED.ELF.2` writer
  (`needed_elf.rs`) is #39's business.
- **Portuale env today**: `PORTAGE_RESTRICT` is USE-reduced with an
  *empty* USE for non-`depend` phases (`ebuild_phases.rs:2079-2086`); a
  `RESTRICT="!x? ( strip )"` real package would diverge — after #37's
  `USE` lands this should use the effective set (file against #37 if
  seen; never patch `estrip`).
- **Image tools** (`TEST/logs/l1-20260913T013910Z/portage.installed-
  before.txt`): `dev-util/debugedit-5.3`, `app-misc/pax-utils-1.3.10`
  (scanelf), binutils, `bzip2`. **Real oracle archives**:
  `TEST/logs/_l1-pkgcache/porttest/{docs,splitdebug,setuid}/*.gpkg.tar`.
- **Fixtures**: `TEST/images/overlay/porttest/porttest/{docs,splitdebug,
  setuid}`; `README.md:32-36` documents the expected split (`dodoc -r`
  → compressed, `newdoc`, `doman` compressed, `doinfo` not, `docinto html`
  not; `splitdebug` → `.debug` + `.build-id` for a binary AND a soname
  lib). Size witness: setuid binary 15424 (portuale) vs 14384 (real).
  `KNOWN_FINDINGS` regexes `TEST/run/l2-portuale-builder.sh:65-71`; yaml
  `known-divergences.yaml:98-134` (`l2-gpkg-dostrip-splitdebug{,-contents,
  -libptsd,-dirs}`).
- **Version skew**: portuale runs *this repo's* `bin/estrip`/`ecompress`/
  `misc-functions.sh`; the reference archives come from the image's
  installed portage 3.0.82.2.

---

## 3. Scope

**In:** default-on `binpkg-dostrip` + `binpkg-docompress` firing on all
source paths via the existing `install_qa_check` call; `FEATURES=
splitdebug` output parity (`/usr/lib/debug/**`, `.build-id/**`);
`RESTRICT=strip` and `FEATURES=nostrip` pinned by two tiny oracle-graded
fixtures; `CONTENTS` recording post-transform paths/md5s on a source
merge; a brush-backend smoke run; an explicit `instprep` decision;
deleting the temporary allowlist entries.

**Out (file, don't absorb):** `packdebug` (`__generate_packdebug`);
`installsources`/`dedupdebug`/`compressdebug` beyond non-regression;
xattr/selinux/chflags branches; `NEEDED.ELF.2` field count and gpkg
metadata members (#39); reimplementing `estrip`/`ecompress` in Rust;
editing vendored `bin/*`; deterministic-compression claims (L3);
resolver/merge/scheduler/compare-stack changes; Python mirror / contract
`CASES` (real-execution-only item).

---

## 4. Gates

- **G1 Trigger source is the resolved config only.** No Rust-side
  switch, no hardcoded `PORTAGE_COMPRESS=bzip2`, no `cfg(test)` shortcut.
  If a branch doesn't fire, the bug is in #37's builder — fix and test it
  there. Owner: agent.
- **G2 `instprep`.** Default: **file #38b** in S4 with a container repro
  and a `scope-backlog.md` §K entry, unless S0 shows the L2/L3 harness
  configures `-binpkg-*` (it does not today). If the user chooses to
  implement: one `run_misc_function(..., "__dyn_instprep", ...)` at the
  treewalk position real uses (after the `install` chain, before the copy
  loop, **also on binpkg merges**), same phase env, relying on real's own
  complement conditions; L1 + L2 re-run mandatory. Owner: **user**; S0
  presents the call-order table.
- **G3 Shell backend.** Prove on `bash` (default); run `porttest/
  splitdebug` once under `--shell brush`; if the external `estrip`/
  `ecompress` subprocesses misbehave under the brush parent, **file**
  (brush-pin workflow), don't drop brush. Owner: agent.
- **G4 Expected outputs come from the real oracle archives** (`tar tf` +
  `gpkg-diff.sh --mode strict`), never from the fixture README (a claim,
  not a proof). Owner: agent.
- **G5 New fixtures** are built by both PMs on the L2 bed, isolate one
  cell each, keep `src_install` trivial (README rule), and are checked
  for name collisions under `fixtures/repo/` and the overlay. Owner:
  agent.
- **G6 Env gaps discovered here are #37 bugs.** They land as #37 follow-up
  commits (builder + unit test), and #38 waits. Owner: agent.

**Evidence bar:** `gpkg-diff.sh --mode strict` shows no `BIG.txt`,
`usr/lib/debug`, `libptsd`, `usr/bin/pt-` rows for `docs`/`splitdebug`/
`setuid` **with the corresponding `KNOWN_FINDINGS`/yaml entries deleted
first**; a source merge's `CONTENTS` matches the oracle VDB. A green run
that still matches an allowlist row is not acceptance.

---

## 5. Slices

### S0 — Recon under #37's env (F, 3-5 h, no product code)

Precondition: #37 S2 on a branch or landed; `metadata/FEATURES` of a
portuale-built porttest archive contains `binpkg-docompress
binpkg-dostrip`.

1. Build `porttest/{docs,splitdebug,setuid}` with `--buildpkgonly` and
   with `-b` in the L2 container (`TEST/run/l2-portuale-builder.sh
   TEST/atomlists/l1-porttest.txt`); keep the build logs.
2. Per fixture: did `install_qa_check` reach `ecompress`/`estrip` (grep
   the log for the compress banner / `strip:` lines / `eqawarn`s)? Which
   tools did `estrip` resolve (`name_of`)? Diff the archive image path set
   and sizes against the oracle (`tar tf`, `gpkg-diff.sh`).
3. Write the table into `TEST/findings/l2.md` (new `## #38 recon`
   section; split to `l2-transforms.md` if it grows): fixture × branch →
   fired? / output equal? / cause class `{portuale-env, container-tool,
   version-skew, real-divergence, fixed}`; tool presence table; the real
   `instprep` call order with citations (§2) for G2; expected path sets
   frozen from the oracle.
4. If any `portuale-env` row exists: file it against #37 (G6) and stop
   #38 until it lands. If more than three systemic non-env blockers
   appear: stop, file, re-sequence with the user.

**Acceptance:** every non-firing branch has a named cause and a repro
command; expected trees frozen; G2 data in hand; no code changed.

### S1 — docompress (M, 2-4 h)

1. Confirm `PORTAGE_COMPRESS`, `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES` (and
   `_FLAGS` only if set in config) reach the misc-functions env (S0
   table); confirm `BIG.txt.bz2`, `small.txt` uncompressed (<128 B),
   `html/` skipped, `doman` compressed, `doinfo` not — exactly as the
   oracle archive shows (G4).
2. On a source merge (`emerge porttest/docs`): `${D}` has no `.ecompress`
   residue; `CONTENTS` md5s match the compressed files; symlinks into
   compressed docs are repaired (`ecompress` relink step).
3. Delete `KNOWN_FINDINGS` `l2-gpkg-docompress|BIG\.txt` and the yaml
   entry; re-run the porttest track.
4. Rust e2e (`tests/test_portuale.py` pattern): build a doc-bearing
   `fixtures/` ebuild with seeded `make.conf` (`PORTAGE_COMPRESS=bzip2`,
   `FEATURES=binpkg-docompress`) and assert the `.bz2` suffix in `${D}`;
   skip with an explicit reason if `bzip2` is absent on the host.

**Acceptance:** `l2-gpkg-docompress` closed with before/after evidence in
`l2.md`; full suite green.

### S2 — dostrip + splitdebug (M, F review, 4-8 h)

1. Confirm `estrip` fires and resolves `strip`/`objcopy`/`readelf`/
   `debugedit`/`scanelf`; setuid binary sizes match the oracle
   (15424 → 14384); `/usr/lib/debug/usr/bin/pt-*.debug`,
   `/usr/lib/debug/usr/lib64/libptsd.so.0.0.0.debug`,
   `/usr/lib/debug/.build-id/xx/yyyy.debug` + symlinks — same set as the
   oracle (`tar tf` both, `diff`).
2. Source merge: `CONTENTS` records the stripped md5 and the debug
   objects; `NEEDED.ELF.2` content unchanged (field count is #39 — do not
   touch its allowlist rows).
3. Once under `--shell brush` (G3); file if broken.
4. Rescope/delete `l2-gpkg-dostrip-splitdebug*` yaml + `KNOWN_FINDINGS`
   rows; porttest track to 0 unexplained.
5. **Stop rule:** toolchain-class failure (`strip` flags, `QA_PRESTRIPPED`
   handling, musl quirks) → file with repro, escalate; never patch the
   vendored script.

**Acceptance:** `l2-gpkg-dostrip-splitdebug` closed with evidence; F
review of the classification, not just the diff.

### S3 — `RESTRICT=strip` / `FEATURES=nostrip` fixtures (M, 2-4 h)

1. Add `porttest/restrict-strip` (compiled binary, `RESTRICT="strip"`);
   both PMs build it on the L2 bed; assert unstripped-size parity and no
   `/usr/lib/debug`. Add a `nostrip` setting variant only if the bed can
   pass per-fixture `FEATURES` cheaply; otherwise file it.
2. Trap: `-binpkg-dostrip` does **not** mean "no strip" — ebuild-called
   `dostrip`/`prepstrip` still run (`misc-functions.sh:241` note). Do not
   write a fixture asserting "no strip" for that case.
3. Update the overlay `README.md` table; check name collisions (G5).

**Acceptance:** two oracle-graded cells pinned; no other matrix claims.

### S4 — `instprep` decision (F, 1 h to file / 4-8 h to implement)

1. Container repro: `FEATURES="-binpkg-dostrip -binpkg-docompress"`,
   `emerge porttest/docs porttest/setuid` under both PMs; real strips/
   compresses at merge (`vartree.py:4440`), portuale does not. Same with a
   binpkg merge of an unstripped archive.
2. Decide with the user (G2). **File:** #38b in `backlog-tasks.md` +
   `scope-backlog.md` §K with the repro and a note in the `ebuild.rs:38`
   stub comment. **Implement:** one `run_misc_function` at real's treewalk
   position on every merge (source and binary), env = phase env; re-run
   L1 (with portage upgrade) and L2; fixture: the repro above goes green.

**Exit:** implemented-and-fixtured, or #38b filed with repro. No third
state.

### S5 — Closeout (M/S, 2-3 h + container runs)

1. Porttest track clean from a fresh run; real set (`L2_REBUILD=1
   L2_MODE=payload-tolerant L2_BUILD_MODE=deep … atomlists/l1-merge.txt`)
   as far as it goes; record the stop point; classify new findings (env →
   #37, metadata → #39, else new).
2. L1 porttest re-run (archive consumer must be unaffected); if S4
   implemented `instprep`, L1 is mandatory and any diff is a finding.
3. Docs: `what-this-proves.md` one appended paragraph (live command:
   build `porttest/docs`, `tar tf` shows `BIG.txt.bz2`; `porttest/
   splitdebug` `.debug` tree), `scope-backlog.md` §K bullet closed,
   `backlog-tasks.md:74` DONE (+ #38b if filed), fixture README rows,
   `docs/030` G0.5 pointer.
4. Full verification pass (`cargo fmt --check`, clippy zero warnings,
   `cargo test --release`, `python3 -m pytest tests -q`), then L2
   porttest, L1.

---

## 6. Fixtures, oracles, tests

| What | Where | Notes |
|---|---|---|
| `porttest/docs` | `TEST/images/overlay/porttest/porttest/docs/` | `BIG.txt` (>128 B) vs `small.txt`, `html/`, man, info |
| `porttest/splitdebug` | same | binary + soname lib → `.debug` + `.build-id` |
| `porttest/setuid` | same | stripped-size witness |
| `porttest/restrict-strip` (new, S3) | same | `RESTRICT=strip` cell |
| Real oracle | `TEST/logs/_l1-pkgcache/porttest/{docs,splitdebug,setuid}/*.gpkg.tar` | **the** expected path set / sizes |
| Comparison | `TEST/compare/gpkg-diff.sh --mode strict`, `gpkg-structure.sh`, `diff.py --layer l2` | archive-vs-archive, root/VDB |
| Runner | `TEST/run/l2-portuale-builder.sh` + `KNOWN_FINDINGS` | delete rows as they pass |
| Rust unit/e2e | `rust/portuale/src/{ebuild_phases,ebuild_merge}.rs`, `tests/test_portuale.py` | env wiring, `.bz2` in `${D}`; no Python mirror |
| Python contract | `tests/` | untouched, green |

**Verification pass (step 8):** `cargo fmt --check`, `cargo clippy
--release --all-targets`, `cargo test --release`, `python3 -m pytest
tests -q`, then L2 porttest; **also L1** (`TEST/run/l1-merge-from-
binpkg.sh`, with the portage upgrade) because the packaging/merge path
is touched.

---

## 7. Risks, traps, stop rules

1. **Version skew** — vendored `bin/` vs image portage 3.0.82.2; classify
   every diff against both before calling it a bug.
2. **Double-transform** — never wire `instprep` without real's `!
   contains_word binpkg-*` complement; a diff adding both call sites
   unconditionally is wrong on sight.
3. **Allowlist theatre** — delete the `KNOWN_FINDINGS`/yaml rows *first*,
   then run; grade portuale-vs-real, never portuale-vs-old-portuale.
4. **Tool absence is behaviour** — real warns and continues without
   `debugedit`; no Rust pre-check, no test skip that hides it.
5. **`PORTAGE_COMPRESS=""` disables** — must flow as empty; `_FLAGS` must
   *not* be exported when unset.
6. **`QA_PRESTRIPPED`/`eqawarn`** — log noise unless payload differs;
   compare payload first.
7. **Payload bytes change by design** — expect `KNOWN_FINDINGS`/yaml churn
   in the same commit; reviewers check it against real.
8. **Scope pull** — `NEEDED.ELF.2` fields, `SIZE`/`IUSE` members,
   `packdebug`, `installsources` → #39 / file.
9. **#37 slippage** — S0 may run on a branch; S1-S3 never stub the env
   "to make progress".
10. **Stop rules:** >3 systemic non-env blockers in S0 → stop, file,
    re-sequence; toolchain-class `estrip` failure in S2 → file, escalate.

## 8. Review checklist (attach to each slice)

- One transform family per slice? (No S1+S2 blob.)
- No transform switch outside the resolved config? No hardcoded
  `PORTAGE_COMPRESS`?
- Complement gating mirrored exactly if `instprep` is wired?
- Expectations derived from the oracle archive, cited by path?
- Allowlist rows deleted *before* the green run?
- Tool-absence path matches the script, no Rust pre-gating?
- `l2.md` evidence appended with exact commands?
- Rust unit/e2e added; no vendored `bin/*` edited; no Python mirror?

## 9. Definition of done

- [ ] S0 table + tool table + `instprep` call order cited; expected trees
      frozen from the oracle.
- [ ] `l2-gpkg-docompress` closed (S1); `l2-gpkg-dostrip-splitdebug*`
      closed (S2) — each with before/after evidence.
- [ ] `RESTRICT=strip` cell pinned by a both-PM fixture (S3).
- [ ] `instprep`: implemented+fixtured **or** #38b filed with repro (S4).
- [ ] No transform reimplemented in Rust; no vendored `bin/*` edited.
- [ ] L2 porttest 0 unexplained; L1 unchanged; full verification pass
      green.
- [ ] Docs updated (S5); no dead allowlist entries.

## 10. Delegation brief (for subagents)

State that #37 S2 has landed (commit hash); pass this file as scope
authority, the G1-G6 answers, the S0 table, the oracle paths (§6), and
the rules: never edit vendored `bin/*`; never add a transform switch
outside the resolved config; derive expectations from the oracle
archives; label claims provisional until the container confirms them.
Return: diff, exact commands, `gpkg-diff.sh` output, archive listings,
and the exact allowlist rows deleted. No container → implement/unit-test
only and label **unverified end-to-end**.

## 11. Findings filed while executing

(append here as S0-S5 run: command, expected, actual, root cause, fix
ref / backlog id)
