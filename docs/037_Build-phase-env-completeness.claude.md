# #37 — Build-phase env completeness — agent plan (claude draft)

Status: **plan only, not executed.** Written 2026-09-13 against `main` @
`ec13936`, every citation below re-read in this checkout (not copied from
the sibling drafts). Backlog: `docs/backlog-tasks.md:73`,
`scope-backlog.md` §K `l2-bpkgonly-env` (`:659-668`). Findings:
`TEST/findings/l2.md` "The producer gaps this run filed" (`:238-282`) and
"l2-bpkgonly-env (scope widened)" (`:331-359`). Sibling drafts:
`037_Build-phase-env-completeness.{deepseek,musespark}.md`; the merged,
canonical plan is `037_Build-phase-env-completeness.plan.md`.

Model tiers (repo convention since 022):

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Opus 5 / Fable 5.1 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Opinion

### 0.1 Verdict — do #37 first; it is one bug with seven symptoms

The finding is not "six missing variables". Real portage builds the phase
env from `config.environ()` (`config.py:3263-3350`): *every* string value
in the resolved config, minus `environ_filter`
(`_config/special_env_vars.py:257`), plus the forced
`PORTAGE_FEATURES`/`USE=PORTAGE_USE` (`:3326-3329`). Portuale builds it
from a hand-curated base (`ebuild_phases::phase_env_vars`, `:1950-2137`)
plus an `extra_env` tail that carries only eleven compiler/make flags
(`pretend.rs::BUILD_VARS` `:5948-5951` → `build_config_env` `:6002`) and
an enabled-IUSE-only `USE` (`emerge_build::build_use_env` `:516-528`).
Everything in the L2 table (`USE` lacking `amd64 elibc_glibc
kernel_linux`; `FEATURES` = raw process env via `phase_features_value`
`:1581`; no `MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*` so `get_libdir` →
`lib`; no `SLOT`; no `PORTAGE_REPO_NAME`/`PORTAGE_REPO_REVISIONS`;
`--buildpkgonly` passing `&[]` at `ebuild_package.rs:455-465`) is one
asymmetry seen from seven angles.

**But the fix is plumbing, not architecture.** Verified: the resolved data
already exists — `portage_profile::Config::other_vars` holds every
scalar including `make.globals` (read `config_root`-relative, `lib.rs:
2245-2258`), the profile chain's `MULTILIB_ABIS`/`DEFAULT_ABI`/
`LIBDIR_amd64`/`ARCH`/`ELIBC`/`KERNEL`, and `PORTAGE_COMPRESS*`;
`Config::resolved_incremental("FEATURES")` (`lib.rs:932`) already stacks
the process env on top of `make.globals` + profile + `make.conf`
(`apply_env_layer` `:1273-1318`), which is exactly why real's list
contains `binpkg-docompress binpkg-dostrip` while portuale's raw-env read
does not. `pretend.rs::config_features_string` (`:5988`) already produces
the right string for `MergeOptions::features`; it just never reaches the
phase env. `entry_build_env` (`emerge_build.rs:558-566`) is the per-entry
seam and is applied last in `phase_env_vars` (`:2125`), so it wins. The
job is to *feed* these seams the real set, and to give `run_buildpkgonly`
(`emerge_build.rs:118`) → `run_package` (`ebuild_package.rs:448`) an env
at all.

### 0.2 Where I disagree with the sibling drafts

- **deepseek G1.1 "export every string value in the resolved config"** is
  the *right end state* but the *wrong first step*. Portuale's
  `other_vars` is not `config.configdict` — it lacks real's `env.d`
  breadth, is missing `pkg`-level values, and contains portuale-only
  keys. Dumping it wholesale risks clobbering computed vars (`HOME`,
  `DISTDIR`, `PORTAGE_TMPDIR`, `ROOT`, …) and changes `environment.bz2`
  unpredictably. **Safest route: a named, cited allowlist that is
  generated from real's `environ_whitelist` (`special_env_vars.py:83`),
  filtered by real's `environ_filter`, minus portuale's computed set** —
  i.e. real's own *whitelisted* set, not real's *everything* set. Real
  itself applies exactly that whitelist whenever `$T/environment` exists
  (`config.py:3275-3301`, every phase after `setup`), so this is not a
  narrowing of real behaviour for `src_*`/`install`; it is real's
  behaviour. Widen to "everything minus filter" only if L3 shows a diff
  it explains (file it then).
- **musespark 0.1 "ABI comes from DEFAULT_ABI in Rust"** — no.
  `config.py` never mentions `ABI`; the vdb oracle shows `ABI="amd64"`
  because the *profile* `make.defaults` sets it and `multilib.eclass`
  reads `${ABI:-${DEFAULT_ABI}}`. It is a plain `other_vars` scalar —
  nothing to compute.
- **Both drafts propose full `USE` = effective set incl. implicit
  flags.** Correct, but note what real exports: `USE = PORTAGE_USE`,
  which is the enabled set **filtered by `IUSE_EFFECTIVE`** (IUSE ∪
  `IUSE_IMPLICIT` ∪ `USE_EXPAND_IMPLICIT`-derived, see `iuse_effective`
  in `portage_profile::Config` `:716`). The oracle value
  `abi_x86_64 amd64 elibc_glibc kernel_linux` for a package with empty
  IUSE is precisely enabled ∩ `IUSE_EFFECTIVE`. Do **not** export the
  raw global enabled set (it would leak `X`, `ssl`, … into a package
  that doesn't declare them). This is the one semantic knot; it gets
  its own gate (G3).
- **musespark "M throughout"** underestimates the USE knot and the
  `environment.bz2` blast radius; **deepseek "F for A/D/E"** is closer.
  My split: F for the allowlist+USE decision (one sitting), M for the
  threading, F for the container triage. See §0.3.

### 0.3 Frontier or cheap model?

| Slice | Content | Tier | Why |
|---|---|---|---|
| S0 | recon table (real vs portuale env, var → setter → slice) | M | mechanical diffing against the oracle; no decisions |
| S1 | `resolved_phase_env()` builder + `USE` shape + unit tests | **F** | the one semantic decision (G2/G3); every later slice consumes it |
| S2 | thread into `emerge <atom>` (+ `--resume`) and `--buildpkgonly` | M (F review) | four call sites, one signature change |
| S3 | resolved `FEATURES` for the Rust-side gates | M (F review) | 13 call sites, but the semantics are "same list, different source" |
| S4 | container proof (porttest track + real set past jq) + allowlist cleanup | **F** | cross-language triage; deciding "env bug vs #38 vs #39 vs new" |
| S5 | docs closeout | S/M | mechanical |

A cheap model *can* ship S0, S2, S3, S5 with S1's builder frozen and the
gates answered. S1 and S4 are not delegable below frontier without
risking a build env that *looks* resolved and is subtly wrong (wrong USE
is worse than missing USE — every `use()` in every ebuild flips).

### 0.4 Difficulty

| Axis | 1-5 | Notes |
|---|---|---|
| Real-source grounding | 2 | `config.environ()` + `doebuild_environment()` are short and read, not inferred |
| Config semantics (USE shape, incremental FEATURES, filter/whitelist) | 4 | the only place to get it *wrong* rather than *incomplete* |
| Rust plumbing | 2 | seams exist; one new param on `run_buildpkgonly`/`run_package` |
| Verification | 3 | objective (L2 bed + `environment.bz2` diff) but container-bound |
| Blast radius | 3 | `environment.bz2` for every source build changes; L1 (prebuilt) must not move |
| **Overall** | **3.5** | small diff, one deep decision, container-gated proof |

Effort: **~18-32 agent-hours, 4-5 sittings** (S0+S1 one sitting; S2+S3
one; S4 rides a container run; S5 short).

---

## 1. Ground truth (verified at `ec13936`)

### 1.1 Real

- `config.environ()` (`config.py:3263-3350`): iterate every `(k, v)` in
  the stacked config; skip `environ_filter`; if `$T/environment` exists
  (all phases after `setup` on a normal build) also skip anything not in
  `environ_whitelist` / `environ_whitelist_re`; then force
  `PORTAGE_FEATURES = FEATURES` (`:3326`), `USE = PORTAGE_USE` (`:3329`),
  pop `AA`/`MERGE_TYPE`/`ESYSROOT`/`BROOT` per EAPI (all EAPI ≥5 here:
  `AA` popped, `MERGE_TYPE` kept, `BROOT` kept for EAPI 7+ — treat
  uniformly per `agent-context.md`'s EAPI-floor rule).
- `environ_whitelist` (`special_env_vars.py:83-250`) contains
  `PORTAGE_COMPRESS`, `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES`,
  `PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS`, `FEATURES`, `USE`,
  `SLOT`, the `MULTILIB`/`LIBDIR` family is *not* in the whitelist by
  name — it reaches the ebuild via the profile → `configdict["defaults"]`
  → `environ()` on the *first* (`setup`) phase, and thereafter through the
  saved `$T/environment`. Portuale re-exports on every phase, so it must
  simply always export them.
- `doebuild_environment()` (`doebuild.py:381-545`): dynamic vars; `SLOT`
  via `setcpv`; `PORTAGE_REPO_NAME` at `:483`
  (`configdict["pkg"]`); `PORTAGE_REPO_REVISIONS` via
  `EbuildPhase._setup_repo_revisions` (`EbuildPhase.py:73-108`, `json
  sort_keys=True`, `{}` when nothing tracked).
- Oracle (`TEST/logs/l1-20260908T170620Z/portage.vdb/pkg/porttest/
  splitdebug-1.0/environment`): `ABI="amd64"`, `ARCH="amd64"`,
  `DEFAULT_ABI="amd64"`, `ELIBC="glibc"`, `KERNEL="linux"`,
  `LIBDIR_amd64="lib64"`, `MULTILIB_ABIS="amd64 x86"`, `SLOT="0"`,
  `PORTAGE_REPO_REVISIONS="{}"`, `USE="abi_x86_64 amd64 elibc_glibc
  kernel_linux"`, `FEATURES`/`PORTAGE_FEATURES` = the resolved sorted list
  (containing `binpkg-docompress binpkg-dostrip`). `PORTAGE_COMPRESS` is
  **absent from the saved env by design** (`save-ebuild-env.sh:117` filters
  it) — do not conclude from the oracle that real doesn't export it.

### 1.2 Portuale

- Base env: `phase_env_vars` (`ebuild_phases.rs:1950-2137`);
  `FEATURES` at `:2043` from `phase_features_value()` (`:1581`, raw
  `std::env`); `USE` at `:2055` from `phase_standalone_base_env`
  (`:841-931`, config-resolved *only* for standalone `ebuild <file>`,
  `("", [])` for `depend` and whenever `extra_env` carries `USE`);
  `extra_env` appended last at `:2125`.
- Bash backend `env_clear()` + whitelist ∩ process env + vars; brush
  backend `phase_setup_script` (`:2139-2190`) `export NAME={value:?}` —
  **not shell-quoted** (documented at `:2139-2147`).
- `ENVIRON_WHITELIST` (`:1387-1541`) already lists `FEATURES`,
  `PORTAGE_COMPRESS*` (`:1469-1471`), `PORTAGE_REPO_NAME`/`_REVISIONS`.
  It is a *calling-env passthrough* filter; it does not set anything.
- Run-wide: `build_config_env` (`pretend.rs:6002-6013`) — `BUILD_VARS`
  only, **skips empty values**; call sites `pretend.rs:4050` (resume) and
  `:11705`. Per-entry: `entry_build_env` (`emerge_build.rs:558-566`) =
  run-wide + `package.env` + `build_use_env`. All three production
  callers: `merge_one_source_entry` (`:500-501`), scheduler
  `build_one_source_entry` (`:858-869`), `run_merge_plan` (`:926-942`).
- `--buildpkgonly`: `pretend.rs:11750` → `run_buildpkgonly`
  (`emerge_build.rs:118-180`, has `repos` + `locate_candidate`, no
  config/env) → `run_package` (`ebuild_package.rs:448-474`,
  `run_commands(..., &[])`, then `package_after_install(..., "")`).
- Rust `FEATURES` gates read `std::env`: `feature_token_present`
  (`ebuild_phases.rs:1550`), `feature_enabled` (`pretend.rs:4132`),
  `distlocks`/`force_mirror` (`ebuild_phases.rs:1097-1106`), plus
  `ebuild_merge.rs` callers. `MergeOptions::features` (`ebuild_merge.rs:
  364`) already carries the resolved list for `PORTAGE_UPDATE_ENV`.
- `portage_profile::Config`: `other_vars` (`lib.rs:911`), `incremental_
  sources` + `resolved_incremental` (`:922-947`, sorted like real),
  `iuse_implicit` (`:701`), `iuse_effective` (`:716`), `use_expand*`
  (`:676-696`); `portage_repo::effective_use_flags` (`:2509`) is the
  single USE resolver, `candidate_use_flags_display` (`:5406`) wraps it
  for IUSE-declared flags.
- Under a fixture `config_root`, `make.globals` is absent → **fixture
  tests must seed `FEATURES`/`PORTAGE_COMPRESS`/multilib vars in the
  fixture `make.conf`/profile explicitly**; they will not appear "for
  free" like on a live host.

---

## 2. Scope

**In:** (1) a cited, real-derived allowlist builder over `Config`;
(2) effective `USE` = enabled ∩ `IUSE_EFFECTIVE`; (3) per-entry `SLOT`,
`PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS`; (4) resolved
`FEATURES`/`PORTAGE_FEATURES` in the phase env; (5) the same on
`--buildpkgonly` and `--resume`; (6) `SOURCE_DATE_EPOCH` as an ordinary
config scalar through the same builder (L3 G0.6); (7) the Rust-side
`FEATURES` gates reading the resolved list where a config is in scope.

**Out (file, don't absorb):** gpkg metadata members (#39); the
transforms themselves (#38 — this item only makes their gates true);
`PORTAGE_DOCOMPRESS`/`PORTAGE_DOSTRIP` arrays (bash defaults,
`phase-helpers.sh:21-28`, not config); `ENV_UNSET`/`filter_calling_env`
beyond what the allowlist gives; `userpriv`/`fakeroot`; the `depend`
phase env (must stay byte-identical — `--regen` goldens).

---

## 3. Gates (answer before S1 code)

- **G1 Allowlist vs everything.** Recommendation (safest): the builder
  exports `other_vars ∩ REAL_ENVIRON_WHITELIST \ PORTUALE_COMPUTED`,
  where `REAL_ENVIRON_WHITELIST` is transcribed from
  `special_env_vars.py:83-250` as a `const` with the citation, and
  `PORTUALE_COMPUTED` is the set `phase_env_vars` already sets (`D`, `ED`,
  `T`, `WORKDIR`, `HOME`, `PORTAGE_BUILDDIR`, `FILESDIR`, `DISTDIR`,
  `ROOT`, `EROOT`, `PATH`, `P*`, `CATEGORY`, `EAPI`, `EBUILD_PHASE`,
  `PORTAGE_TMPDIR`, …, each with its `doebuild_environment()` line). Plus
  the profile arch/multilib family that is not in real's whitelist by
  name but reaches the ebuild via `defaults` on `setup`: `ARCH`, `ELIBC`,
  `KERNEL`, `ABI`, `DEFAULT_ABI`, `MULTILIB_ABIS`, `LIBDIR_*`,
  `IUSE_IMPLICIT`, `USE_EXPAND*` (explicit list, cited to the oracle
  env). Owner: user. Alternative (deepseek G1.1): everything minus
  filter — record why rejected for v1 (§0.2).
- **G2 Empty values.** `build_config_env` skips empties. Real exports
  `PORTAGE_COMPRESS=""` as a meaningful "disable" (`ecompress:254`). The
  new builder must export empty strings for keys that are *set* empty
  and omit keys that are *unset*. Owner: agent (no user call needed;
  cite `ecompress`).
- **G3 `USE` shape.** `USE` = `effective_use_flags(...)` ∩
  `iuse_effective(candidate IUSE)`; plus `PORTAGE_USE` identical (real
  exports both). The `Packages`-index / `metadata/USE` field uses the
  *same* value (the oracle's `metadata/USE` is `abi_x86_64 amd64
  elibc_glibc kernel_linux`, not enabled-IUSE-only) — so
  `write_post_install_metadata`/`package_after_install`'s `use_flags`
  should receive it too; `use_flags_display` stays for `--pretend`
  bracket output only. Owner: user (S1 presents a live side-by-side).
- **G4 Rust `FEATURES` gates.** Pass the resolved list explicitly where
  a `Config` is in scope (the `emerge` paths); keep `std::env` as the
  standalone `ebuild <file>` fallback. If the change grows past the ~13
  sites, split as #37b. Owner: user if it changes any CLI-visible
  meaning (`--buildpkg=n` still wins).
- **G5 Where it lives.** `portage_profile::phase_environ(&Config) ->
  Vec<(String,String)>` (pure, unit-testable, no binary) + `emerge_build::
  entry_phase_env(...)` per entry. `ebuild_phases` stays config-free so
  standalone `ebuild <file>` keeps working. `pretend::build_config_env`
  becomes a wrapper (or is deleted). Owner: agent.
- **G6 Brush quoting.** If any exported value can contain `$`/backticks
  (`PORTAGE_COMPRESS_EXCLUDE_SUFFIXES` has `[l]?` — no `$`, fine; but
  `CFLAGS` can carry `$(…)` in the wild), `phase_setup_script` must
  shell-quote. Owner: agent — implement single-quote escaping in S2,
  small and safe.

**Evidence bar:** accept S2 only when, on the L2 porttest track, the
portuale archive's `metadata/USE`, `metadata/FEATURES`, `.keep_*-<slot>`
and normalized `environment.bz2` match real's for `emptydirs`, `docs`,
`splitdebug`, and `NEEDED` paths start `/usr/lib64`. "Bigger
`environment.bz2`" is not evidence.

---

## 4. Slices

### S0 — Recon table (M, 2-3 h, no code)

1. Unpack real vs portuale `porttest/{docs,splitdebug,emptydirs}`
   archives (`TEST/logs/_l1-pkgcache/porttest/*` vs the last L2 run);
   normalize `environment.bz2` with `TEST/compare/normalize.py`'s
   ruleset (import, don't copy); list every key present/different.
2. For each key: real setter (`config.py`/`doebuild.py`/profile
   `make.defaults`/`make.globals`/bash default) → target slice.
3. Write the table into `TEST/findings/l2.md` under `l2-bpkgonly-env`
   (extend, don't replace). Anything not covered by §2 → new finding.

### S1 — `phase_environ()` builder + `USE` shape (F, 5-8 h)

1. Transcribe `environ_whitelist` and `environ_filter` as `const`s in
   `portage-profile` with line citations (they are data; a one-time
   transcription, refreshed when the vendored portage moves).
2. Define `PORTUALE_COMPUTED` (G1) with per-entry citation.
3. `phase_environ(&Config)`: `other_vars` ∩ whitelist ∖ computed ∪
   explicit profile family; `FEATURES` and `PORTAGE_FEATURES` from
   `resolved_incremental("FEATURES")` (fallback `other_vars`); export
   empties (G2). Deterministic order (sorted by key).
4. `effective_phase_use(config, iuse, ...) -> String`: `effective_use_
   flags` ∩ `iuse_effective(iuse)`, space-joined, sorted like real
   (`PORTAGE_USE` is `" ".join(sorted(...))`, `config.py:2261`).
5. Unit tests (synthetic profile under a temp `config_root`, seeded
   `make.conf`): `USE` contains exactly `amd64 elibc_glibc kernel_linux
   abi_x86_64` for empty IUSE and adds `foo` for `IUSE="foo"` when
   enabled; `FEATURES` folds a `-token`; `PORTAGE_COMPRESS=""` exported
   as empty; `SRC_URI`/`RDEPEND` absent; `HOME`/`DISTDIR` absent even
   when set in `make.conf`; `SOURCE_DATE_EPOCH` passes through;
   `MULTILIB_ABIS`/`LIBDIR_amd64`/`ABI` present.
6. Present the G3 side-by-side (old `build_use_env` vs new) on
   `porttest/docs` and one IUSE-bearing real package to the user.

### S2 — Thread it (M, F review, 5-8 h)

1. `emerge <atom>`: replace `build_config_env` at `pretend.rs:4050` and
   `:11705` with `phase_environ(&config)`; extend `entry_build_env` with
   `SLOT` (`entry.slot` or `"0"`, the `entry_package_env_vars` precedent
   at `:531`), `PORTAGE_REPO_NAME` (candidate repo), `PORTAGE_REPO_
   REVISIONS` (`"{}"` unless tracked), `USE`/`PORTAGE_USE` from S1.4.
2. `phase_env_vars`: prefer an `extra_env` `FEATURES` over
   `phase_features_value()` — simplest: leave the base as is, since
   `extra_env` extends last and wins; but **assert** in a unit test that
   the last `FEATURES` pair wins under both backends (brush
   `phase_setup_script` exports in order → last wins too; verify).
3. `--buildpkgonly`: `run_buildpkgonly` gains `build_env: &[(String,
   String)]` + a per-entry closure or the `MergeOptions`-shaped struct
   (G5); `run_package` gains an `extra_env` param forwarded to
   `run_commands` and passes the real `USE` to `package_after_install`.
   Standalone `ebuild <file> package` keeps `&[]`/`""`.
4. `--resume` source path (`pretend.rs:4043-4050`) uses the same.
5. Brush: shell-quote values in `phase_setup_script` (G6).
6. Tests: unit on `entry_build_env` shape and on the last-wins rule;
   Rust e2e (`tests/test_portuale.py` pattern) building `porttest/
   emptydirs`-like fixture from `fixtures/` asserting `.keep_*-0` and
   `metadata/USE`; contract suite untouched and green.

### S3 — Resolved `FEATURES` for Rust gates (M, F review, 3-6 h)

1. Inventory every `std::env::var("FEATURES")` on the build/merge path
   (`ebuild_phases.rs:1097,1106,1551,1582`, `pretend.rs:4132`,
   `ebuild_merge.rs`); classify: resolve-time (`buildpkg`,
   `binpkg-multi-instance`, `binpkg-signing`, `buildpkg-live`),
   phase-time (sandbox family, `distlocks`, `force-mirror`), CLI-only.
2. Pass the resolved list (`MergeOptions::features`, already there)
   into the phase-time gates via a parameter; keep the env read as the
   standalone fallback with a doc comment saying so.
3. Tests: `FEATURES="-sandbox"` in fixture `make.conf` disables the
   sandbox despite `sandbox` in the process env; `FEATURES=buildpkg` in
   `make.conf` produces a binpkg without `--buildpkg`.
4. Stop rule: if the parameter threading touches more than the
   inventory, file #37b and land S1/S2 first.

### S4 — Container proof + allowlist cleanup (F, 3-6 h + wall clock)

1. `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`;
   delete `KNOWN_FINDINGS` (`l2-portuale-builder.sh:62-64`) and
   `known-divergences.yaml` `l2-bpkgonly-env*` rows that now pass; the
   #38/#39 rows stay.
2. Real set: `L2_REBUILD=1 L2_MODE=payload-tolerant L2_BUILD_MODE=deep
   TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-merge.txt`;
   acceptance = oniguruma in `/usr/lib64`, jq configures. Record the new
   stop point; classify new findings env/#38/#39/new — never widen the
   allowlist to pass.
3. L1 porttest pair re-run: prebuilt merges must not move; any
   `environment` diff is a finding.
4. Also run L0 once (build path touched; cheap insurance).

### S5 — Docs (S/M, 1-2 h)

`what-this-proves.md` paragraph (live-verified command + the grep of
`metadata/USE`); `scope-backlog.md` §K bullet closed/narrowed;
`backlog-tasks.md:73` DONE; `030_L3-source-build-parity.deepseek.md`
G0.5/G0.6 pointer; one-line `agent-context.md` memory note only if
needed.

---

## 5. Risks, traps, stop rules

1. **Wrong `USE` is worse than missing `USE`** — S1 stop rule: if the new
   value disagrees with real on any fixture (not merely adds), stop and
   escalate G3.
2. **Clobbering computed vars** — `extra_env` wins; `PORTUALE_COMPUTED`
   is the guard; unit-test that a `make.conf` `HOME=` does not reach the
   phase.
3. **`depend` phase** must stay byte-identical (`--regen` goldens): the
   builder never runs for `depend`.
4. **Brush quoting** (G6) — the moment config text reaches `export
   NAME=…` unquoted, `$(…)` in `CFLAGS` executes.
5. **Fixture `config_root` has no `make.globals`** — tests must seed
   values; a unit test that passes on the host and fails in CI is this.
6. **Version skew** — record the image's portage (3.0.82.2) with every
   run; `environ_whitelist` contents differ across versions.
7. **`environment.bz2` churn** is expected on source builds and
   forbidden on L1 prebuilt merges.
8. **Stop rule (real set):** if jq still dies on an env-shaped error
   after S2, file the exact variable/consumer with a repro; do not add
   ad-hoc exports.

---

## 6. Delegation brief

Hand a subagent: this file as scope authority; the G1-G6 answers (never
let it re-decide G1/G3 silently); the exact files/lines from §1.2; the
oracle path from §1.1; the rule "negative claims about real must be
checked in `3rdparty/portage/`"; the evidence bar from §3. Expected
return: diff, commands + results, archive/env evidence, new findings with
repros. A subagent without container access may implement S1-S3 but must
label them **unverified end-to-end**.
