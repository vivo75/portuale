# 037 — Build phase env completeness — agent plan (musespark draft)

> **Historical planning/investigation doc, retired 2026-09-15.** Its outcome is `docs/backlog-tasks.md`'s status line for this item; the extracted ground truth, gotchas and dead ends are in [`../recap-of-backlog-ops-2026-09-15.md`](../recap-of-backlog-ops-2026-09-15.md). Kept verbatim below for citation/provenance only.


Status: **not started.** Covers backlog #37 (`docs/backlog-tasks.md:73`),
the `scope-backlog.md` §K `l2-bpkgonly-env` entry (`:659-668`), and the
findings `TEST/findings/l2.md:240-254` (S3) + `:331-359` (S5). Blocks
the L2 real set, L3 S3, #38 (whose strip/compress branches gate on the
resolved `FEATURES` this item threads through), and the VDB half of #39
(`repository`/`REPO_REVISIONS`/`USE` in archive metadata come from this
same env).

**Read first:** `AGENTS.md` (steps 4/5/7/8 — note step 4's
real-execution carve-out, §2 rule 4 below), `docs/agent-context.md`,
`TEST/findings/l2.md` S3+S5, `docs/030_L3-source-build-parity.deepseek.md`
§1.4 trap 3 + G0.5/G0.6 (this item owns both), and the code under test:
`rust/portuale/src/ebuild_phases.rs` (`compute_environment` `:397`,
`phase_env_vars` `:~1900-2137`, `ENVIRON_WHITELIST` `:1387-1541`,
`phase_features_value` `:1581`, `phase_standalone_base_env` `:841`),
`rust/portuale/src/emerge_build.rs` (`run_buildpkgonly` `:118`,
`entry_build_env` `:558`), `rust/portuale/src/pretend.rs`
(`BUILD_VARS` `:5948`, `build_config_env` `:6002`,
`config_features_list` `:5973`). Real-portage semantics:
`3rdparty/portage/lib/portage/package/ebuild/doebuild.py:381`
(`doebuild_environment`) and
`3rdparty/portage/lib/portage/package/ebuild/config.py:3263`
(`config.environ()`).

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

**Do this first of the two Tier-5 items, and do it as plumbing, not as
semantics.** The backlog one-liner ("curated whitelist, not the
resolved config env") is accurate but undersells how much of the
machinery already exists: the standalone `ebuild <file> <phase>` path
*already* loads the resolved config (`phase_standalone_base_env`,
`ebuild_phases.rs:841` — USE via `candidate_use_flags_display`,
compiler flags via `pretend::build_config_env`), and the merge path
*already* has the per-entry override seam (`entry_build_env`,
`emerge_build.rs:558`, applied as `extra_env` after the base vars).
What is missing is (a) the *contents* of what gets threaded (resolved
`FEATURES`, effective USE with the implicit arch part, `SLOT`,
`PORTAGE_REPO_NAME`/`REPO_REVISIONS`, multilib, `SOURCE_DATE_EPOCH`),
and (b) threading *anything at all* on the `--buildpkgonly` path
(`run_buildpkgonly` calls `run_package`, which passes `&[]` as
`extra_env` — `ebuild_package.rs:455-465`). No new architecture is
needed; every fix lands in an existing seam.

Two findings from grounding that narrow the work further:

1. `PORTAGE_REPO_NAME` / `PORTAGE_REPO_REVISIONS` are already in
   `ENVIRON_WHITELIST` (`ebuild_phases.rs:1497-1498`) — they pass
   through *if set*, but portuale never sets them. Real sets them in
   `doebuild_environment` (`doebuild.py:479-483`: repo lookup via
   `mydbapi.repositories.get_repo_for_location(mytree)`). The fix is to
   set them from the already-known candidate repo, not to widen any
   whitelist.
2. `PORTAGE_DOCOMPRESS` / `PORTAGE_DOSTRIP` arrays are **not** config
   values — they are bash-side defaults from `bin/phase-helpers.sh:21-28`,
   already vendored and already sourced by real phase execution. #38
   therefore needs no config work for the path lists, only the
   `FEATURES` + `PORTAGE_COMPRESS` family this item threads. Do not let
   #37 creep into reimplementing bash defaults in Rust.

### 0.2 Frontier or cheap model?

**M core with F review; S0 can be M alone.** The failure mode is
*omission* (a var left unthreaded shows up as an L2 diff — the bed
catches it), not *misclassification* (nothing here asks the agent to
judge noise vs bug). The one semantic knot — effective USE with the
implicit profile part vs the `use_flags_display` enabled-only subset —
wants a frontier eye at review time, because getting it wrong silently
changes `use()` results in every ebuild. Everything else is
plumb-and-verify. If only cheap models are available: ship S1+S2 (the
two merge/build paths), hand S3 (multilib, the real-set unblocker) to a
frontier session only if the L2 real set still fails after S1+S2.

### 0.3 Difficulty, by axis

| Axis | 1-5 | Notes |
|---|---|---|
| Real-source grounding | 2 | `doebuild_environment` + `config.environ()` are read, not reverse-engineered; whitelist already mirrors real's |
| Plumbing (Rust) | 3 | four call sites, one signature change (`run_buildpkgonly`); `extra_env` ordering (base vs override) must be preserved |
| Effective-USE semantics | 3.5 | the single knot: implicit flags + `package.use` + `use.force/mask` must match `PORTAGE_USE`, not `use_flags_display` |
| Multilib | 2.5 | mechanical once threaded (`get_libdir` already reads the vars; they are just absent) |
| Verification | 2 | fully objective: L2 bed + `environment.bz2` diffs; no judgment calls |
| **Overall** | **3** | the cheapest Tier-5 item per unit of unblock; no bash, no toolchain |

---

## 1. Ground truth

### 1.1 What exists (reuse, do not rebuild)

- **Merge-path seam:** `entry_build_env(options, entry)`
  (`emerge_build.rs:558-566`) = run-wide `options.build_env`
  (`pretend.rs::build_config_env`: `BUILD_VARS` only — 11 compiler/make
  flags) + per-package `package.env` build vars + entry `USE` from
  `use_flags_display` (enabled IUSE only). Applied as `extra_env`,
  which overrides the base vars in `phase_env_vars`. All three
  production paths use it: `merge_one_source_entry` (`:500-501`),
  `build_one_source_entry` (`:858`), `merge_one_built_entry` (`:926-927`).
- **Standalone seam:** `phase_standalone_base_env`
  (`ebuild_phases.rs:841`) already does a full config load
  (`find_repos` + `resolve_config` + `candidate_use_flags_display` +
  `build_config_env`) and exports base `USE` + flags — but *only* when
  `extra_env` carries no `USE` (merge builds skip it entirely) and
  never for the `depend` phase (deliberate: cache-byte stability).
- **Resolved FEATURES helper:** `config_features_list` /
  `config_features_string` (`pretend.rs:5973-5990`) already resolve the
  incremental list — used for `MergeOptions::features` and
  `INSTALL_MASK`, but *not* for the phase `FEATURES` var, which still
  reads the raw process env (`phase_features_value`,
  `ebuild_phases.rs:1581-1583`).
- **Whitelist:** `ENVIRON_WHITELIST` (`ebuild_phases.rs:1387-1541`)
  already contains every var this item needs to set (`SLOT` is exported
  by the ebuild bash itself; `PORTAGE_REPO_NAME`/`REPO_REVISIONS`,
  `MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*` pass through if present —
  they are absent because nobody sets them, not because they are
  filtered).
- **L2 oracle:** `TEST/findings/l2.md:240-254` is the field-by-field
  spec (USE / FEATURES / `.keep` SLOT suffix / libdir / environment.bz2),
  and `TEST/run/l2-portuale-builder.sh` + `known-divergences.yaml`
  (`l2-bpkgonly-env`) is the grader.

### 1.2 What is missing (the slice list in §4)

| # | Gap | Real source | Portuale site |
|---|---|---|---|
| 1 | Phase `FEATURES` is raw process env, not resolved incrementals | `config.environ()` exports `mysettings["FEATURES"]` | `phase_features_value` (`ebuild_phases.rs:1581`) |
| 2 | `USE=` lacks the implicit profile part (`amd64`, `elibc_glibc`, `kernel_linux`) | `mydict["USE"] = PORTAGE_USE` (`config.py:3329`) | `build_use_env` (`emerge_build.rs:516`) uses enabled-IUSE-only |
| 3 | No `SLOT` in phase env (`.keep_<cp>-<slot>` degrades to trailing `-`) | `doebuild_environment` via `setcpv` | `entry_build_env` never sets it |
| 4 | No `PORTAGE_REPO_NAME` / `PORTAGE_REPO_REVISIONS` | `doebuild.py:479-483` | never set (whitelisted but absent) |
| 5 | No multilib (`MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*`) → `get_libdir` falls back to `lib` → oniguruma mis-installs → jq configure dies | config `other_vars` | never threaded |
| 6 | `--buildpkgonly` threads no `build_env` at all | real `--buildpkgonly` is the same `doebuild` path | `run_buildpkgonly` → `run_package` → `&[]` |
| 7 | `SOURCE_DATE_EPOCH` dropped (not in whitelist, not set) | config key exported via `environ()` | L3 G0.6 owns the requirement; this item owns the plumbing |

Out of scope (explicitly *not* this item): `PORTAGE_DOCOMPRESS` /
`PORTAGE_DOSTRIP` arrays (bash defaults, #38's problem space, not a gap
at all); full `config.environ()` parity (the whitelist stays — real's
`filter_calling_env` + `environ_filter` are approximated, not ported);
`KV`, `AA`-gating, `ESYSROOT`/`BROOT` phase-gating (`config.py:3331-3356`
— EAPI-edge narrowing, file separately if L3 ever trips on it).

### 1.3 Traps found while scoping (encode these, don't rediscover them)

1. **`extra_env` order is the override chain.** `phase_env_vars`
   pushes base vars first, then `standalone_base_env`, then
   `extra_env` last (`ebuild_phases.rs:2090-2125`). New resolved values
   for the merge path belong in `entry_build_env` (the tail), never in
   the base — or standalone behaviour changes silently.
2. **The `depend` phase gate stays.** `phase_standalone_base_env`
   returns `("", [])` for `depend`, and every `--regen` golden assumes
   it. New vars must not leak into the `depend` phase env.
3. **`USE` has two consumers with different shapes.** Bash `use()` /
   USE-conditionals need the *effective* set (implicit included);
   `Packages`-index / metadata `USE` fields need real's
   `Package.use.enabled` shape (already threaded as `use_flags` in
   `build_one_source_entry`, `emerge_build.rs:891-895` — do not "fix"
   that one with the effective set).
4. **`SLOT` in phase env vs `entry.slot`.** `GraphEntry::slot` is
   `Option`; real always has one post-`setcpv`. Default `None` → `"0"`
   (the existing `entry_matches_any` precedent,
   `emerge_build.rs:295`), never empty string (the `.keep_…-` bug is
   exactly empty-SLOT).
5. **Determinism is a feature here, not a risk.** Resolved config is
   deterministic; raw process env is not. Each slice must move a var
   *from* process env *to* resolved config, never the reverse. The
   full verification pass (AGENTS step 8) plus the L2 fixture track
   (must stay 0 unexplained) is the regression gate.

---

## 2. Rules and invariants for every slice

1. **Real-execution only: Rust + Rust tests, no Python mirror, no
   contract `CASES`.** AGENTS.md step 4's carve-out (same as L2):
   `emerge_pretend_reference.py` mirrors CLI recognition, not phase
   execution. Cover each slice with a Rust unit test in the touched
   crate + a Rust fixture-driven e2e test (`tests/test_portuale.py`
   pattern), and prove it on the L2 bed.
2. **Never weaken L0/L1/L2.** After every slice: `cargo test --release`
   + `python3 -m pytest tests -q` green, and
   `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`
   shows no *new* unexplained finding (the `l2-bpkgonly-env`
   allowlisted findings should shrink, never grow).
3. **No silent behaviour change outside the phase env.** Touch
   `emerge_build.rs`, `ebuild_phases.rs`, `ebuild_package.rs`,
   `pretend.rs` (`build_config_env` family) only. No resolver, merge,
   scheduler, or compare-stack changes.
4. **Every new env var cites its real setter.** `doebuild.py:<line>` or
   `config.py:<line>` in the code comment — the file already follows
   this convention; keep it.
5. **Capture evidence in `TEST/findings/l2.md`, not the chat.**
   Per-var before/after (`environment.bz2` dumps, archive metadata
   rows) with exact commands.

---

## 3. Gates (owner decisions — ask before S1)

- **G0.1 No feature gate.** Recommendation: land ungated (these are
  bug fixes toward real; a `PORTUALE_*` gate like #19's would double
  the L2 verification work for no safety — the bed *is* the safety).
  Owner: user.
- **G0.2 Effective-USE shape.** Recommendation: `PORTAGE_USE`
  (implicit + `package.use` + force/mask, exactly what real exports)
  for the phase `USE=` var; keep `use_flags_display`-derived values
  for index/metadata `USE` fields. Owner: user (asked at S2 review —
  S2 must present the two shapes side by side with a live diff).
- **G0.3 `--buildpkgonly` signature.** Recommendation: pass the
  resolved per-entry env through (new param or an options struct),
  not the whole `Config` — keeps `run_buildpkgonly` testable
  host-side. Owner: user only if it conflicts with the scheduler
  work; otherwise agent's call.

---

## 4. Slices

### S0 — Recon: the var-by-var diff table (M, 2–4 h)

**Goal:** turn `l2-bpkgonly-env`'s five-row table into an exhaustive,
cited spec so S1–S4 have no unknowns.

Steps:

1. On the L2 `porttest` pair (`docs`, `splitdebug`, `phases`): unpack
   both PMs' archives, dump `metadata/{USE,FEATURES,SLOT}`,
   `build-info/` presence, and `environment.bz2` (normalize via
   `TEST/compare/normalize.py`'s `norm_environment` — one ruleset,
   import it, don't copy it). Record every key present in real's env
   but absent/different in portuale's, with the real setter
   (`doebuild.py` / `config.py` / `make.globals` / bash default).
2. Same for the S5 real-set case: oniguruma's `NEEDED` lib path +
   `get_libdir` inputs (`MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*`
   presence in each env).
3. Write the table into `TEST/findings/l2.md` (extend `l2-bpkgonly-env`,
   don't replace it) with columns: var | real value | portuale value |
   real setter | target slice (S1–S4).
4. Confirm §1.2 gaps 1–7 against the table; file anything extra as a
   new finding, don't silently absorb it.

**Acceptance:** the table covers every env-caused L2 diff on the
fixture track; each row points at S1/S2/S3/S4; no code changed.

### S1 — Merge-path env: FEATURES + USE + SLOT + repo identity (M, F review, 6–10 h)

**Goal:** `emerge <atom>` source builds run with the resolved env.
The L2 fixture-track `l2-bpkgonly-env` rows for USE/FEATURES/SLOT go
green (the `--buildpkgonly`-only rows stay — that's S2).

Steps:

1. `FEATURES`: resolve via `config_features_string` (exists,
   `pretend.rs:5985`) into `MergeOptions::build_env` at the
   `pretend.rs:4050` / `:11705` call sites, and switch
   `phase_features_value` to prefer the threaded value (keep the
   process-env read as the standalone fallback — standalone has no
   graph config). Preserve token order (resolved incremental order,
   not sorted).
2. `USE`: extend `build_use_env` (`emerge_build.rs:516`) with the
   effective set — implicit profile flags + `package.use` +
   force/mask — resolved per entry (G0.2). Reuse
   `candidate_use_flags_display`'s `effective_use_flags` path, the
   same one the standalone seam uses; do not invent a second USE
   resolver.
3. `SLOT`: append `("SLOT", entry.slot or "0")` in `entry_build_env`.
4. `PORTAGE_REPO_NAME` / `PORTAGE_REPO_REVISIONS`: set from the
   located candidate's repo (`locate_candidate` already returns it —
   `Candidate.repo_name`?; verify, else thread from `repos`). Revisions
   empty-dict `{}` shape when unknown (match real's file format, not
   just the env var).
5. Rust unit tests: per-var presence/shape on a synthetic entry; e2e:
   build a real fixture (`porttest/phases` prints env — extend its
   phase log or add an assertion target) and assert the resolved
   values in the saved `environment.bz2`.
6. Re-run the L2 fixture track; delete/rescope the fixed
   `known-divergences.yaml` rows.

**Acceptance:** fixture-track USE/FEATURES/SLOT rows byte-match real;
`environment.bz2` diff vs real contains no resolved-config var;
full suite green.

**Stop rule:** if effective-USE resolution disagrees with real on any
fixture (not just missing — *wrong* values), stop after S1-steps-1+3+4,
file the USE shape as its own finding, and escalate G0.2 — do not
ship a wrong USE to fix a missing USE.

### S2 — `--buildpkgonly` threads the graph env (M, 4–8 h)

**Goal:** close the S3 half of `l2-bpkgonly-env` (the table at
`TEST/findings/l2.md:240-254` was captured on `--buildpkgonly`).

Steps:

1. Change `run_buildpkgonly` (`emerge_build.rs:118`) to accept the
   resolved env (G0.3: recommended — a `&MergeOptions`-derived
   `build_env` + per-entry closure reusing `entry_build_env`, not a
   `Config`). Update the `pretend.rs:11750` call site.
2. `run_package` gains an env param (replacing the `&[]` at
   `ebuild_package.rs:455`), forwarded to both `run_commands` (the
   `install` chain) and `package_after_install` (the `USE=` metadata
   half — currently `""` with the documented "no graph, no USE" gap;
   the buildpkgonly path *has* a graph, so it passes the real flags).
3. Standalone `ebuild <file> package` keeps `""`/standalone-base
   behaviour (no graph reaches it — the existing doc comment stays
   true for that path).
4. Same verification as S1, but invoked via `--buildpkgonly`:
   `metadata/{USE,FEATURES}` + `.keep` + `environment.bz2` on the
   fixture track.

**Acceptance:** `--buildpkgonly` and `emerge <atom>` produce identical
env-shaped metadata on the same fixture; the S3 table's portuale
column matches real; L2 fixture track `l2-bpkgonly-env` entries
deleted (or rescoped to S3/S4-only rows with evidence).

### S3 — Multilib: the real-set unblocker (M, F review if S1's USE changed shape, 4–8 h)

**Goal:** `get_libdir` stops falling back to `lib`; oniguruma installs
to `/usr/lib64`; jq's configure finds it. This is the S5 stop-rule
blocker — the single highest-leverage slice in Tier 5.

Steps:

1. Thread `MULTILIB_ABIS`, `DEFAULT_ABI`, and `LIBDIR_<abi>` for each
   listed ABI from `config.other_vars` into `entry_build_env`
   (all three are plain config scalars — no new resolution logic).
2. Verify host-side: unit test asserting the three vars present with
   profile values; fixture e2e if a multilib-aware porttest fixture
   exists (else the container run is the test).
3. Container proof (the actual acceptance): L2 deep-mode build of
   oniguruma + jq's configure past the `Package 'oniguruma' not
   found` death (`TEST/findings/l2.md:310-348` repro commands).
   Run only as far as jq configures — a full real-set build is L2/L3
   business, not this slice's.
4. Confirm `NEEDED` lib path row (`/usr/lib64/…`) matches real.

**Acceptance:** oniguruma lands in `/usr/lib64`, jq configures, the
S5 table's libdir row matches real. If jq dies later on a *different*
env var, that's S4 or a new finding — record and continue, the slice
still closes on the multilib rows.

### S4 — Remainder + `SOURCE_DATE_EPOCH` (M, 3–6 h)

**Goal:** mop up §1.2 gaps 7 + anything S0 filed beyond S1–S3.

Candidates (do only what S0's table justifies; file the rest):

- `SOURCE_DATE_EPOCH` (L3 G0.6): config key → phase env (extend
  `BUILD_VARS` or a dedicated append — note it is *not* in
  `ENVIRON_WHITELIST` today, so add it with the real citation).
  Unit test pins it; the L3 harness exercises it through
  `make.conf`, not a special case.
- `CBUILD` default (`CBUILD` defaults to `CHOST` — check whether
  `resolve_config` already does this; if yes, it rides `BUILD_VARS`
  for free and needs only a test).
- `PORTAGE_FEATURES` export (`config.py:3326` — cheap, do it).
- `MAKEOPTS`/`GNUMAKEFLAGS` fallback (`doebuild.py:645-653` — only if
  S0 shows a real diff; `MAKEOPTS` is already in `BUILD_VARS` when
  configured).

**Acceptance:** S0's table has no unfiled rows; every row is green,
in S1–S3, or a new backlog entry with a repro.

### S5 — L2 real-set re-run + closeout (M, 2–4 h + container wall-clock)

**Goal:** prove the unblock and close the item.

1. Re-run `L2_REBUILD=1 L2_MODE=payload-tolerant L2_BUILD_MODE=deep
   TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-merge.txt` as far
   as the bed goes; triage per L2 rules (new env-class findings →
   fix here; packaging-class → #38; anything else → file, don't
   absorb).
2. Delete every `known-divergences.yaml` + `KNOWN_FINDINGS` entry this
   item fixed. Print entries that matched nothing as removal
   candidates (don't leave dead allowlist).
3. Docs: `scope-backlog.md` §K (`l2-bpkgonly-env` closed with the
   evidence pointer), `backlog-tasks.md` #37 → DONE, `what-this-proves.md`
   one appended paragraph with a runnable live-verified example
   (AGENTS step 7 — e.g. the oniguruma libdir path before/after).
4. Full verification pass (AGENTS step 8).

---

## 5. Routing summary

| Slice | Tier | Effort | Depends on | Deliverable |
|---|---|---|---|---|
| S0 | M | 2–4 h | L2 bed present | var-by-var diff table in `l2.md` |
| S1 | M (F review) | 6–10 h | S0, G0.1–G0.2 | merge-path FEATURES/USE/SLOT/repo |
| S2 | M | 4–8 h | S1 | buildpkgonly threads graph env |
| S3 | M | 4–8 h + container | S1 | multilib; jq configures |
| S4 | M | 3–6 h | S0 | remainder + SOURCE_DATE_EPOCH |
| S5 | M | 2–4 h + wall-clock | S1–S4 | real-set re-run + docs |

Total: **~21–40 agent-hours, 4–6 sittings.** S0–S2 are one sitting;
S3–S4 a second; S5 rides a container run.

---

## 6. Definition of done

- [ ] S0 table complete; every row cited to a real setter and routed.
- [ ] L2 fixture track: all env-caused diffs gone; `l2-bpkgonly-env`
      allowlist entries deleted or rescoped with evidence.
- [ ] Oniguruma → `/usr/lib64`; jq configures (S5 repro commands in
      `l2.md`).
- [ ] `SOURCE_DATE_EPOCH` in phase env from config (L3 G0.6 satisfied
      on portuale's side).
- [ ] Full verification pass green; L0/L1/L2 defaults bit-identical
      apart from the fixed rows.
- [ ] Docs updated (S5); no dead allowlist entries left behind.

## 7. Review checklist (attach to each slice)

- Does the change keep the `depend`-phase env byte-identical?
- Is `extra_env` still the last (winning) layer in `phase_env_vars`?
- Any var moved *from* resolved config *to* process env? (Forbidden —
  direction is always process-env → resolved-config.)
- Index/metadata `USE` fields: still the enabled set, not the
  effective set? (S1 trap 3.)
- New code comment cites `doebuild.py` / `config.py` line numbers?
- Rust unit + e2e tests added; no Python mirror touched?
- `l2.md` evidence appended with exact commands?

## 8. Risks

1. **Effective-USE shape mismatch** (S1 stop rule covers it): wrong USE
   is worse than missing USE. The bed catches it; the rule stops it.
2. **Scope pull from #38/#39.** Their env needs ride this item's
   plumbing — land the plumbing, don't land their features. G0.5 of
   the L3 plan says the same.
3. **`--buildpkgonly` signature churn.** `run_buildpkgonly` has one
   production caller; still, keep the new param narrow (G0.3).
4. **Container wall-clock on S3/S5.** Time-box; a red real-set run
   with precise new findings is a valid outcome, not a failed slice.

## 9. Non-goals

- Full `config.environ()` parity (whitelist stays; `KV`/`AA`/`ESYSROOT`
  gating filed separately if ever observed).
- `PORTAGE_DOCOMPRESS`/`DOSTRIP` arrays (bash defaults — #38's space,
  and not a gap).
- Resolver/merge/scheduler/compare-stack changes of any kind.
- `@system`/L3 work (L3 *verifies* this item in its S3; it doesn't own
  it).
- Python mirror, contract `CASES`, `--json` changes.

## 10. Findings filed while executing

(append here as S0–S5 run; one entry per finding: command, expected,
actual, root cause, fix ref / backlog id)
