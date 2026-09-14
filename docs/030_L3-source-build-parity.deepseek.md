# L3 — source-build parity — agent plan (deepseek draft)

Status: **in progress — S0–S2 landed 2026-09-14; S3 verification-mode;
S4 partial (stop rule invoked).** The harness is shipped
(`TEST/run/l3-source-parity.sh` + `TEST/layers/l3/build-and-merge.sh` +
`l3-{smoke,core,system}.txt`) and the portage-vs-portage control pair is
**0 unexplained on `l3-smoke`** (full-tree, no per-path rule needed).
The candidate run (`l3-20260914T021049Z`) builds and merges all 6 smoke
packages under portuale and surfaced four producer classes; two are
fixed with Rust regression tests (`l3-subslot-self-collision`,
`l3-config-multi-assignment-stacks`) and two are filed with repros
(`l3-merge-owner-1-1`, `l3-merge-vdb-env-accumulates`). Per the S4 stop
rule (>3 distinct systemic classes), `l3-core` (344 ebuilds with `-e`)
and `@system` (368) are deliberately not started until those two are
fixed. S3's #37/#38 prerequisites landed as their own Tier-5 slices
(G0.5), so it is verification-mode. See `TEST/findings/l3.md` for the
live record. Written 2026-09-13 against `main` @ `473b8f8` (L2 S6).
Covers backlog #30 (`docs/backlog-tasks.md:56`), the
`scope-backlog.md` §I L3 bullet (`:605-607`), the layer design
`docs/real-world-testing.md` §5 L3 (`:126-134`) plus its §2 determinism
controls (`:26-51`), §3 triage table (`:53-71`), §7 risks (`:179-195`)
and §8 metrics (`:197-206`), and the retired planning doc
`docs/history/real-world-testing.md` §14 item 7 (`:906`) and §5
(`:475-481`).

**Read first:** `AGENTS.md` (steps 4/5/7/8), `docs/agent-context.md`,
[`TEST/README.md`](../TEST/README.md) (L1/L2 runbooks), the L2 plan
[`029_portuale-as-builder.deepseek.md`](029_portuale-as-builder.deepseek.md)
(its opinion/rules/gates sections are the template for this one; its
"Findings filed while executing" is why L3 is blocked),
[`TEST/findings/l2.md`](../TEST/findings/l2.md) `:310-372` (the producer
gaps L3 inherits), [`TEST/findings/l1.md`](../TEST/findings/l1.md) (the
harness's hard-won gotchas), `docs/real-world-testing.md` §2–§3 + §5 +
§8, `TEST/compare/normalize.md` (the normalisation contract), the
existing engines `TEST/run/l1-merge-from-binpkg.sh`,
`TEST/run/l2-portuale-builder.sh`, `TEST/layers/l1/{build,consume}.sh`,
`TEST/layers/l2/build-{portage,portuale}.sh`, `TEST/run/lib.sh`, and the
code under test: `rust/portuale/src/emerge_build.rs` (module doc + the
`run_source_merge` call at `pretend.rs:11850`),
`rust/portuale/src/ebuild_phases.rs` (`ENVIRON_WHITELIST` `:1387-1541`,
`environ_whitelisted` `:1544`). Real-portage semantics:
`3rdparty/portage/lib/portage/package/ebuild/doebuild.py` and
`.../package/ebuild/config.py::environ()` (`:3263`).

Model tiers, same convention as 022/023/024/025/029:

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

**L3 is the right next layer after L2, but it is a producer-qualification
campaign, and the honest headline is: the bed is cheap, the green gate is
not, and the bill is entirely in portuale bugs that are already filed.**

- What L3 uniquely proves: that when portuale *runs the real ebuild phase
  chain over a real tree at system scale* and merges the result, the
  installed system is materially the same as real portage's — VDB
  metadata, CONTENTS structure, paths, modes, owners, symlinks. No
  fixture suite can make that claim; the pytest contract suite and cargo
  tests stop at hand-picked inputs, and L1/L2 only cover the consumer and
  archive halves.
- Why after L2: L2 shipped the consumer/packaging half and, in doing so,
  filed exactly the producer gaps L3 walks into — `l2-bpkgonly-env`
  (build-phase env is a curated whitelist, not the resolved config env),
  `l2-gpkg-dostrip-splitdebug`, `l2-gpkg-docompress`,
  `l2-gpkg-metadata-members` (backlog #37–#39,
  `docs/backlog-tasks.md:65-69`). L2's real set is red because of them.
  **L3 cannot be green before #37 and #38 land.** #39's *VDB* half
  (repository/IUSE/IUSE_EFFECTIVE/NEEDED.ELF.2 on a source merge) will
  surface here too. #40 (consumer env of portuale-built archives) and
  #41 (cache-less repo) are not on L3's path.
- The truth about the producer today: L2 S5 built and merged **13 real
  packages** under portuale from source, then died at `app-misc/jq::
  configure` ("Package 'oniguruma' not found") because
  `toolchain-funcs::get_libdir` fell back to `lib` (no
  `MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*` in the phase env)
  (`TEST/findings/l2.md:310-348`). That is the same class of failure that
  every `@system` package will repeat until #37 lands.
- What L3 costs: harness ~1–2 days (mechanical, L1/L2 template);
  producer prerequisites ~2–4 days (they are #37/#38's own work, shared
  with Tier 5); bring-up on a small real closure ~2–4 days; `@system`
  is hours of wall-clock per run plus triage. Realistic total **8–16
  agent-days across 7–12 sittings**, with the wall-clock and the triage
  dominating.
- Recommendation: scope v1 DoD as *harness shipped + portage-vs-portage
  noise floor pinned + `l3-core` green + `@system` run with every
  finding fixed or filed*. `@system` **green** is the stretch goal, gated
  on #37/#38. A desktop `@world` soak is a follow-on axis, never v1.
  Do not sell #30 as one slice, and do not let it absorb #37/#38
  silently: land those as their own slices with their own commits
  (G0.5).

### 0.2 Frontier or cheap model?

**Hybrid, ~30% M / ~70% F, and the split is clean.**

| Slice | Content | Tier | Why |
|---|---|---|---|
| S0 | recon spike; decide G0.1–G0.4 with data | **F** | unknown-unknowns; a wrong mechanism choice poisons every later run |
| S1 | orchestrator + layers + atomlists + report | M | copies L1/L2; every interface is frozen here |
| S2 | portage-vs-portage noise floor + normalisation | M (F review) | mechanical with judgment at the edges (what is legitimately noise) |
| S3 | producer prerequisites (#37/#38 + SDE plumbing) | **F** | cross-language env/config semantics; real ebuild behaviour |
| S4 | `l3-core` bring-up + triage | **F** | misclassification is the failure mode |
| S5 | `@system` gate + metrics | **F** | same, at scale |
| S6 | docs closeout | S/M | mechanical |

A cheap model can land S1 + S2 against the existing corpus and stop
cleanly. It must not run S3–S5: the failure mode there is not "less
code", it is **misclassification** — allowlisting a structural diff to
reach green, or "fixing" nondeterminism by tolerating too much. If only
cheap models are available, ship S1+S2 and hand S3–S5 to a frontier
session with the S2 noise set in hand.

### 0.3 Difficulty, by axis

| Axis | 1-5 | Notes |
|---|---|---|
| Bash orchestration | 2.5 | L1/L2 are the template, but: two long runs, a determinism block, partial-failure capture, wall-clock timeouts |
| Environment | 4 | rootful podman, 1.7 GB image, network fetches, hours per `@system` run, disk, inspectability |
| Comparison semantics | 3.5 | `normalize.py`/`diff.py` already do VDB + payload tolerance; the new work is the source-build noise floor (container identity, regenerated files) |
| Producer bring-up (phases/eclasses/toolchain) | **5** | the actual unknown: gcc/glibc/python/perl/toolchain-funcs/flag-o-matic/multilib/python-r1/perl-functions/go-module/cargo/meson/cmake … |
| Triage (portuale bug vs portage bug vs environment) | **5** | the dominant cost; needs real-source grounding |
| **Overall (green `@system`)** | **4.5** | harness alone is a 2.5 |

---

## 1. Ground truth

### 1.1 What exists (reuse, do not rebuild)

- **L1 engine**: `TEST/run/l1-merge-from-binpkg.sh` (`ensure_portuale_built`
  + `ensure_image` + run/consume/normalise/diff flow),
  `TEST/layers/l1/build.sh` (Portage source build → shared `$PKGDIR`),
  `TEST/layers/l1/consume.sh` (one PM merges into a fresh container,
  builds the CONTENTS path list, snapshots via `snapshot.sh`,
  `meta.tsv`). L3's in-container half is this script's shape with the
  merge replaced by build+merge.
- **L2 builders**: `TEST/layers/l2/build-{portage,portuale}.sh` — the
  same contract on both sides, the `bpkgonly|deep` mode switch, the
  shared-`DISTDIR` convention, `FEATURES`/`BINPKG_FORMAT` pinning, and
  the porttest overlay staging block (`build-portuale.sh:30-39`).
  `TEST/run/l2-portuale-builder.sh` — the orchestrator shape: per-PM
  caches under `TEST/logs/`, `KNOWN_FINDINGS` classifier, report +
  symlink.
- **Compare stack**: `TEST/compare/snapshot.sh` (full-tree or `--paths`
  walk, `.files.tsv` + `.mtimes.tsv` + `.vdb.tar` + `.meta.tsv`),
  `TEST/compare/normalize.py` (already handles VDB fields incl.
  `BINPKGMD5` — the L2/L3 cross-build case — `environment.bz2`,
  `CONTENTS` mtimes, presence-only caches), `TEST/compare/diff.py`
  (`--layer l0|l1|l2|...` + `--tolerate-payload`; typed findings
  MISSING/MODE/OWNER/XATTR/SIZE/CONTENT/SYMLINK/VDB/CONTENTS/MTIME),
  `TEST/compare/known-divergences.yaml` (layer-keyed allowlist with
  `owner:`), `TEST/compare/normalize.md` (the human spec).
- **Atomlists**: `l0-resolve.txt` (`:161-163` has the `@system`/`@world`
  set entries), `l1-merge.txt` (10 real atoms, the natural `l3-core`),
  `l1-smoke.txt`, `l1-porttest.txt`.
- **L0 already proves resolution parity for the whole-graph sets**:
  `layers/l0/in-container.sh` runs `emerge -pe @system` etc. under both
  PMs; the latest clean L0 is 96/120 with parity 0.800
  (`agent-context.md:146-151`). L3 does **not** re-test resolution; it
  tests building+merging the resolved graph.
- **Portuale's real source path**: `emerge <atom>` (non-`--pretend`) →
  `pretend::run` → `emerge_build::run_source_merge`
  (`pretend.rs:11850`, `emerge_build.rs:205`) with the `-jN` scheduler,
  `--load-average`, build-log capture, `--keep-going`, `--resume`
  (mtimedb), `ebuild_phases` over real `bin/*.sh` (default `bash`).
  `--emptytree/-e` is parsed and honored in resolution
  (`pretend.rs:8739-8740`, `:9499`, `:10445`).
- **Determinism controls** as a spec: `docs/real-world-testing.md:26-51`
  (`SOURCE_DATE_EPOCH=1740000000`, `MAKEOPTS=-j1`, `EMERGE_DEFAULT_OPTS=""`,
  `LC_ALL`/`TZ`/`umask`, same repo commits, never `--sync`, same uid
  mapping).
- **Container image**: `TEST/create-container.bash` — rootful podman,
  pinned gentoo + buildovl + porttest repos, `systemd`/amd64 stage3
  profile, base portage upgraded to `3.0.82.2` by the L0/L1/L2 runs.
- **Host caches already on disk**: `TEST/logs/_l2-distfiles/` (share it:
  both PMs must fetch identical distfiles) and
  `TEST/logs/_l2-pkgcache-*` (not used by L3 — L3 builds from source).

### 1.2 What is missing

- `TEST/run/l3-source-parity.sh` (orchestrator), `TEST/layers/l3/*`
  (the per-PM in-container build+merge driver), `l3-smoke.txt` /
  `l3-core.txt` / `l3-system.txt` atomlists, the `l3-report.{txt,json}`
  + symlink convention.
- A **portage-vs-portage control run** (the noise floor) with its
  recorded noise set.
- `SOURCE_DATE_EPOCH` plumbing on the portuale side: `grep -rn
  SOURCE_DATE_EPOCH rust/ python/ tests/` → **zero matches**. It is not
  in `ENVIRON_WHITELIST` (`ebuild_phases.rs:1387-1541`), so a
  process-env export is dropped before the phases run. Real portage
  exports it via `config.environ()` (`config.py:3263`) because it is a
  config key; portuale must do the same.
- `TEST/findings/l3.md` and the first `TEST/logs/metrics/<date>-l3.json`
  (§8 of the design says L3 owns the metrics/trend start).
- The producer fixes themselves: #37/#38 (and the VDB half of #39) —
  shared with Tier 5, gated at G0.5.

### 1.3 L3's boundary against L0/L1/L2/L5 (do not duplicate)

- **L0** = `--pretend` resolution at real-tree scale. L3's job is not
  resolution: if a source build exposes a resolver bug, that finding
  goes to L0's pipeline/backlog, not to an L3 allowlist.
- **L1** = both PMs merge *one identical prebuilt binpkg set*. L3 does
  not use `$PKGDIR` at all (no `-k`, no `-K`, no gpkg) — the whole point
  is that each PM builds its own bytes.
- **L2** = portuale as *archive producer* + cross-install. If an L3
  finding is archive-shaped (`Build-Info`, gpkg metadata, Manifest), it
  belongs to L2/#38/#39, not L3.
- **L5** owns unmerge/depclean/preserved-libs/CONFIG_PROTECT/fault
  injection. L3 may incidentally cover the env-update outputs the merge
  writes (L1 already snapshots them), but no lifecycle probes.
- **payload bytes are never the gate.** L3's gate is
  VDB + structure (paths/types/modes/owners/symlinks/xattrs); compiled
  `CONTENT`/`SIZE` diffs are tolerated exactly as L2 does
  (`diff.py --tolerate-payload`), with the noise floor as evidence.

### 1.4 Traps found while scoping (encode these, don't rediscover them)

1. **`emerge @system` alone is a no-op** — the image already has
   `@system` installed. Both PMs must be forced into a real source
   rebuild with `--emptytree` (`-e`) plus `--usepkg=n`. Verify in S0
   that portuale's *real* path honors `-e` end-to-end (it is parsed and
   fed to the resolver; the build path consumes the resolved entries).
   If it does not, that is the first producer finding.
2. **`--jobs=1` + `MAKEOPTS=-j1` are the v1 determinism story** (design
   §5/§2). `@system` is hours serially; an `emerge --jobs=N` axis is a
   follow-on, never a way to "speed up the first run".
3. **Process-env vs resolved-config asymmetry is the blocker.** Real
   portage exports `config.environ()` (make.conf/profile/env.d values,
   including `SOURCE_DATE_EPOCH`, `MAKEOPTS`, `FEATURES`, `USE`).
   Portuale's phase env is a **curated whitelist** plus a few resolved
   scalars (`ebuild_phases.rs:1387-1541`, `phase_features_value()` read
   straight from the process env — `:1581`). Until #37 lands, an
   env-var-based determinism block is **not** honored symmetrically.
   Recommendation: put the determinism block in the *resolved config*
   (append it to `/etc/portage/make.conf` inside the container), so both
   PMs must read the same config — which is exactly #37's contract.
4. **`FEATURES` must be pinned to the execution model both PMs share.**
   The image's `make.conf` carries the stage3 `FEATURES` set including
   `userpriv`/`usersandbox`/`userfetch`/`usersync`, which are portuale
   **non-goals** (`scope-backlog.md` Part 3 / §D): real portage would run
   builds as the `portage` user while portuale runs them as root —
   a same-config, different-execution divergence that would show up as
   ownership/generated-file noise in every run. L3's block must disable
   them explicitly (`-userpriv -usersandbox -userfetch -usersync`), keep
   the portuale-implemented isolation tokens (`sandbox`, `pid-sandbox`),
   keep `splitdebug` (so #38 stays under test), and keep `test` off
   (src_test only runs under `FEATURES=test`; enabling it is its own
   axis). Document the final block in `TEST/findings/l3.md` and in
   `normalize.md` if it needs a note. Real portage's
   `config.environ()` is the reference for what "same env" means.
5. **`sys-apps/portage` and `dev-lang/python` are inside `@system`** —
   the run rebuilds the reference portage (and the interpreter) while
   running. That is normal `emerge -e @system` behaviour; compare the
   *installed state*, never the running process.
6. **`SOURCE_DATE_EPOCH` does not neutralize compiler nondeterminism
   across PMs.** Two independent full source builds differ in bytes
   (debug paths, build order, toolchain state). Structure/metadata stay
   hard; payload stays tolerated.
7. **A partial run is not a parity run.** If either PM fails mid-run,
   snapshot nothing as a candidate; capture the merge log + the merged
   list for triage, mark the report `partial`, and do not diff as if it
   were complete. (An in-place `-e @system` that dies at glibc leaves a
   container that is no longer a valid comparison target.)
8. **`@world` in the image is empty** (L0 finding H seeded it for
   pretend only) — a real `@world` build needs a real world file; that
   is the follow-on soak's problem.
9. **`l2-no-md5-cache-ebuild-fallback` (#41) is invisible on the real
   tree**; do not let any L3 atomlist need a cache-less repo.
10. **`BINPKGMD5` is absent on source merges** (L1/L2 added the
   normalization for archives); `normalize.py` already blanks it —
   verify on the first L3 run rather than assume.
11. **Temporary L2 allowlist entries are debt.** L3 runs on the same
    `known-divergences.yaml`; each `l2-*` entry must be deleted the
    moment its gap is fixed, and the L3 report should print
    allowlist entries that matched nothing as removal candidates
    (`docs/real-world-testing.md:197-206`). While #37/#38 are open,
    expect an `l3-*` entry per *filed* gap only — never for an
    unexplained structural diff.

---

## 2. Rules and invariants for every slice

1. **Never weaken L0/L1/L2.** `diff.py`/`normalize.py`/`snapshot.sh`
   default behaviour must stay byte-identical; L3 is opt-in via
   `--layer l3`. After any compare-stack change, re-run
   `TEST/run/l1-merge-from-binpkg.sh TEST/atomlists/l1-porttest.txt`
   and `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`
   and require 0 unexplained.
2. **No silent allowlisting.** Every hard finding is triaged into
   exactly one of: portuale bug (fix it), portage bug (entry with
   `ticket:`), environmental (entry with `reason:` and `owner:`).
   Structural findings (path/type/mode/owner/xattr/symlink/missing) are
   never allowlisted. `compiler-nondeterminism` as a blanket reason is
   forbidden — the S2 noise set is the evidence.
3. **Same-run comparisons only.** The portage-vs-portage control and the
   candidate/portage pair for a green verdict come from the same
   orchestrator invocation (or a recorded control run in the same
   environment, for the `@system` gate).
4. **Real-execution only.** Portuale fixes here are Rust + a Rust
   end-to-end/unit test in the relevant crate. No
   `emerge_pretend_reference.py` mirror, no contract `CASES` entry
   (AGENTS step 4's own carve-out, same as L2).
5. **Bounded scope.** Touch `TEST/` and, only for triaged producer bugs,
   `rust/`. Do not refactor the resolver/merge paths "while here"; do
   not touch `--solver=`, brush, or mrg.
6. **Determinism block is part of the interface.** A run that skips a
   control in `docs/real-world-testing.md:26-51` is not comparable and
   must be labeled invalid, not "red".
7. **Heavy-run discipline.** Container builds are not part of every
   slice's pytest/cargo pass (AGENTS step 8). S1/S2 must be usable
   host-side or on `l3-smoke`; full `@system` runs are explicit,
   time-boxed, and never run in parallel with each other on the host.
8. **Never paper over a failure** with `--usepkg`, `-k`, a
   `FEATURES=-buildpkg`, or a smaller atomlist. The set is the test.
9. **Capture evidence, not just conclusions.** Every finding in
   `TEST/findings/l3.md` carries the exact command, expected (real
   `emerge`, same image) vs actual (portuale), root cause, and
   fix-ref/backlog id. Commands/results go in the file, not the chat.
10. **Default shell backend stays `bash`.** `--shell brush` is a
    follow-on axis; an `@system` run on brush is out of v1.

---

## 3. Gates (owner decisions)

- **G0.1 Target ladder and v1 DoD.** Recommendation: `l3-smoke` (fast
  iteration) → `l3-core` (= the `l1-merge.txt` closure, the first green
  gate) → `@system` (the real gate; "green" if #37/#38 have landed). v1
  DoD: harness shipped + noise floor pinned + `l3-core` green +
  `@system` run with every finding fixed/filed; `@system` green is the
  stretch goal. `@world` is explicitly a follow-on. Owner: user.
  **Status 2026-09-14: taken — `l3-smoke`/`l3-core`/`l3-system` lists
  exist; `l3-smoke` (6 ebuilds with `-e`) is the iteration gate.**
- **G0.2 Mechanism.** Recommendation: two fresh containers, one per PM,
  each runs the *same* `emerge --emptytree --oneshot --usepkg=n` on the
  same set, then each container is snapshotted. Not `--root`, not a
  shared root, not a binpkg round trip (that is L1/L2). Rationale: it
  matches the L1 `consume` pattern, avoids portuale's multi-root
  approximation (`scope-backlog.md` §A `--root-deps` entry), and makes
  each PM's result a complete installed-system state. S0 supplies the
  data (does `-e` really rebuild under portuale?). Owner: user.
  **Status 2026-09-14: taken — `layers/l3/build-and-merge.sh` runs
  `--emptytree --oneshot --usepkg=n` per PM in a fresh container;
  portuale's `-e` resolves and rebuilds (it found the sub-slot
  self-collision on its first run).**
- **G0.3 Comparison scope.** Recommendation: **full-tree + full-VDB**
  snapshot. Both containers start from the same image, so untouched
  files are identical; a file one PM merged and the other did not shows
  up as MISSING (correct), and no "what did it rebuild" reconstruction
  is needed. `snapshot.sh` already prunes `/proc`, `/sys`, `/dev`,
  `/run`, `/tmp`, `/var/tmp`, `/var/log`, `/var/db/pkg` (VDB is the tar)
  and the repo trees. If full-tree noise proves unmanageable on
  `@system`, fall back to the L1 restricted mode (rebuild set's
  CONTENTS + config-protect/env-update targets) as a documented
  narrowing. Owner: user. (S2 decides with data; either way notify.)
  **Status 2026-09-14: taken, full-tree — `snapshot.sh` gained a
  `SNAPSHOT_PRUNE` hook for the repo bind mount and always prunes
  `/TEST`/`/distfiles`; the L3 containers share `--hostname
  porttest-l3`. The smoke control pair is 0 unexplained in every
  category, no per-path rule needed.**
- **G0.4 Payload tolerance and the noise floor.** Recommendation:
  structural/VDB always hard; `--tolerate-payload` in every L3 diff;
  the **control pair is two portage runs**, whose diff must be 0
  unexplained before any portuale comparison counts. Payload paths that
  differ portage-vs-portage are the recorded nondeterminism set; a
  portuale-vs-portage payload diff *outside* that set is still reported
  and must be explained (never a blanket allowlist). Owner: user.
  **Status 2026-09-14: implemented — `diff.py --layer l3
  --tolerate-payload` runs both pairs; the control pair is the
  noise-floor gate (`rc=1` when it is dirty). The smoke control has 0
  payload diffs; the candidate payload set is the recorded
  nondeterminism set (two portage builds agree exactly at this
  scale).**
- **G0.5 Where #37/#38 live.** Recommendation: execute them as their own
  backlog items (Tier 5) with their own commits; S3 is then a
  *verification* pass (re-run L2 client track; confirm the
  `known-divergences` entries close). If the user prefers one campaign,
  S3 owns them under this plan. Owner: user. **Status 2026-09-13: taken
  as recommended — #37 ran as its own Tier-5 item (S0–S5,
  `docs/037_Build-phase-env-completeness.plan.md`) and is closed; S3
  here is now verification-mode for the env half. #38 has landed
  (S0-S5, `docs/038_Packaging-transforms.plan.md`, 2026-09-13) — the
  transforms and merge-time `instprep` are parity-proven on the L2
  porttest track, so S3 here is verification-mode for both halves.**
- **G0.6 `SOURCE_DATE_EPOCH` plumbing.** Recommendation: fix inside
  #37's resolved-env work (config key → phase env) plus a unit test
  pinning it; no separate user-facing flag. The harness puts it in
  `/etc/portage/make.conf` so the fix is exercised through the config
  path, not a special case. Owner: user (asked before S3). **Status
  2026-09-13: done in #37 S1 — `portage_profile::phase_environ` exports
  the `make.conf`/`make.globals` scalar as an ordinary config key, and
  `phase_environ_exports_the_profile_family_and_folded_incrementals`
  (`rust/portage-profile/src/phase_environ.rs`) pins
  `SOURCE_DATE_EPOCH` from a seeded `make.conf`. No separate flag.**
- **G0.7 Allowlist ownership.** A human adjudicates every new entry;
  agents propose with evidence (the two snapshots + the exact commands).
  `owner:` is mandatory. Owner: user.

---

## 4. Slices

### S0 — Recon spike: can portuale build+merge a real package in-place? (F, 3–6 h)

**Goal:** de-risk the mechanism and the gate decisions with data before
any harness exists. Decides G0.1–G0.4.

Steps:

1. Control: `TEST/run/l1-merge-from-binpkg.sh TEST/atomlists/l1-smoke.txt`
   → must be green.
2. Pick the smallest real target, e.g. `app-text/tree` (+ its closure
   via `--emptytree`). In two fresh containers (one portage, one
   portuale — mirror `podman_run_portuale` from `TEST/run/lib.sh:23`),
   stage the determinism block into `/etc/portage/make.conf`:
   ```
   MAKEOPTS="-j1"
   SOURCE_DATE_EPOCH=1740000000
   EMERGE_DEFAULT_OPTS=""
   # pin FEATURES to the model both PMs implement (see §1.4 trap 4):
   # keep sandbox pid-sandbox xattr filecaps splitdebug; drop the
   # non-goals (-userpriv -usersandbox -userfetch -usersync) and
   # -buildpkg -sign -ccache -distcc -cgroup; test off.
   FEATURES="-buildpkg -sign -ccache -distcc -cgroup \
             -userpriv -usersandbox -userfetch -usersync \
             sandbox pid-sandbox xattr filecaps splitdebug"
   # LC_ALL/TZ/umask via export as in consume.sh:29-35
   ```
   and run `emerge --emptytree --oneshot --usepkg=n --color=n app-text/tree`.
3. Snapshot both containers (`snapshot.sh / <prefix>`, full-tree) and
   diff (`normalize.py` + `diff.py --layer l3 --tolerate-payload`) by
   hand; inspect every finding.
4. Time the run and record `emerge -p --emptytree @system | wc -l`
   (set size) under both PMs; this calibrates S4/S5.
5. Write `TEST/findings/l3.md` (new): recipes, raw output, the archive
   of the spike, the verdicts on G0.1–G0.4.

**Acceptance:** at least one real package is built+merged from source by
*both* PMs in fresh containers, snapshotted, diffed, and every
difference explained; the `@system` set size and a per-run time estimate
are recorded.

**Stop rule:** if portuale cannot build+merge a dep-free real package at
all, or if `-e` is not honored on the real path: file each finding with
a repro, do not build harness code on a broken foundation, and schedule
the producer fix as its own slice (likely #37). This is the
frontier-model gate that makes S1/S2 safe to delegate.

### S1 — The harness: orchestrator + layers + atomlists + report (M, 6–12 h)

**Goal:** the runnable L3 bed, proven end-to-end on `l3-smoke` (it may
be red — that is expected and useful). No producer fixes here.

New files:

- `TEST/layers/l3/build-and-merge.sh <portage|portuale> <atomlist> <out-prefix>`
  — the in-container half. Contract: stage the determinism block (§S0's
  `make.conf` snippet, the FEATURES pin included) and export the
  process-env equivalents for real portage's benefit, documenting that
  #37 makes the config path authoritative for portuale; run
  `$EM --emptytree --oneshot --usepkg=n --color=n <atoms>`; tee the
  merge log to `<out-prefix>.merge.log`; on rc != 0 write
  `<out-prefix>.partial` and still capture the merged list; on rc == 0
  snapshot full-tree + full-VDB via
  `/TEST/compare/snapshot.sh / <out-prefix>`, plus `meta.tsv`
  (pm, portage/portuale version, repos' git SHAs, profile, date) — the
  L1 `consume.sh:140-146` shape. Appending to `/etc/portage/make.conf`
  is per-container and idempotent.
- `TEST/run/l3-source-parity.sh [atomlist]` — the host orchestrator.
  ```
  Env: L3_PM=both|portage|portuale     (default both; one-side iteration)
       L3_CONTROL=1                     (run the portage pair too)
       L3_BUILD_ARGS="--emptytree --oneshot --usepkg=n --color=n"
       L3_DISTFILES (default $LOGS_DIR/_l2-distfiles)
       L3_TMPDIR    (default $LOGS_DIR/_l3-portage-tmp-<pm>, wiped unless L3_KEEP_TMP=1)
       L3_TIMEOUT   (default 28800)
       L3_SKIP_PORTAGE_UPGRADE=0        (same semantics as L1)
  Flow:
    1. ensure_portuale_built; ensure_image
    2. per PM: podman run <image> /TEST/layers/l3/build-and-merge.sh ... \
         -v $L3_DISTFILES:/distfiles -v $L3_TMPDIR-$pm:/var/tmp/portage
    3. if L3_CONTROL=1: portage-a vs portage-b (the noise floor)
    4. normalize each prefix; diff with --layer l3 --tolerate-payload
    5. write l3-report.{txt,json} + ln -sfn l3-report.txt; emit metrics
    6. classify findings: MISSING/MODE/OWNER/XATTR/SYMLINK/VDB/CONTENTS
       unexplained => rc 1; [PAYLOAD] counted, never fatal
  ```
- `TEST/atomlists/l3-smoke.txt` (`app-text/tree`, `sys-apps/pv`,
  `sys-apps/dmidecode` — small, real, dependency-light),
  `l3-core.txt` (= `l1-merge.txt`'s ten atoms), `l3-system.txt`
  (literal `@system`).
- Report dir: `TEST/logs/l3-<ts>/{portage,portuale,control-a,control-b}/`,
  `l3-report.{txt,json}`, symlink `TEST/logs/l3-report.txt`.

**Acceptance:** `TEST/run/l3-source-parity.sh TEST/atomlists/l3-smoke.txt`
runs end-to-end on both PMs (rc 0 or findings, both acceptable) and
produces the typed report + metrics; `L3_PM=portage` alone works; L0/L1/L2
are untouched/default-clean.

### S2 — Noise floor: portage-vs-portage must be 0 unexplained (M, F review, 4–8 h)

**Goal:** establish that the *instrument* is clean before grading
portuale. Everything unexplained in a portage-vs-portage run is harness
noise; fix it in the harness or normalize it with a documented rule.

Steps:

1. `L3_CONTROL=1 TEST/run/l3-source-parity.sh TEST/atomlists/l3-smoke.txt`
   until the control pair is 0 unexplained.
2. For every residual class, decide:
   - harness bug (e.g. different `DISTDIR`, different `FEATURES`,
     container identity files) → fix the harness;
   - legitimate volatility → a `normalize.py` rule + a
     `normalize.md` line + a pinned case in a host test
     (`TEST/compare/test-normalize-l3.py` or the existing tolerance
     test). Expected candidates to investigate: `/etc/machine-id`,
     `/etc/hostname`, `/etc/resolv.conf`, `/etc/ssh/ssh_host_*`,
     `/etc/ld.so.cache` (already presence-only), `/usr/share/info/dir`
     (already), `/etc/ca-certificates.*`, anything under `/var/lib/`
     the merge regenerates.
3. Record the **payload nondeterminism set** for the smoke/core set:
   paths whose `CONTENT`/`SIZE` differ portage-vs-portage. This is the
   S4 discriminator (two-Portage-builds evidence, never a blanket
   allowlist).
4. Re-run the L1 porttest pair to prove the new rules didn't change L1.

**Acceptance:** portage-vs-portage `l3-smoke` (and, if cheap, `l3-core`)
0 unexplained; noise set recorded in `TEST/findings/l3.md`; L1/L2 still
green; `normalize.md` and `normalize.py` in sync.

### S3 — Producer prerequisites: #37 resolved env + #38 packaging transforms (F, 16–30 h; shared with Tier 5)

**Goal:** close the gaps that make every L3 run red by construction.
**Skip to verification mode if #37/#38 already landed** (G0.5).
**Status 2026-09-13: #37 has landed** (S0–S5, see G0.5/G0.6 above) —
step 1 below is now a *verification* pass: the resolved env is threaded
and the L2 porttest track is green with the `l2-bpkgonly-env` allowlist
gone (`TEST/findings/l2.md` S4/S5). **#38 has since landed too**
(S0-S5, `docs/038_Packaging-transforms.plan.md`; `TEST/findings/l2.md`
"#38 S0-S5"), so step 2 below is now a *verification* pass as well:
the transforms' `l2-gpkg-dostrip-splitdebug`/`-docompress` allowlists
are gone and `L2_REBUILD=1 … l1-porttest.txt` is green.

Workstreams (details live in the findings, this is the L3-facing
contract):

1. **#37 build-phase env** (`rust/portuale/src/emerge_build.rs`,
   `ebuild_phases.rs`): thread the resolved config environment
   (`Config::environ()`-equivalent) into the phase env instead of the
   curated whitelist — implicit profile USE (`amd64`, `elibc_glibc`,
   `kernel_linux`), resolved `FEATURES`, `SLOT`,
   `PORTAGE_REPO_NAME`/`REPO_REVISIONS`, multilib
   (`MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*`), and `SOURCE_DATE_EPOCH`
   (G0.6). Both `emerge <atom>` and `--buildpkgonly` paths
   (`run_buildpkgonly` currently threads no `build_env` at all).
   Rust e2e test: build a real fixture with the resolved env and assert
   `get_libdir`/USE/FEATURES in the saved `environment.bz2`.
2. **#38 packaging transforms** (dostrip/estrip + splitdebug +
   ecompress): portuale's install/package path must run the same
   transforms real portage's `prepstrip`/`ecompress` do, so merged
   binaries are stripped and docs compressed identically. This is L3-
   visible (`/usr/lib/debug/**`, `BIG.txt` vs `BIG.txt.bz2`), not just
   L2-archive-visible.
3. Re-run `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`
   and delete each `known-divergences.yaml` entry whose finding is now
   a plain pass; the L2 orchestrator's `KNOWN_FINDINGS` list shrinks
   correspondingly.
4. L3 sanity: re-run `l3-smoke` and confirm the #37/#38 finding classes
   disappear.

**Acceptance:** the L2 real set builds further than jq; the L2 fixture
track's allowlist entries for #37/#38 are gone; `l3-smoke`'s findings
are no longer the env/packaging classes.

**Stop rule:** if more than three distinct systemic producer bugs block
the build after #37/#38, STOP after filing them with repros, do not
land L3 as green, and escalate the ordering to the user.

### S4 — `l3-core` bring-up: the first green gate (F, 16–30 h)

**Goal:** both PMs build+merge the ten-atom `l1-core` closure from
source in fresh containers; VDB + structure parity with the portage
reference; every finding fixed or filed.

Steps:

1. `TEST/run/l3-source-parity.sh TEST/atomlists/l3-core.txt` with
   `L3_CONTROL=1`; triage every hard finding by the §3 table of
   `docs/real-world-testing.md:53-71`:
   - `$ROOT` only, VDB agrees → merge-path bug;
   - VDB only → metadata-capture bug;
   - both, same package → phase behaviour (preinst/postinst/collision);
   - only after the run → state leak.
2. For each portuale bug: minimal repro in `TEST/findings/l3.md`, fix in
   `rust/` + Rust e2e test, re-run. For each gap already in
   `scope-backlog.md`, file with the repro and continue if not blocking.
3. Payload diffs: compare the run's portuale-vs-portage payload set
   against S2's portage-vs-portage noise set; anything outside it is a
   finding until explained.
4. Record the per-package verdict table in `TEST/findings/l3.md`.

**Acceptance:** `l3-core` 0 unexplained hard findings; control pair 0;
L1/L2 still green; every residue adjudicated with `layer: l3` entries
(agent-proposed, user-approved, never structural).

**Stop rule:** time-box; if a systemic class (e.g. a whole eclass family
fails) appears beyond three distinct bugs, stop after filing, propose
ordering, escalate.

### S5 — `@system` gate + metrics (F, 8–20 h + 2–6 h wall-clock per run)

**Goal:** the layer's real gate — portuale builds and merges `@system`
from source, and the installed system matches real portage's modulo
tolerated payload diffs.

Steps:

1. `L3_PM=portage` first (reference + control), record wall-clock and
   the portage-vs-portage noise set at `@system` scale (one control run
   is mandatory: full-tree diff at scale is where stale normalize rules
   show up).
2. `TEST/run/l3-source-parity.sh TEST/atomlists/l3-system.txt` on both
   PMs; L3_TIMEOUT generous; `L3_KEEP_TMP=1` so a failure leaves
   `/var/tmp/portage` logs on the host for triage.
3. Triage every hard finding. Prefer fixing; file with repro + backlog
   id when the fix is its own slice. **No structural allowlist entry**,
   ever.
4. Emit `TEST/logs/metrics/<date>-l3.json` per
   `docs/real-world-testing.md:197-206`: `parity_rate` (packages with
   zero unexplained diff / total), divergences by category and package,
   `allowlist_hits` + entries that matched nothing (removal
   candidates), wall time per PM, merge-list length.
5. Record the **eclass coverage ledger** in `TEST/findings/l3.md`:
   which eclasses the run executed, which failed, which were never
   reached. That is the map for the next producer slice.

**Acceptance:** `@system` run completes on both sides; 0 unexplained
hard findings (or every residue fixed/filed and the user accepted the
filed set as the v1 close); metrics file written; `@system` green if
#37/#38 landed.

**Stop rule:** if `@system` surfaces a systemic class (toolchain,
python-r1, multilib), stop at the first safe checkpoint, file the class
with repros, and do not let the run consume more than the time-box. A
red `@system` with precise findings is a valid S5 outcome; a green one
produced by allowlisting structural diffs is not.

### S6 — Docs closeout + follow-on filing (S/M, 2–4 h)

- `TEST/README.md`: replace the `L3+ (not yet implemented)` stanza
  (`:143-149`) with the L3 runbook (commands, env, outputs, host vs
  container split, the control pair).
- `docs/real-world-testing.md` §5 L3: mark shipped with the live
  evidence pointer; §7 risks updated (an `@system` wall-clock note; the
  `@world` soak stays open).
- `docs/scope-backlog.md` §I: L3 bullet closed/updated with the real
  status; §K updated as #37/#38 close.
- `docs/backlog-tasks.md:56` #30 → status with a one-line pointer;
  Tier 5 entries updated/deleted as their gaps close.
- `docs/what-this-proves.md`: append one slice paragraph with a
  runnable, live-verified example (AGENTS step 7).
- `TEST/findings/l3.md`: final verdict table + eclass ledger.
- File the `@world` soak as its own task if not already in §I.

---

## 5. Routing summary

| Slice | Tier | Effort | Depends on | Deliverable |
|---|---|---|---|---|
| S0 | **F** | 3–6 h | image present | findings + G0.1–G0.4 data |
| S1 | M | 6–12 h | S0 mechanism | `layers/l3/*`, `l3-source-parity.sh`, atomlists, report |
| S2 | M (F review) | 4–8 h | S1 | 0-unexplained control + noise set + normalize pins |
| S3 | **F** | 16–30 h | S2 (instrument) | #37/#38 closed (or verified), L2 entries deleted |
| S4 | **F** | 16–30 h | S2, S3 | `l3-core` green |
| S5 | **F** | 8–20 h + wall clock | S4 | `@system` run, metrics, eclass ledger |
| S6 | S/M | 2–4 h | all | docs + verdict |

Total: **~55–110 agent-hours, 7–12 sittings**, dominated by S3–S5 and by
container wall-clock. Cheap models can do S1+S2 and stop; everything
from S3 on wants a frontier model, and S0 must be frontier by design.

---

## 6. Definition of done

- [x] S0 recorded; G0.1–G0.4 answered with data.
- [x] `TEST/run/l3-source-parity.sh TEST/atomlists/l3-smoke.txt` runs
      end-to-end; report + metrics emitted; `L3_PM` one-side mode works.
- [x] portage-vs-portage control is 0 unexplained on smoke.
- [ ] `l3-core` is 0 unexplained hard findings. *(not started: S4 stop
      rule — two filed systemic classes, `l3-merge-owner-1-1` and
      `l3-merge-vdb-env-accumulates`)*
- [ ] `@system` has been run on both PMs; every hard finding is
      fixed or filed with a repro, an `owner:` and (if adjudicated) a
      `layer: l3` entry; metrics + eclass ledger written. *(not
      started, same stop rule)*
- [x] No structural finding is allowlisted (none added); the `l2-*`
      entries were deleted as their gaps closed.
- [x] Full verification pass (AGENTS step 8) still green: `cargo fmt
      --check`, clippy 0 warnings, `cargo test --release` (1036 passed),
      `python3 -m pytest tests -q` (1568 passed).
- [x] L1/L2 porttest runs still green after compare-stack changes
      (`snapshot.sh` gains only the full-tree prune/FIND_ROOT paths).
- [~] Docs updated (S6 partial — this file's status/DoD, backlog
      #30 DONE-PARTIAL, §I L3 bullet, `what-this-proves.md`,
      `agent-context.md`); findings live in `TEST/findings/l3.md`.

## 7. Review checklist (attach to each slice)

- Does the change keep L0/L1/L2 defaults bit-identical in behaviour?
- Is every new finding class justified against the triage categories in
  `docs/real-world-testing.md:53-71`?
- Any new allowlist entry: evidence, `reason:`, `owner:`, `layer: l3`?
  Does the entry encode a *structural* diff? If yes it is invalid.
- Was the run valid — determinism block complete, same-run control,
  repos pinned, no `--sync`, no hidden flags?
- If a portuale fix landed: Rust e2e/unit test added, no Python mirror,
  no resolver/merge refactor smuggled in?
- Are commands/results appended to `TEST/findings/l3.md`, not just in
  the chat?
- Is `normalize.md` still in sync with `normalize.py`?

## 8. Risks

1. **Producer readiness is the unknown.** 13 real packages built, then
   jq died on the env gap; `@system` wants gcc/glibc/python/perl. S3 is
   the fix; S0's stop rule bounds the downside.
2. **Wall-clock.** Serial `-e @system` is hours per PM; iterating on a
   failure costs another full run unless the harness is built to
   snapshot partial state for triage (S1 does). Budget accordingly;
   never "save time" by dropping `-j1`.
3. **Nondeterminism.** Cross-PM payload bytes will differ; S2's noise
   floor is the only acceptable discriminator. A cheap model that
   treats `--tolerate-payload` as "everything is fine" is the failure
   mode to watch.
4. **Full-tree snapshot noise.** First full-tree `@system` diff may be
   loud (container identity, regenerated caches). S2's normalize rules
   plus G0.3's fallback keep this bounded; each rule is documented and
   pinned.
5. **The run rebuilds the toolchain in place.** A failure mid-`@system`
   (especially glibc/gcc) leaves an unusable container — that is why
   each run is a throwaway container and a partial run is never graded.
6. **Fetch at scale.** Hundreds of distfiles, mirrors, `->` renames,
   Manifest digests. Share `_l2-distfiles`; a fetch failure is a
   producer finding, not a reason to shrink the set.
7. **Allowlisting pressure to "go green".** Rule 2 + G0.7 + the review
   checklist; the user adjudicates every entry.
8. **Scope creep into Tier 5.** #37/#38 are prerequisites, not L3's
   deliverable (G0.5); if they are open at S3, land them as their own
   commits.
9. **`sys-apps/portage` inside the set.** The reference rebuilds itself
   mid-run; only an installed-state comparison is meaningful. Note it in
   the report so a future reader does not "fix" it.

## 9. Non-goals

- `@world` soak, musl / no-multilib / hardened profile variants, arm64.
- xpak (`.tbz2`) axis; HTTP binhost; `mrg`/L4; L5 lifecycle and fault
  injection; brush as the shell backend.
- Byte-parity of compiled payloads; a bit-reproducible build claim.
- Any resolver / merge-order / scheduler feature work beyond fixing
  bugs this bed surfaces; `--solver=` backends.
- Rewriting L0/L1/L2 scripts. L3 reuses `snapshot.sh`, `normalize.py`,
  `diff.py`, `known-divergences.yaml`, `lib.sh`, and the L2
  distfiles cache.

## 10. Findings filed while executing

(append here as S0–S5 run; one entry per finding: command, expected,
actual, root cause, fix ref / backlog id)
