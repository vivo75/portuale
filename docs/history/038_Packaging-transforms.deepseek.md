# #38 — Packaging transforms: dostrip/splitdebug/docompress — agent plan (deepseek draft)

> **Historical planning/investigation doc, retired 2026-09-15.** Its outcome is `docs/backlog-tasks.md`'s status line for this item; the extracted ground truth, gotchas and dead ends are in [`../recap-of-backlog-ops-2026-09-15.md`](../recap-of-backlog-ops-2026-09-15.md). Kept verbatim below for citation/provenance only.


Status: **plan only, not executed. Blocked on #37.** Written 2026-09-13
against `main` @ `ec13936`. Backlog: `docs/backlog-tasks.md:74`,
`scope-backlog.md` §K (`:669-673`). Findings:
`TEST/findings/l2.md` §S3 `l2-gpkg-dostrip-splitdebug` /
`l2-gpkg-docompress` and the S5 table. L3 contract:
`docs/030_L3-source-build-parity.deepseek.md` §S3.2 + G0.5 (`:503-528`).
Companion plan: `docs/037_Build-phase-env-completeness.deepseek.md`
(this item's enabling dependency).

**Read first:** `AGENTS.md`, `docs/agent-context.md`, `TEST/findings/l2.md`
(S3/S5), `docs/037_Build-phase-env-completeness.deepseek.md` (the
`FEATURES`/`PORTAGE_COMPRESS` thread-through contract),
`docs/029_portuale-as-builder.deepseek.md` §1.1/§S3, and the module doc
comments of `rust/portuale/src/{ebuild_phases.rs,ebuild_package.rs}`.
Real authority (all in this checkout): `bin/misc-functions.sh`
(`install_qa_check` — docompress `:147-152`, scanelf NEEDED
`:154-236`, dostrip `:239-250`), `__dyn_instprep` (`:280-298`),
`bin/phase-functions.sh` (`__dyn_install` `:642-775`, QA_PRESTRIPPED
`:656-680`, build-info REPO_REVISIONS `:769`), `bin/estrip`
(has_feature/has_restriction `:404-421`, `save_elf_debug`
`:96-210`, prepstrip `:495-710`), `bin/ecompress` (`:56-130`
queue/size-limit, `:200-300` setup/compress/relink),
`bin/phase-helpers.sh` (`docompress`/`dostrip` `:164-210`,
`PORTAGE_DOCOMPRESS*` arrays `:21-23`), `cnf/make.globals`
(default FEATURES `:77-84`, `PORTAGE_COMPRESS*` `:107-111`).

Model tiers (same convention as 022-037):

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

**Do #38 after #37, and do not rewrite `estrip`/`ecompress` in Rust —
turn on the real ones and fix the integration.** The critical fact
(verified below, `ebuild_phases.rs:2996-3029`): portuale **already
runs the real, unmodified `bin/misc-functions.sh
install_qa_check install_symlink_html_docs install_hooks`** right after
every successful `install` phase, on every source path including
`--buildpkgonly` (via `run_package` → `run_commands(["install"])`).
`install_qa_check` is exactly where real portage performs
`dostrip`/`estrip` and `ecompress` (`misc-functions.sh:147-152`,
`:239-250`). The transforms are inert today for two reasons, both from
#37:

1. `FEATURES` in the phase env is the raw process env
   (`ebuild_phases.rs:1581` via `phase_features_value`), so
   `contains_word binpkg-dostrip "${FEATURES}"`/`binpkg-docompress`
   see the harness's stripped-down list, not the resolved config list
   (`make.globals:77-84` ships both **on** by default);
2. `PORTAGE_COMPRESS` (and its flags/suffix/exclude vars) only exist if
   something exports them; they are `make.globals` scalars that #37's
   resolved environ will carry. Without it `bin/ecompress` dies
   (`ecompress:228-230`).

So the slice's shape is: **recon under resolved FEATURES → fix what the
real scripts need but the phase env still lacks → pin the resulting
payload with the fixtures and the real archive oracle → delete the
temporary allowlists.** The risk is not "write a stripper"; it is
misclassifying integration failures (missing tool, wrong path, wrong
ordering, version skew between portuale's vendored `bin/` and the
container's installed portage) as transform bugs, or allowlisting a real
divergence.

### 0.2 Frontier or cheap model?

**Mid-tier can execute most of it after #37 and after the recon is
written down; frontier does the recon and the triage.** `estrip` is a
~700-line bash program full of QA heuristics (pre-stripped detection,
hardlink dedup, splitdebug, `STRIP_MASK`, `QA_PRESTRIPPED`, elfutils vs
GNU strip flags); the plan is *not* to port it, so the hard part is
observing its interaction with portuale's phase env. Classification
("is this a portuale env bug, a real-portage version difference, or a
missing container tool?") is frontier work.

| Slice | Content | Tier | Why |
|---|---|---|---|
| A | recon under resolved FEATURES; write findings | **F** | unknown-unknowns; decides G2.2/G2.3 |
| B | strip/estrip integration + splitdebug fixture | M (F review) | real script already runs; fix env gaps |
| C | compress/ecompress integration + docs fixture | M (F review) | same |
| D | merge path + CONTENTS interaction | M | narrow, verifiable |
| E | `-binpkg-*`/instprep decision (implement or file) | **F** | semantics call |
| F | verification, allowlist cleanup, docs | M/S | mechanical |

If only a cheap model is available, it can run B/C/D/F only *after* A
has been answered and the expected path sets are fixed from the real
archive oracle (§5). A and E are not delegable below frontier.

### 0.3 Difficulty, by axis

| Axis | 1-5 | Notes |
|---|---|---|
| Rust plumbing | 2 | #37 already threads the env; this adds triggers/flags |
| Bash-tool integration | 4 | real `estrip`/`ecompress`, tool availability, queue/skip semantics |
| Determinism | 3-4 | strip/splitdebug must be byte-stable given `SOURCE_DATE_EPOCH` |
| Evidence/triage | 4 | real-vs-vendored portage version, fixture vs real oracle |
| Environment | 3 | container runs minutes; porttest track fast |
| **Overall** | **3.5** | low code volume, high observation cost |

---

## 1. Ground truth

### 1.1 What already happens in portuale

- `ebuild_phases::run_commands_async` runs the install chain and then,
  for `phase == "install"`, calls
  `run_misc_functions(..., "install_qa_check install_symlink_html_docs install_hooks", ...)`
  (`ebuild_phases.rs:2996-3029`) using the same `extra_env` the phase
  used. This is real `_post_phase_cmds["install"]`
  (`_emerge/EbuildPhase.py:424,442-461`), unconditional, not
  `FEATURES`-gated at the call site.
- All source paths reach it: `emerge <atom>` → `ebuild_merge::run_merge`
  → `run_commands(["install"])` (`ebuild_merge.rs:2719-2729`);
  `--buildpkgonly` → `ebuild_package::run_package` → same
  (`ebuild_package.rs:455-465`); then the packaging tail runs
  `__dyn_package` (`package_after_install`, `:487+`).
- `bin/misc-functions.sh` sources `ebuild.sh` (`:16`), which sources
  `phase-helpers.sh` (`bin/ebuild.sh:80`) — so `PORTAGE_DOCOMPRESS`
  (`/usr/share/{doc,info,man}`), `PORTAGE_DOCOMPRESS_SKIP`
  (`/usr/share/doc/${PF}/html`) and `PORTAGE_DOCOMPRESS_SIZE_LIMIT=128`
  (`phase-helpers.sh:21-23`) are initialized inside the misc shell, and
  ebuilds' `docompress`/`dostrip` helpers (`:164-210`) can extend them.

### 1.2 Why the transforms are inert (the #37 dependency)

`bin/misc-functions.sh:147-152` / `:239-250`:

```
if [[ ${PORTAGE_COMPRESS} ]] && contains_word binpkg-docompress "${FEATURES}"; then
    "${PORTAGE_BIN_PATH}"/ecompress --queue "${PORTAGE_DOCOMPRESS[@]}"
    ... --dequeue
fi
...
if contains_word binpkg-dostrip "${FEATURES}"; then
    if ___eapi_has_dostrip; then
        "${PORTAGE_BIN_PATH}"/estrip --queue "${PORTAGE_DOSTRIP[@]}"
        ... --dequeue
    else
        "${PORTAGE_BIN_PATH}"/estrip --prepallstrip
    fi
fi
```

- default `FEATURES` (`cnf/make.globals:77-84`) contains
  `binpkg-docompress binpkg-dostrip` (both on); `splitdebug`,
  `compressdebug`, `installsources` are **off** by default and are
  enabled explicitly by the L2 harness and L3's determinism block
  (`docs/030` §1.4 trap 4 keeps `splitdebug` on);
- `PORTAGE_COMPRESS="bzip2"` and
  `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES="css gif htm[l]? jp[e]?g js pdf png"`
  (`cnf/make.globals:107-111`) are config scalars that only #37's
  resolved environ will deliver;
- `estrip` additionally reads `PORTAGE_STRIP_FLAGS` (else derives
  elfutils/GNU flags, `estrip:495-511`), `STRIP_MASK` (exported by
  misc-functions `:244`), `ARCH`/`KERNEL` (config scalars), `SLOT`,
  `QA_PRESTRIPPED[_<arch>]`, `QA_STRICT_PRESTRIPPED`, `EPREFIX`,
  `ED`/`D`/`T`/`PORTAGE_BUILDDIR` (already in `phase_env_vars`);
- `ecompress` needs `PORTAGE_COMPRESS`, optional
  `PORTAGE_COMPRESS_FLAGS` (else per-compressor defaults
  `:232-247`), `PORTAGE_BIN_PATH`, `ED`/`T`, `___parallel`
  (`bin/isolated-functions.sh`), `find0`, `bzip2` on `PATH`.

The observed symptom (S3): real ships `BIG.txt.bz2` and
`.debug`+`.build-id` objects; portuale ships `BIG.txt`, unstripped
binaries, no `/usr/lib/debug`.

### 1.3 The `-binpkg-*`/`instprep` branch (documented cut today)

Real's split depends on the features: with `binpkg-dostrip`/
`binpkg-docompress` **on** (default) the transforms happen at
`install_qa_check` time, i.e. before packaging **and before merge**;
with either **off**, `__dyn_instprep` (`misc-functions.sh:264-303`)
does the same at merge time. Portuale does not run `instprep` at all
(`ebuild.rs:38` stub; `ebuild_options.rs:90` only recognizes the CLI
token), so the `-binpkg-*` configuration is unmodelled. That is the
deliberate boundary of this slice (G2.2).

### 1.4 Ordering with NEEDED.ELF.2 and CONTENTS

- Inside `install_qa_check` the scanelf NEEDED/NEEDED.ELF.2 block
  (`misc-functions.sh:154-236`) runs **before** `estrip`
  (`:239-250`) — real `bug #749624`: strip after capture. Portuale's
  transforms will therefore also run after the vendored writer, and
  stripping does not change `DT_NEEDED`, so preserve-libs data stays
  valid.
- `NEEDED.ELF.2`'s field set (vendored 5-field vs installed real's
  6-field) is **#39** (`l2-needed-elf2-format`) — do not fix it here.
- The payload the merge records in `CONTENTS` is `${D}` **after**
  `install_qa_check`, so compressed/split paths are what must be
  recorded. Verify the `CONTENTS` md5s match the transformed files.

---

## 2. Scope

**In scope:**

1. Default-on path: `binpkg-dostrip` + `binpkg-docompress` from the
   resolved `FEATURES` (delivered by #37) drive real `estrip`/
   `ecompress` through the existing `install_qa_check` call.
2. `splitdebug` (explicitly on in the L2 harness and L3 block):
   `/usr/lib/debug/**` objects + `.build-id` links, `objcopy`/`strip`/
   `readelf`; `debugedit` absent falls back with a warning
   (`estrip:160-182`) — acceptable if the fixture stays parity-clean.
3. `docompress -x` / `dostrip [-x]` helpers: the queue/ignore arrays
   already work via the real helpers; pin that they survive
   `environment.bz2` save/restore and that `PORTAGE_DOCOMPRESS_SKIP`
   semantics match.
4. Any phase-env variable the recon (slice A) proves missing
   (`PORTAGE_COMPRESS*`, `PORTAGE_STRIP_FLAGS`, `STRIP_MASK`, `ARCH`,
   `KERNEL`, …): add through #37's resolved-environ builder, never as
   one-off hardcodes.

**Out of scope (file, do not silently skip):**

- `-binpkg-dostrip`/`-binpkg-docompress` → `instprep`-time transforms
  (G2.2). If a user configures them, portuale must not silently skip
  stripping — at minimum document the cut; prefer implementing
  `__dyn_instprep`'s shell call at merge time if the recon shows it is
  cheap.
- `compressdebug`/`installsources`/`dedupdebug` beyond what
  `FEATURES=splitdebug` already exercises: run once with the fixture,
  and if a divergence appears, file it with a repro rather than port
  `send_elf_debug`/source installation.
- gpkg metadata completeness (`SIZE`, `IUSE*`, `NEEDED.ELF.2`
  ELF-class, …) — **#39**.
- xattr/selinux/chflags branches of `estrip`/`misc-functions`.
- Reimplementing `estrip`/`ecompress` in Rust — explicitly rejected.

---

## 3. Gates (decide before writing code)

- **G2.1 — Trigger source.** The only switch is the resolved `FEATURES`
  (#37, G1.4) plus resolved `PORTAGE_COMPRESS*`. No `cfg`-style Rust
  switch, no `#[cfg(test)]` shortcut: if the transforms don't fire, the
  bug is in the env threading.
- **G2.2 — `instprep` cut.** Recommendation: land the default-on path,
  and file `-binpkg-*`/`instprep` as **#38b** with a repro unless slice
  A shows real's `instprep` is already being run by the existing merge
  path (it is not, per §1.3). The user must see this boundary in the
  backlog, not in a commit message.
- **G2.3 — Shell backend.** The misc-functions call uses `options.shell`
  (default `bash`; `--shell brush` is opt-in). Validate on `bash`
  first; then run the splitdebug fixture once with `--shell brush` to
  prove the external `estrip`/`ecompress` subprocesses still run under
  the brush parent. If brush breaks the transforms, file it
  (brush-pin.md workflow) — do not drop brush support.
- **G2.4 — Expected path sets.** Derive the expected archive payload
  from the **real Portage-built oracle** (`TEST/logs/_l1-pkgcache/
  porttest/docs`, `.../splitdebug`), never from guessing `ecompress`'s
  skip list. The fixture comments in the ebuild are not authoritative.
- **G2.5 — Determinism.** `strip`/`objcopy` must be byte-deterministic
  for the same input. Confirm by building the same fixture twice under
  `SOURCE_DATE_EPOCH` and diffing `image.tar.zst` payload hashes;
  non-determinism here would poison L3's payload tolerance story.

**Evidence bar:** the L2 `porttest` track's `docs`/`splitdebug` archives
must be payload-equal to real's under `gpkg-diff.sh --mode strict` for
the transform-affected paths (`BIG.txt.bz2`, `/usr/lib/debug/**`,
`.build-id`), and the corresponding temporary allowlist entries must be
deleted. A run that is green only because the allowlist still matches
is not acceptance.

---

## 4. Slices

### Slice A — recon under resolved FEATURES (F, 3-6 h)

**Goal:** see what the real scripts do when enabled, and classify every
failure before fixing anything.

Steps:
1. Wait for #37's slices A-C (at least the phase-env threading); the
   harness must show resolved `FEATURES`/`USE` in
   `metadata/FEATURES`/`metadata/USE` first.
2. In the L2 container, build `porttest/docs` and `porttest/splitdebug`
   from source archive-only, with the resolved `FEATURES` including
   `binpkg-dostrip binpkg-docompress splitdebug xattr` and
   `PORTAGE_COMPRESS=bzip2` reachable from the resolved config (put it
   in the fixture container's `make.conf`, per `docs/030` §1.4 trap 4 —
   not as an exported env var, so the test exercises #37).
3. Capture: does `install_qa_check` reach the `estrip`/`ecompress`
   lines? What do the logs say (`eerror`/`die` paths)? Which tools are
   missing (`strip`, `objcopy`, `readelf`, `debugedit`, `dwz`,
   `scanelf`, `bzip2`, `rsync`)? Compare the archive image path set
   against `_l1-pkgcache`.
4. Write `TEST/findings/l2.md` (or a new `TEST/findings/l2-transforms.md`
   if it grows): per-check result table, exact failing command, and a
   classification `portuale-env | container-tool | version-skew |
   real-divergence`.
5. Record the G2.2/G2.3/G2.4 answers with data.

**Exit:** every inert/skipped transform has a named cause and a
repro command; the expected path sets are frozen in the findings doc.

### Slice B — strip / splitdebug (M, 4-8 h; F review)

**Goal:** `binpkg-dostrip` + `FEATURES=splitdebug` produce the same
`${D}` (and archive) as real.

Steps:
1. Fix only the classified portuale-env gaps (likely: `PORTAGE_STRIP_FLAGS`
   passthrough if set in config, `STRIP_MASK`, `ARCH`/`KERNEL` — all
   through #37's builder; `QA_PRESTRIPPED` comes from the ebuild env).
2. Confirm `estrip --queue/--ignore/--dequeue` with the EAPI 6+
   `dostrip` helper paths: add a `dostrip -x` case to the fixture only
   if the oracle's real archive proves the semantics; otherwise leave
   `porttest/splitdebug` as-is (fixtures must isolate behavior).
3. Verify: `/usr/lib/debug/<path>.debug` objects, `.build-id/xx/…`
   links, stripped binary sizes matching real's, hardlink handling
   (the fixture compiles both a binary and a lib), and `CONTENTS`
   recording the transformed paths/md5s.
4. Rust tests: unit for any new env pair; e2e container build
   asserting the path set + `gpkg-diff.sh` on the pair.

**Exit:** `porttest/splitdebug` clean in strict `gpkg-diff.sh` against
the real reference archive; its `known-divergences.yaml`
`l2-gpkg-dostrip-splitdebug*` entries deleted.

### Slice C — compress / docompress (M, 4-8 h; F review)

**Goal:** `binpkg-docompress` + resolved `PORTAGE_COMPRESS` reproduce
real's doc/man compression exactly.

Steps:
1. Ensure `PORTAGE_COMPRESS`/`_FLAGS`/`_EXCLUDE_SUFFIXES` arrive from
   the resolved config (slice #37's builder; they are make.globals
   scalars). If the resolved environ deliberately drops them, the drop
   is the bug — do not hardcode `bzip2`.
2. Verify `ecompress --queue "${PORTAGE_DOCOMPRESS[@]}"` behavior on
   `porttest/docs`: which files compress (`BIG.txt` > 128 bytes vs
   `small.txt`), `html/` skip, man compression, symlink repair
   (`fix_symlinks`), and the `.ecompress` residue being cleaned.
3. Verify `dodoc`/`newdoc`/`doman`/`doinfo` outputs against the oracle
   (`_l1-pkgcache/porttest/docs`), including the exact suffix.
4. Verify `CONTENTS` and the gpkg `image.tar` agree (no `.ecompress`
   files, no double entries), and that a source merge (no binpkg)
   installs the same transformed payload real does.

**Exit:** `porttest/docs` clean in strict `gpkg-diff.sh`; the
`l2-gpkg-docompress*` entries deleted.

### Slice D — merge path + CONTENTS interaction (M, 2-4 h)

**Goal:** source merges without `--buildpkgonly` install the
transformed payload, and the vdb matches the oracle.

Steps:
1. Build+merge both fixtures with portuale and with real portage in
   fresh containers (the L2 cross-install direction A machinery already
   exists); diff `$ROOT` filesystems + VDB.
2. Confirm `CONTENTS` md5s are computed post-transform, `NEEDED.ELF.2`
   (written pre-strip) is unchanged, and preserve-libs sees the
   stripped objects.
3. Re-run L1's porttest merge pair to prove no regression from the
   `install_qa_check` path (L1 merges prebuilt archives, so it should
   be untouched).

**Exit:** L2 cross-install porttest 0 unexplained; L1 still green.

### Slice E — `-binpkg-*`/instprep decision (F, 2-6 h or file-only)

**Goal:** no silent divergence when the user disables
`binpkg-dostrip`/`binpkg-docompress`.

Steps:
1. Reproduce the real behavior in a container (`FEATURES="-binpkg-dostrip
   -binpkg-docompress"`, `source merge`): real strips/compresses at
   `instprep` time (`misc-functions.sh:280-298`), portuale does
   nothing.
2. Decide with the user: implement `__dyn_instprep`'s equivalent at
   merge time (the shell call is a small extension of the existing
   `run_misc_functions` surface, but it is a *new* phase invocation in
   the merge path) or file **#38b** with the repro and document the
   cut in `scope-backlog.md`. Recommendation: file, unless slice A/B/C
   show the merge path already invokes `instprep`-adjacent machinery.

**Exit:** either the behavior is implemented and covered by a fixture,
or #38b exists with a repro command and a scope-backlog entry. No
third state.

### Slice F — verification + docs (M/S, 2-4 h + runs)

Steps:
1. `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`:
   remove the #38 `KNOWN_FINDINGS` entries
   (`TEST/run/l2-portuale-builder.sh:65-71`) and the
   `known-divergences.yaml` `l2-gpkg-dostrip-*`/`l2-gpkg-docompress-*`
   entries as their checks pass; re-run clean.
2. Re-run the L2 real set (`L2_REBUILD=1 L2_MODE=payload-tolerant
   L2_BUILD_MODE=deep ... atomlists/l1-merge.txt`) and record the new
   stop point in `TEST/findings/l2.md`.
3. L3 sanity when the bed exists: `l3-smoke`'s payload findings must no
   longer include transform classes.
4. Docs: new `docs/what-this-proves.md` paragraph with a live-verified
   example (build `porttest/docs`, list `image.tar.zst`, show
   `BIG.txt.bz2`); `scope-backlog.md` §K entry updated/closed;
   `backlog-tasks.md:74` marked done; #38b added if slice E files it.

---

## 5. Fixtures, oracles, and tests

| What | Where | Notes |
|---|---|---|
| `porttest/docs` | `TEST/images/overlay/porttest/porttest/docs/` | `BIG.txt` (>128 B) vs `small.txt`, `html/`, man, info |
| `porttest/splitdebug` | same | compiled binary+lib, `tc-getCC`, `.debug` + build-id |
| Real archive oracle | `TEST/logs/_l1-pkgcache/porttest/{docs,splitdebug}` | **the** expected path set/suffix; consult before asserting |
| gpkg comparison | `TEST/compare/gpkg-diff.sh` (`--mode strict`), `gpkg-structure.sh` | archive-vs-archive, payload bytes |
| L2 runner | `TEST/run/l2-portuale-builder.sh` + `KNOWN_FINDINGS` (`:57-71`) | delete entries as they pass |
| VDB/root diff | `TEST/compare/{normalize.py,diff.py}` (`--layer l2 --tolerate-payload`) | cross-install direction A |
| Rust unit tests | `rust/portuale/src/ebuild_phases.rs` (`run_commands`/misc-functions), `ebuild_package.rs` | env wiring; no Python mirror (real-execution-only) |
| Python contract | `tests/` | must stay green; this slice adds no CLI surface |

Fixture rule (`AGENTS.md` step 5): no new fixture if `docs`/`splitdebug`
already isolate the behavior — extend them only with oracle-backed
assertions. Check `fixtures/repo/` collisions before adding any.

**Verification pass (step 8):** `cargo fmt --check`, `cargo clippy
--release --all-targets`, `cargo test --release`, `python3 -m pytest
tests -q`, then the L2 porttest container run. Because this slice
touches the packaging/merge path, also run `TEST/run/
l1-merge-from-binpkg.sh` (L1) to prove the archive consumer is
unaffected.

---

## 6. Risks, traps, stop rules

1. **Version skew.** `estrip`/`ecompress` come from portuale's vendored
   `bin/` (this repo's portage), while the comparison reference is the
   image's installed portage 3.0.82.2. Any unexplained diff must be
   classified against *both* sources before it is called a bug
   (`misc-functions.sh`, `estrip`, `ecompress` differ across versions).
2. **Missing tools.** `debugedit`/`dwz` are optional but change
   `.build-id` handling (`estrip:160-210`); `rsync` is needed by
   `installsources` only (out of scope); `bzip2` and `scanelf` must be
   on `PATH`. Record tool presence in the recon so later diffs are not
   misattributed.
3. **Determinism.** Two identical builds may still differ in payload
   if the toolchain embeds paths/ids. Use `SOURCE_DATE_EPOCH` + `-j1`
   like the L3 block; for strict L2, compare against a portage-built
   reference *of the same fixture* (already the harness contract).
4. **`QA_PRESTRIPPED` warnings are output, not failure.** `estrip`
   `eqawarn`s pre-stripped files; if a fixture's binary is already
   stripped by the compiler, the QA log differs but the payload may
   not. Compare payload first, warnings second.
5. **`PORTAGE_RESTRICT` reduction.** Portuale sets `PORTAGE_RESTRICT`
   with an empty USE set for non-`depend` phases
   (`ebuild_phases.rs:2079-2086`); a `RESTRICT="strip? ( … )"` form
   would diverge from real's USE-reduced value. The fixtures use plain
   `RESTRICT`; if a real package hits this, file it against #37's
   reduction rather than patching `estrip`.
6. **Do not "fix" #39 here.** `NEEDED.ELF.2` field count and gpkg
   metadata members will keep `gpkg-diff` noisy; they are explicitly
   #39. Deleting their allowlist entries is out of this slice.
7. **Stop rule.** If slice A's classification counts more than three
   distinct systemic transform blockers (beyond env threading), stop,
   file them with repros, and escalate sequencing to the user — do not
   grow the slice into "port estrip".

---

## 7. Delegation brief (for subagents)

Before delegating: state that **#37 must have landed** (or the
subagent must use #37's builder, not hardcoded vars); hand over the
project/generation from `list_projects`/`index_status`; the G2 answers
from §3; the exact fixture/oracle paths from §5; and the classification
table from slice A if it exists. A subagent must:

- never edit `bin/estrip`/`bin/ecompress`/`bin/misc-functions.sh`
  (vendored real source) to make a test pass;
- never add a transform switch outside the resolved config;
- derive expected paths from `TEST/logs/_l1-pkgcache/porttest/*`, and
  label any claim as provisional until the container run confirms it;
- return the diff, the exact commands run, the archive listing and
  `gpkg-diff.sh` output, and a per-entry list of allowlist entries
  deleted. A subagent without container access may implement and unit
  test slices B/C but must mark them **unverified end-to-end**.
