# 038 — Packaging transforms: dostrip / splitdebug / docompress — agent plan (musespark draft)

Status: **not started.** Covers backlog #38 (`docs/backlog-tasks.md:74`),
the `scope-backlog.md` §K entries `l2-gpkg-dostrip-splitdebug` + `l2-gpkg-docompress`
(`:669-672`), and the findings `TEST/findings/l2.md:121-132` (S0) +
`:255-281` (S3). Depends on #37 (the strip/compress branches gate on
resolved `FEATURES` + the `PORTAGE_COMPRESS` family #37 threads
through); verify against #37's S5, not against today's env.

**Read first:** `AGENTS.md` (steps 4/5/7/8 — §2 rule 1 below),
`docs/agent-context.md`, `TEST/findings/l2.md` S0+S3,
`docs/037_Build-phase-env-completeness.musespark.md` (sibling plan —
§1.1's dependency list is this plan's prerequisite),
`docs/030_L3-source-build-parity.deepseek.md` G0.5 (Tier-5 items land
as their own commits), and the code under test:
`rust/portuale/src/ebuild_package.rs` (`invoke_dyn_package` `:773`,
`package_after_install` `:487`), `rust/portuale/src/ebuild_phases.rs`
(`run_misc_function`, `phase_features_value` `:1581`),
`TEST/compare/known-divergences.yaml` (`l2-gpkg-dostrip-splitdebug*`,
`l2-gpkg-docompress*`). Real-portage semantics (all vendored, all
unmodified, all already on disk):
`3rdparty/portage/bin/misc-functions.sh:120-308` (`__dyn_package`
`:120-263`, `__dyn_instprep` `:265-308`), `bin/estrip` (the whole
script — queue/ignore/dequeue/prepallstrip + `splitdebug`/`nostrip`/
`compressdebug`/`installsources`/`xattr` gates `:407-460`),
`bin/ecompress` (whole script — `PORTAGE_COMPRESS`/`_FLAGS`/`_SUFFIX`,
`--queue/--ignore/--dequeue`), `bin/phase-helpers.sh:21-28`
(`PORTAGE_DOCOMPRESS`/`DOSTRIP` bash defaults) + `:164-220`
(`docompress()`/`dostrip()` ebuild helpers),
`bin/phase-functions.sh:579-660` (`__dyn_install` — note what it does
*not* call).

Model tiers, same convention as 022/023/024/025/029/030:

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

**Second of the two Tier-5 items, and splittable into three
independently-shippable transforms.** The backlog one-liner ("no
dostrip/estrip, no splitdebug, no ecompress") reads like one gap but
grounds out as three, with different shapes:

- **(a) `ecompress`/`docompress`** — the most mechanical. One script
  (`bin/ecompress`), gating on `PORTAGE_COMPRESS` non-empty +
  `binpkg-docompress` in `FEATURES` (`misc-functions.sh:147-153` for
  the package path, `:280-286` for the merge path). Path lists are
  bash defaults (`phase-helpers.sh:21-23`), already vendored. The work
  is env + invocation, both already seamed.
- **(b) `estrip`/`dostrip`** — medium. One script (`bin/estrip`),
  gating on `binpkg-dostrip` in `FEATURES` (`misc-functions.sh:239-252`
  vs `:288-299`), EAPI-gated helper variants
  (`___eapi_has_dostrip`), `RESTRICT=strip`/`nostrip` +
  `FEATURES=nostrip` interactions (`estrip:407-460`), queue scoping
  (`PORTAGE_DOSTRIP`, default `/` — `:24-28`). Same env + invocation
  shape as (a), plus the RESTRICT matrix to pin.
- **(c) `splitdebug`** — the hardest. Lives *inside* `estrip`
  (`do_splitdebug`, `save_elf_debug`, `.debug` + `.build-id` layout,
  `installsources` interplay), needs external tools (`debugedit`,
  `scanelf`), and its outputs are the L2 fixture track's sharpest
  assertions (`/usr/lib/debug/**`, setuid size 15424→14384). Do it
  last, against the other two being green.

The single most important grounding result: **portuale already runs
the real code that contains these branches.** `invoke_dyn_package`
(`ebuild_package.rs:773`) executes the real, unmodified
`__dyn_package`, which *is* the `binpkg-docompress`/`binpkg-dostrip`
call site. The branches don't fire because (1) `FEATURES` is the raw
process env — the L2 builder exports
`buildpkg binpkg-multi-instance splitdebug xattr -sign`, which
contains *neither* `binpkg-docompress` nor `binpkg-dostrip` (real gets
them default-on from `make.globals` via resolved config — i.e. #37's
S1), (2) `PORTAGE_COMPRESS`/`_FLAGS` are never set (whitelisted,
`ebuild_phases.rs:1469-1471`, but absent), and (3) the merge path
never runs `__dyn_instprep` at all — no caller references it outside
the `instprep` CLI action name (`ebuild_options.rs:90`), so a normal
`emerge <atom>` merge gets *neither* the package-path *nor* the
instprep-path transforms. **Do not reimplement any transform in Rust.**
Set the env, invoke the real scripts on the real paths, pin with
fixtures.

### 0.2 Frontier or cheap model?

**M throughout, F review on (b) and (c); (a) can ship on M alone.**
The work is read-bash-carefully + set-env + run-fixtures; the bed
grades objectively (archive path sets either match or they don't).
The judgment surface is the RESTRICT/FEATURES matrix in (b) and the
tool-absence degradation in (c) (no `debugedit` on a minimal host must
degrade exactly the way real does — `estrip`'s own error path, not a
Rust pre-check that anticipates it). If only cheap models are
available: ship (a) + the env half of (b), hand (c) to a frontier
session with the (a)+(b)-green bed in hand.

### 0.3 Difficulty, by axis

| Axis | 1-5 | Notes |
|---|---|---|
| Real-bash comprehension | 3 | `estrip` ~700 lines + `ecompress` ~260 + the two `misc-functions.sh` call sites; dense but self-contained, no resolver/depgraph contact |
| Env plumbing (Rust) | 2 | #37 built the pipes; this item mostly turns the taps (one or two new config scalars) |
| Invocation wiring | 2.5 | where `__dyn_instprep` (or the estrip/ecompress calls directly) runs on the merge path; ordering vs `merge_tree` matters |
| RESTRICT/FEATURES matrix | 3 | (b): `nostrip`/`strip`/`binchecks`/`splitdebug` × FEATURES tokens; each cell needs a fixture assertion |
| Toolchain dependence | 3.5 | (c): `strip`, `debugedit`, `scanelf`, compressors (`bzip2`/`gzip`/`xz`/`zstd`) on minimal/musl hosts; absence must match real, not crash |
| Determinism | 2 | strip/compress/debug-split are deterministic transforms; payload bytes change *by design* (L2 strict mode already expects the new bytes — update allowlist, don't fight it) |
| **Overall** | **3** | three M-sized slices in a trench coat; the risk is shipping them as one unreviewable blob (don't — §4) |

---

## 1. Ground truth

### 1.1 What exists (reuse, do not rebuild)

- **Real scripts, vendored:** `bin/estrip`, `bin/ecompress`,
  `bin/misc-functions.sh` (`__dyn_package` + `__dyn_instprep`),
  `bin/phase-helpers.sh` (`docompress()`/`dostrip()` helpers +
  `PORTAGE_DOCOMPRESS/DOSTRIP` defaults). Portuale runs them
  unmodified — the fix is env + invocation, never a Rust
  reimplementation.
- **Package-path invocation:** `invoke_dyn_package`
  (`ebuild_package.rs:773-891`) runs real `__dyn_package` with an
  `extra_env` that already handles the gpkg/xpak compressor split
  (`BINPKG_COMPRESS*` vs `PORTAGE_COMPRESSION_COMMAND`). The
  strip/compress env rides the same `extra_env`.
- **Merge-path gap:** `ebuild_merge::run_merge` runs the `install`
  chain + `merge_tree` (+ pre/postinst). Nothing on this path invokes
  `__dyn_instprep` or the binpkg-strip/compress branches — that is the
  (a)/(b) merge-path half of the work. Check `run_misc_function`'s
  existing call sites before adding one (same sandbox/capture
  handling as every other misc-function call).
- **L2 oracles:** `porttest/docs` (doc tree → `BIG.txt` vs
  `BIG.txt.bz2`; `TEST/images/overlay/porttest/README.md:32` documents
  the expected compressed/uncompressed split per helper),
  `porttest/splitdebug` (binary + soname-lib → `.debug` + `.build-id`;
  `README.md:35`), real-vs-portuale size witness (setuid 15424 vs
  14384, `l2.md:255-259`). The `l2-portuale-builder.sh`
  `KNOWN_FINDINGS` regexes (`:65-71`) enumerate the exact paths.
- **#37 prerequisite:** resolved `FEATURES` in the phase env (037/S1),
  `PORTAGE_COMPRESS*` scalars if they prove to be config values (S0
  task 1 settles this — they may already flow via `other_vars` once
  #37 lands, since they are whitelisted).

### 1.2 What is missing (the slice list in §4)

| # | Gap | Real call site | Portuale site |
|---|---|---|---|
| 1 | `PORTAGE_COMPRESS`/`_FLAGS`/`_SUFFIX` unset → `ecompress` dies/skips | `ecompress:228-260`; set from `make.globals` via config | never set (whitelisted, absent) |
| 2 | Resolved `FEATURES` lacks `binpkg-docompress`/`binpkg-dostrip` → both `__dyn_package` branches skip | `misc-functions.sh:147-153,239-252` | raw process env (#37/S1) |
| 3 | Merge path never runs `__dyn_instprep` → `!binpkg-*` complements never fire | `misc-functions.sh:265-308` | no caller |
| 4 | `estrip` RESTRICT/FEATURES matrix unpinned (no fixture asserts any cell) | `estrip:407-460` | — |
| 5 | `splitdebug` tool path (`debugedit`, `.build-id` links, `installsources`) unexercised | `estrip:98-290,632+` | — |

Out of scope: `packdebug` tarball (`__generate_packdebug`,
`misc-functions.sh:507-567` — separate FEATURES, separate slice if
ever needed); `QA_PRESTRIPPED`/`install-qa-check.d` (QA notices, not
payload); `compressdebug`/`installsources` beyond what splitdebug's
slice needs to keep them from regressing; deterministic-compression
flag tuning (ship real's defaults first).

### 1.3 Traps found while scoping (encode these, don't rediscover them)

1. **Two paths, complementary gates — exactly one transform must fire
   per merge.** `__dyn_package` strips/compresses iff the `binpkg-*`
   token is present; `__dyn_instprep` does iff it is *absent*
   (`misc-functions.sh:147-153` vs `:280-286`, `:239-252` vs
   `:288-299`). Wiring both invocations without the complement check
   double-strips (or double-compresses). Mirror real's exact
   condition, don't approximate it.
2. **`binpkg-dostrip` off does not mean "no strip".** Note at
   `misc-functions.sh:241`: "disabling it won't help with packages
   calling prepstrip directly" — ebuild-called `dostrip()`/
   `prepstrip` still run. A fixture asserting "no strip happened"
   under `-binpkg-dostrip` is wrong unless the ebuild calls nothing.
3. **`PORTAGE_COMPRESS=""` (empty) is a real state** (`ecompress:254`:
   "to disable compression, set `PORTAGE_COMPRESS=\"\"` instead" of
   `true`/`:`). Empty must flow through as empty, not as absent —
   the `build_config_env` "skip empties" shape is *wrong* for this
   var; set it explicitly.
4. **Payload bytes change by design.** Stripped binaries and
   compressed docs produce *different* bytes than today. L2 strict
   fixtures + `KNOWN_FINDINGS` + `known-divergences.yaml` must be
   updated to the new expected bytes in the same commit — a red bed
   after these slices means the expectations weren't updated, not
   (necessarily) that the transform is wrong. Disambiguate by
   diffing portuale-vs-real, never portuale-vs-old-portuale.
5. **Tool absence is a real behaviour, not a test-skip.** If
   `debugedit`/`scanelf`/a compressor is missing in the test
   container, real portage takes a specific path (warn/skip/die —
   read each script's own handling). Match it; don't gate the Rust
   side on tool presence.

---

## 2. Rules and invariants for every slice

1. **Real-execution only: Rust + Rust tests, no Python mirror, no
   contract `CASES`.** AGENTS.md step 4's carve-out (same as L2 and
   037). Cover each slice with a Rust unit test + a Rust
   fixture-driven e2e test (`tests/test_portuale.py` pattern:
   build the `porttest/docs` / `porttest/splitdebug` fixtures, assert
   archive bytes/paths), and prove it on the L2 bed.
2. **Never reimplement a transform in Rust.** Env + invocation of the
   vendored scripts only. If a script proves un-runnable as-is, that
   is a finding (file it, don't fork the script — `brush-pin.md`'s
   thin-fork precedent does not extend to portage's `bin/`).
3. **One transform per slice (§4).** (a), (b), (c) land as separate
   commits in that order; each leaves the bed green-or-better than it
   found it.
4. **Never weaken L0/L1/L2.** Full verification pass (AGENTS step 8)
   per slice; the L2 fixture track must show the claimed rows fixed
   and no new unexplained finding.
5. **Evidence in `TEST/findings/l2.md`.** Per-slice before/after
   (archive listings, sizes, `.debug` trees) with exact commands.

---

## 3. Gates (owner decisions — ask before S1)

- **G0.1 Order inside the item.** Recommendation: (a) ecompress → (b)
  estrip → (c) splitdebug. Rationale: dependency order (each later
  slice's assertions assume the earlier transform's bytes), and
  ascending tool-dependence. Owner: user.
- **G0.2 Merge-path mechanism.** Recommendation: invoke the real
  `__dyn_instprep` via the existing `run_misc_function` seam at the
  same point real `doebuild` runs it (before `merge_tree`, after the
  `install` chain — cite the real call order in the commit), rather
  than calling `estrip`/`ecompress` directly. Rationale: inherits
  real's complement-gating (trap 1) and future upstream changes for
  free. Owner: user (S0 presents the exact real call order with
  citations; if it doesn't fit `run_merge`'s structure, escalate
  rather than improvising).
- **G0.3 `RESTRICT=strip` semantics.** `estrip` treats explicit
  `RESTRICT=strip` as force-strip even under `nostrip`-ish setups
  (`phase-functions.sh:770,824` carry the same formula). Recommendation:
  no special-casing — the real script already implements it once the
  env is right; fixtures pin it. Owner: user only if a fixture
  contradicts the script.

---

## 4. Slices

### S0 — Recon: call order + var provenance (M, 2–4 h)

**Goal:** no unknowns before touching invocation. Answers G0.2 with
citations.

Steps:

1. Settle `PORTAGE_COMPRESS`/`_FLAGS`/`_SUFFIX`/`_EXCLUDE_SUFFIXES`
   provenance: config (`make.globals` → `resolve_config`) or
   bash default? If config, confirm #37 already threads them (or
   file the delta into 037/S4 — don't fix it here).
2. Map real's exact transform call order across `doebuild.py`
   (which phases call `__dyn_package` vs `__dyn_instprep`, and where
   relative to `merge()`), `misc-functions.sh:120-308`, and
   `phase-functions.sh:579+`. Write the ordered list with line
   citations into `TEST/findings/l2.md`.
3. On the L2 fixture pair, record the per-path expected outputs:
   `porttest/docs` (which files compress, to what suffix, which stay
   — `README.md:32` is the claim, verify against real's archive),
   `porttest/splitdebug` (full `.debug` + `.build-id` tree from
   real's archive).
4. Check tool presence in the L2 image (`strip`, `debugedit`,
   `scanelf`, `bzip2`/`gzip`/`xz`/`zstd`) and record real's
   absence-path for each (trap 5).

**Acceptance:** ordered call list + provenance table + expected-output
trees, all cited; G0.2 answered with data; no code changed.

### S1 — (a) `ecompress` / `docompress` (M, 4–8 h)

**Goal:** `porttest/docs` archives match real path-for-path
(`BIG.txt.bz2`, not `BIG.txt`).

Steps:

1. Thread `PORTAGE_COMPRESS` (+ `_FLAGS`/`_SUFFIX`/
   `_EXCLUDE_SUFFIXES`, per S0's provenance — explicit-empty
   handling per trap 3) into the package + merge transform env,
   behind #37's resolved-`FEATURES` (which must contain
   `binpkg-docompress` by default — verify, don't assume).
2. Wire invocation per G0.2 (package path: already runs
   `__dyn_package` — confirm the branch now fires with env alone;
   merge path: the `__dyn_instprep` complement).
3. Fixture e2e: `porttest/docs` strict path-set parity vs real
   (compressed suffix, `docinto html` uncompressed, `doinfo`
   uncompressed — the `README.md:32` split, verified in S0).
4. Update `KNOWN_FINDINGS` + `known-divergences.yaml`
   (`l2-gpkg-docompress*` deleted), re-run the L2 fixture track.

**Acceptance:** `l2-gpkg-docompress` closed with evidence; docs
fixture path sets byte-match real; full suite green.

### S2 — (b) `estrip` / `dostrip` (M, F review, 6–10 h)

**Goal:** binaries ship stripped; the RESTRICT/FEATURES matrix pinned.

Steps:

1. Thread `STRIP_MASK` (if S0 shows it as config) + confirm
   `PORTAGE_DOSTRIP` defaults come from bash (no action if so).
2. Same invocation shape as S1 for the `binpkg-dostrip` /
   `!binpkg-dostrip` complements.
3. Pin the matrix with fixtures (extend `porttest/splitdebug` or a
   new `porttest/strip` fixture only if needed — prefer existing):
   default strip (setuid size witness 15424→14384),
   `RESTRICT=strip` force, `RESTRICT=nostrip` / `FEATURES=nostrip`
   skip, ebuild-called `dostrip()` under `-binpkg-dostrip` still
   strips (trap 2).
4. Update `KNOWN_FINDINGS` + `known-divergences.yaml`
   (`l2-gpkg-dostrip-splitdebug*` split: the strip rows close here,
   the debug rows stay for S3 — rescope the entries with evidence,
   don't delete what isn't green).

**Acceptance:** strip rows of `l2-gpkg-dostrip-splitdebug` closed;
matrix cells pinned dual-... (no — Rust-only, §2 rule 1) Rust-e2e
pinned; full suite green.

**Stop rule:** if `estrip` fails on the fixture track for toolchain
reasons (missing `strip` features on musl, `QA_PRESTRIPPED` handling),
file with the repro and escalate — don't patch around the real
script (rule 2).

### S3 — (c) `splitdebug` (M, F review, 6–12 h)

**Goal:** `/usr/lib/debug/**` + `.build-id` parity with real.

Steps:

1. Confirm `FEATURES=splitdebug` reaches the transform env (L2
   builder exports it; #37/S1 makes it resolved) and `debugedit` is
   present where the transforms run (image + minimal-host story per
   S0 task 4).
2. Drive via the same invocation as S2 (splitdebug is an `estrip`
   internal — no new call site, only env + tools). Pin
   `compressdebug`/`installsources` non-regression alongside (they
   share `estrip`'s flag matrix).
3. Fixture e2e: `porttest/splitdebug` strict tree parity (binary AND
   soname-lib `.debug` + `.build-id` symlinks), plus `NEEDED.ELF.2`
   sanity (strip changes debuglink, not NEEDED — assert no
   `l2-needed-elf2-format` regression; that entry belongs to #39,
   don't absorb it).
4. Close/rescope the remaining `l2-gpkg-dostrip-splitdebug*` entries;
   re-run the L2 fixture track to 0 unexplained.

**Acceptance:** `l2-gpkg-dostrip-splitdebug` closed with evidence;
splitdebug tree matches real; full suite green.

### S4 — L2 real-set check + closeout (M, 2–4 h + container wall-clock)

**Goal:** prove the transforms at real scale and close the item.

1. Re-run the L2 fixture track (must be 0 unexplained) and, if #37's
   S5 already unblocked deep mode, the real-set builder as far as it
   goes — triage packaging-class findings here, env-class back to
   #37, anything else filed not absorbed.
2. Delete every fixed `known-divergences.yaml` + `KNOWN_FINDINGS`
   entry; print no-longer-matching entries as removal candidates.
3. Docs: `scope-backlog.md` §K (both entries closed with evidence
   pointers), `backlog-tasks.md` #38 → DONE, `what-this-proves.md`
   one appended paragraph with a runnable live-verified example
   (AGENTS step 7 — e.g. `porttest/docs` `BIG.txt.bz2` +
   `porttest/splitdebug` `.debug` tree before/after).
4. Full verification pass (AGENTS step 8).

---

## 5. Routing summary

| Slice | Tier | Effort | Depends on | Deliverable |
|---|---|---|---|---|
| S0 | M | 2–4 h | #37 S1 (resolved FEATURES) | call order + provenance + expected trees |
| S1 | M | 4–8 h | S0 | (a) ecompress; `l2-gpkg-docompress` closed |
| S2 | M (F review) | 6–10 h | S1 | (b) estrip; strip rows closed + matrix pinned |
| S3 | M (F review) | 6–12 h | S2 | (c) splitdebug; debug rows closed |
| S4 | M | 2–4 h + wall-clock | S1–S3 | real-set check + docs |

Total: **~20–38 agent-hours, 4–6 sittings**, after #37. S0+S1 are one
sitting; S2, S3 one each; S4 rides a container run. A cheap model can
take S0+S1 alone and stop cleanly (S2/S3 wait on its output without
rework).

---

## 6. Definition of done

- [ ] S0 call order + provenance cited; G0.1–G0.3 answered.
- [ ] `l2-gpkg-docompress` closed (S1); strip rows closed (S2); debug
      rows closed (S3) — each with `l2.md` before/after evidence.
- [ ] RESTRICT/FEATURES strip matrix pinned by Rust e2e tests.
- [ ] No transform reimplemented in Rust; no Python mirror touched.
- [ ] L2 fixture track 0 unexplained; full verification pass green.
- [ ] Docs updated (S4); no dead allowlist entries left behind.

## 7. Review checklist (attach to each slice)

- Exactly one transform per slice? (No (a)+(b)+(c) blob.)
- Complement gating mirrored exactly (`binpkg-*` vs `!binpkg-*` —
  trap 1)? No double-strip/double-compress path?
- `PORTAGE_COMPRESS=""` flows as empty, not absent (trap 3)?
- Bed triaged portuale-vs-real, not vs-old-portuale (trap 4)?
- Tool-absence paths match the script, no Rust pre-gating (trap 5)?
- `l2.md` evidence appended with exact commands?
- Rust unit + e2e tests added; no Python mirror touched?

## 8. Risks

1. **#37 slippage.** Every branch here gates on resolved `FEATURES`.
   If #37/S1 isn't landed, S0 recon is still doable but S1–S3 must
   wait — do not stub the env to "make progress".
2. **Double-transform.** Trap 1 is the classic failure; G0.2's
   mechanism (real `__dyn_instprep`, real conditions) exists to
   prevent it. Any slice diff showing both call sites added without
   the complement condition is wrong on sight.
3. **Toolchain gaps on minimal hosts.** `debugedit` especially may be
   absent outside the L2 image. Real's absence-path is the spec;
   a musl-static story question goes to the user, not into a
   silent skip.
4. **Expectation churn.** Payload bytes change by design across all
   three slices — reviewers must expect `KNOWN_FINDINGS`/yaml churn
   and check it against real, not against the previous portuale.
5. **`packdebug` pull.** Explicitly out (see §1.2) — if `FEATURES=packdebug`
   appears in a test env, that's a pre-existing gap, not this item's.

## 9. Non-goals

- `packdebug` tarballs, QA checks, `QA_PRESTRIPPED` handling beyond
  not breaking it.
- `PORTAGE_DOCOMPRESS/DOSTRIP` content changes (bash defaults stand).
- Compressor flag tuning / deterministic-compression claims.
- Resolver/merge/scheduler/compare-stack changes of any kind.
- Python mirror, contract `CASES`, `--json` changes.
- L3/`@system` work (L3 verifies packaging in its S3; it doesn't own
  it).

## 10. Findings filed while executing

(append here as S0–S4 run; one entry per finding: command, expected,
actual, root cause, fix ref / backlog id)
