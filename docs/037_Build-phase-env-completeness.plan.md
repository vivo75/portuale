# Plan: backlog #37 — Build-phase env completeness (merged)

Status: **S0 + S1 + S2 complete 2026-09-13; S3 not started.** S2 threaded the resolved env into `emerge`/`--resume`/`--buildpkgonly` (both backends), dropped `AA`/`O`, shell-quoted the brush exports, and captured the L2 porttest track green (0 unexplained) with the `l2-bpkgonly-env` rows deleted; residuals filed in `TEST/findings/l2.md` S2. G1 (full
layer) and G3 (`PORTAGE_USE` everywhere) decided by the owner; S1 landed
`portage_profile::phase_environ` / `portage_use` + the three transcribed
key sets, unit-tested, not yet threaded (S2). S0's exhaustive
var-by-var recon is written into `TEST/findings/l2.md` under
`l2-bpkgonly-env` ("S0 recon (#37, 2026-09-13)"): real 160 env keys vs
portuale 45 on the porttest pair; 118 only-real grouped by setter and
slice; 3 only-portuale (`AA`, `O`, `LC_ALL`); 2 value-differs (`USE`,
`FEATURES`). S0 **revises G1** (see §4 G1) — the whitelist-only export
set cannot satisfy the `environment.bz2` acceptance bar; owner decision
required before S1. Written 2026-09-13 against `main` @ `ec13936`.

This is the **merge** of three independent drafts —
`037_Build-phase-env-completeness.deepseek.md`, `.musespark.md`,
`.claude.md` — keeping, per the owner's brief, *the most promising
approach and the safest route*. Where the drafts disagreed, §0 records
the adjudication and the checkout evidence that settled it. The
individual drafts stay as history; **this file is the scope authority
for agents**.

Backlog: `docs/backlog-tasks.md:73`; `scope-backlog.md` §K
`l2-bpkgonly-env` (`:659-668`). Findings: `TEST/findings/l2.md:238-282`
(S3) and `:331-359` (S5, "scope widened"). L3 contract:
`docs/030_L3-source-build-parity.deepseek.md` §S3 + G0.5/G0.6. This item
is the **producer prerequisite** for the L2 real set, all of L3, #38
(its gates read the resolved `FEATURES`/`PORTAGE_COMPRESS` this item
threads), and the VDB half of #39.

**Read first:** `AGENTS.md` (steps 1, 4's real-execution carve-out, 5,
7, 8), `docs/agent-context.md` ("Real ebuild phase execution"),
`TEST/findings/l2.md` S3+S5, then the code under test —
`rust/portuale/src/ebuild_phases.rs` (`phase_standalone_base_env` `:841`,
`ENVIRON_WHITELIST` `:1387-1541`, `feature_token_present` `:1550`,
`phase_features_value` `:1581`, `phase_env_vars` `:1950-2137`,
`phase_setup_script` `:2139-2190`), `rust/portuale/src/emerge_build.rs`
(`run_buildpkgonly` `:118`, `build_use_env` `:516`, `entry_build_env`
`:558`), `rust/portuale/src/ebuild_package.rs` (`run_package` `:448`,
`package_after_install` `:487`), `rust/portuale/src/pretend.rs`
(`feature_enabled` `:4132`, `BUILD_VARS` `:5948`, `config_features_list`
`:5973`, `build_config_env` `:6002`, call sites `:4050`/`:11705`/
`:11750`), `rust/portage-profile/src/lib.rs` (`Config` `:385`,
`other_vars` `:911`, `incremental_sources`/`resolved_incremental`
`:922-947`, `apply_env_layer` `:1273`, `make.globals` read `:2245-2258`),
`rust/portage-repo/src/lib.rs` (`effective_use_flags` `:2509`,
`candidate_use_flags_display` `:5406`). Real authority:
`3rdparty/portage/lib/portage/package/ebuild/config.py::environ()`
(`:3263-3350`), `_config/special_env_vars.py` (`environ_whitelist` `:83`,
`environ_filter` `:257`), `doebuild.py::doebuild_environment()`
(`:381-545`, `PORTAGE_REPO_NAME` `:483`),
`_emerge/EbuildPhase.py::_setup_repo_revisions` (`:73-108`),
`bin/save-ebuild-env.sh` (`:20-29`, `:105-125`). Line numbers were
correct at `ec13936`; **re-locate by symbol name before editing**.

Model tiers (repo convention since 022): **F** frontier (Claude Opus 5 /
Fable 5.1), **M** mid (Claude Sonnet 5), **S** small (Claude Haiku 4.5).
"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Adjudications (what was merged, what was rejected, why)

| Topic | deepseek | musespark | claude | **Merged decision** | Evidence |
|---|---|---|---|---|---|
| Nature of the work | one architectural asymmetry, semantic core | plumbing in existing seams | plumbing, with one semantic knot (USE) | **plumbing with one gated semantic decision**: the seams exist (`entry_build_env`, `extra_env`-last, `config_features_string`, `effective_use_flags`); only the USE shape and the export set need a decision | `emerge_build.rs:558-566`, `ebuild_phases.rs:2125`, `pretend.rs:5988`, `portage-repo/src/lib.rs:2509` |
| Export set (G1) | *everything* in resolved config minus `environ_filter` | keep the curated whitelist, add the known vars | real's own `environ_whitelist` ∩ `other_vars`, minus computed, plus the profile arch/multilib family | **originally claude's, revised to deepseek's by S0 evidence (2026-09-13)** — real's saved env is the accumulated *setup-phase full dump* (real imports all of `os.environ`, `config.py:551,567`; the whitelist only kicks in from phase 2, `:3275-3305`), so whitelist-only cannot pass the `environment.bz2` acceptance row. Revised route: `(config scalars ∪ process env) − environ_filter − PORTUALE_COMPUTED`; the whitelist route survives only as the fallback with a weakened acceptance bar. musespark's "add the six" fails on the next real package | `TEST/findings/l2.md` "S0 recon"; `config.py:3275-3305`, `special_env_vars.py:83-250` |
| `USE` shape (G3) | `PORTAGE_USE` = enabled filtered by IUSE/implicit | `PORTAGE_USE` for the phase var; **keep enabled-IUSE-only for index/metadata `USE`** | enabled ∩ `IUSE_EFFECTIVE`, **same value for `metadata/USE`** | **claude/deepseek**: musespark's trap 3 is wrong — real's binpkg `metadata/USE` *is* `PORTAGE_USE` (oracle: `abi_x86_64 amd64 elibc_glibc kernel_linux` for a package with empty IUSE) | `TEST/findings/l2.md:246`, `config.py:3329`, vdb oracle env line 918 |
| `ABI` | compute from `DEFAULT_ABI` | — | plain profile scalar | **plain scalar**: `config.py` never mentions `ABI`; profile `make.defaults` sets it, `multilib.eclass` reads `${ABI:-${DEFAULT_ABI}}` | `grep ABI config.py` → 0 hits; oracle line 802 |
| Empty values (G2) | — | `PORTAGE_COMPRESS=""` must flow as empty (in its 038 draft) | export set-but-empty, omit unset | **export set-but-empty**; `build_config_env`'s skip-empties shape is wrong for the new builder | `ecompress:228-229,254` |
| Rust `FEATURES` gates | in this slice (G1.4), split as #37b if it grows | not mentioned | in this slice, M with F review, stop rule → #37b | **in this slice as S3, with the #37b stop rule** | `ebuild_phases.rs:1097,1106,1551,1582`, `pretend.rs:4132` |
| Tiering | F for builder/gates/verification; M threading | M throughout, F review of USE | F for builder+USE and container triage; M threading | **F: S1, S4. M (F review): S2, S3. M: S0. S/M: S5** — see §1.2 | — |
| Recon slice | none (jump to builder) | S0 var-by-var table first | S0 table first | **S0 first** — cheap, removes unknowns, and it is what the delegation brief hands to cheaper models | — |
| Brush quoting | risk item | — | gate G6, fix in S2 | **fix in S2** (single-quote escaping in `phase_setup_script`); config text (`CFLAGS` with `$(…)`) is about to reach an unquoted `export` | `ebuild_phases.rs:2139-2147` |
| Fixture `config_root` | — | — | no `make.globals` under fixtures → seed explicitly | **kept as a trap** | `lib.rs:2245-2258` |
| `SLOT` default | — | `None` → `"0"` (existing precedent), never `""` | same | **kept** | `emerge_build.rs:531` |
| Determinism direction | — | every slice moves a var process-env → resolved config, never reverse | — | **kept as an invariant** | — |

Everything below is the merged plan; the drafts are not needed to
execute it.

---

## 1. Opinion

### 1.1 Verdict

**Do #37 first (before #38), as one focused item of five slices.** It
is one bug seen from seven angles: real builds the phase env from the
resolved config (`config.environ()`), portuale from a curated base plus
an `extra_env` tail carrying eleven compiler/make flags
(`pretend.rs::BUILD_VARS`) and an enabled-IUSE-only `USE`. Symptoms
(`TEST/findings/l2.md`): `USE` lacks `amd64 elibc_glibc kernel_linux`;
`FEATURES` is the raw process env (so `binpkg-dostrip`/`binpkg-docompress`
from `make.globals` never appear — #38's gates are false for this reason
alone); no `MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*` so `get_libdir` →
`lib`, oniguruma lands in `/usr/lib`, jq's `econf` dies; no `SLOT`
(`.keep_porttest_emptydirs-`); no `PORTAGE_REPO_NAME`/`_REVISIONS`;
`--buildpkgonly` passes `&[]` (`ebuild_package.rs:455-465`).

**The fix is plumbing.** Verified in this checkout: `Config::other_vars`
already holds every scalar including `make.globals` (read
`config_root`-relative on a live host), the profile chain's
`MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_amd64`/`ARCH`/`ELIBC`/`KERNEL`/`ABI`,
and `PORTAGE_COMPRESS*`; `resolved_incremental("FEATURES")` already
stacks the process env on top of `make.globals`+profile+`make.conf`
exactly like real (`apply_env_layer`); `config_features_string` already
produces the right string for `MergeOptions::features`; `entry_build_env`
is the per-entry seam and is applied last (wins). What is missing is
*feeding* these seams the right set, and giving `run_buildpkgonly` →
`run_package` an env at all.

### 1.2 Model tiers and split

| Slice | Content | Tier | Why |
|---|---|---|---|
| S0 | recon: var-by-var real-vs-portuale table, each row → setter → slice | M | mechanical diffing against the oracle; no decisions |
| S1 | `phase_environ()` builder + `USE` shape + pure unit tests | **F** | the one semantic decision; every later slice consumes it; wrong `USE` flips every `use()` in every ebuild |
| S2 | thread into `emerge <atom>`, `--resume`, `--buildpkgonly`; brush quoting | M (F review) | four call sites, one signature change |
| S3 | resolved `FEATURES` for the Rust-side gates | M (F review) | ~13 sites; "same list, different source" |
| S4 | container proof (porttest track, real set past jq, L1 non-regression) + allowlist cleanup | **F** | cross-language triage; env-bug vs #38 vs #39 vs new |
| S5 | docs closeout | S/M | mechanical |

A cheap model can execute S0, S2, S3, S5 once S1's builder is frozen and
§4's gates are answered. S1 and S4 are not delegable below frontier: the
failure mode is a build env that *looks* resolved and is subtly wrong,
which the fixture track may not catch but every real ebuild will.

### 1.3 Difficulty

| Axis | 1-5 | Notes |
|---|---|---|
| Real-source grounding | 2 | `config.environ()` + `doebuild_environment()` are read, not inferred |
| Config semantics (USE shape, incremental `FEATURES`, whitelist/filter, empties) | 4 | the only place to be *wrong* rather than *incomplete* |
| Rust plumbing | 2 | seams exist; one new param on `run_buildpkgonly`/`run_package` |
| Verification | 3 | objective (L2 bed + normalized `environment.bz2`) but container-bound |
| Blast radius | 3 | `environment.bz2` of every source build changes by design; L1 (prebuilt) must not move |
| **Overall** | **3.5** | small diff, one deep decision, container-gated proof |

Effort: **~20-34 agent-hours, 4-5 sittings** (S0+S1 one sitting; S2+S3
one; S4 rides a container run; S5 short).

### 1.4 Order vs #38

Strictly **#37 → #38**. #38's transform branches
(`misc-functions.sh:149,243`) read `FEATURES` and `PORTAGE_COMPRESS`
from the phase env; until S2 lands they cannot fire. #38's own recon can
start after S2 (it does not need S3-S5).

---

## 2. Ground truth (verified at `ec13936`)

### 2.1 Real

- `config.environ()` (`config.py:3263-3350`): iterate every string in
  the stacked config; skip `environ_filter`; when `$T/environment` exists
  (every phase after `setup` on a normal build, `:3275-3284`) also skip
  anything not in `environ_whitelist`/`_re` (`:3296-3305`); then force
  `PORTAGE_FEATURES = FEATURES` (`:3326`) and `USE = PORTAGE_USE`
  (`:3329`, "filtered by IUSE and implicit IUSE"); pop `AA`, keep
  `MERGE_TYPE`, EAPI-gate `ESYSROOT`/`BROOT` (EAPI ≥5 only here — treat
  uniformly per the EAPI-floor rule in `agent-context.md`).
- `environ_whitelist` (`special_env_vars.py:83-250`) includes `FEATURES`,
  `USE`, `SLOT`, `PORTAGE_COMPRESS`, `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES`,
  `PORTAGE_COMPRESSION_COMMAND`, `PORTAGE_REPO_NAME`,
  `PORTAGE_REPO_REVISIONS`, `CCACHE_*`/`DISTCC_*` by regex. The arch/
  multilib family (`ARCH`, `ELIBC`, `KERNEL`, `ABI`, `DEFAULT_ABI`,
  `MULTILIB_ABIS`, `LIBDIR_*`, `IUSE_IMPLICIT`, `USE_EXPAND*`) is *not*
  in the whitelist by name: it reaches the ebuild on the first (`setup`)
  phase from `configdict["defaults"]` and persists via the saved
  `$T/environment`. Portuale re-exports on every phase, so it must always
  export them explicitly.
- `doebuild_environment()` (`doebuild.py:381-545`): dynamic vars; `SLOT`
  via `setcpv`; `PORTAGE_REPO_NAME` (`:483`, `configdict["pkg"]`);
  `PORTAGE_REPO_REVISIONS` via `EbuildPhase._setup_repo_revisions`
  (`EbuildPhase.py:73-108`, `json.dumps(sort_keys=True)`, `"{}"` when
  nothing is tracked); consumed by `phase-functions.sh:762-769` into
  build-info `repository`/`REPO_REVISIONS`.
- **Oracle** (`TEST/logs/l1-20260908T170620Z/portage.vdb/pkg/porttest/
  splitdebug-1.0/environment`): `ABI="amd64"`, `ARCH="amd64"`,
  `CBUILD="x86_64-pc-linux-gnu"`, `DEFAULT_ABI="amd64"`, `ELIBC="glibc"`,
  `KERNEL="linux"`, `LIBDIR_amd64="lib64"`, `MULTILIB_ABIS="amd64 x86"`,
  `SLOT="0"`, `PORTAGE_REPO_REVISIONS="{}"`, `USE="abi_x86_64 amd64
  elibc_glibc kernel_linux"`, `IUSE_EFFECTIVE="abi_x86_64 alpha amd64 …"`,
  `FEATURES`/`PORTAGE_FEATURES` = resolved sorted list containing
  `binpkg-docompress binpkg-dostrip`. `PORTAGE_COMPRESS` is **absent from
  the saved env by design** (`save-ebuild-env.sh:117`) — do not infer
  from the oracle that real doesn't export it to the phase.
- `PORTAGE_DOCOMPRESS`/`PORTAGE_DOSTRIP` arrays are bash defaults
  (`phase-helpers.sh:21-28`), not config. Nothing for Rust to do.

### 2.2 Portuale

- Base env `phase_env_vars` (`ebuild_phases.rs:1950-2137`): `FEATURES` at
  `:2043` from `phase_features_value()` (`:1581`, raw `std::env`); `USE`
  at `:2055` from `phase_standalone_base_env` (`:841-931`; config-
  resolved *only* for standalone `ebuild <file>`, returns `("", [])` for
  `depend` and whenever `extra_env` carries `USE`); `extra_env` appended
  last at `:2125` — **the override chain**.
- Bash backend `run_one_phase_bash` (`:2390`): `env_clear()` + (process
  env ∩ `ENVIRON_WHITELIST`) + vars. Brush backend `phase_setup_script`
  (`:2139-2190`): `export NAME={value:?}`, **not shell-quoted**
  (documented `:2139-2147`).
- `ENVIRON_WHITELIST` (`:1387-1541`) already lists `FEATURES`,
  `PORTAGE_COMPRESS*` (`:1469-1471`), `PORTAGE_REPO_NAME`/`_REVISIONS`
  (`:1497-1498`). It is a calling-env passthrough filter; it sets nothing.
- Run-wide `build_config_env` (`pretend.rs:6002-6013`): `BUILD_VARS` only
  (`CFLAGS CXXFLAGS CPPFLAGS LDFLAGS FFLAGS FCFLAGS ASFLAGS MAKEOPTS CHOST
  CBUILD CTARGET`), **skips empty values**; call sites `:4050` (resume),
  `:11705` (emerge). Per-entry `entry_build_env` (`emerge_build.rs:558-
  566`) = run-wide + `package.env` (`:531-556`, `slot` default `"0"`) +
  `build_use_env` (`:516-528`, enabled IUSE only). Production callers:
  `merge_one_source_entry` (`:500-501`), scheduler `build_one_source_
  entry` (`:858-869`), `run_merge_plan` (`:926-942`).
- `--buildpkgonly`: `pretend.rs:11750` → `run_buildpkgonly` (`emerge_
  build.rs:118-180`; has `repos` + `locate_candidate`, no config/env) →
  `run_package` (`ebuild_package.rs:448-474`; `run_commands(..., &[])`,
  then `package_after_install(..., "")`).
- Post-install: `run_commands_async` runs real `install_qa_check` right
  after `install` with the same `extra_env` (`ebuild_phases.rs:2996-
  3029`), then `write_post_install_metadata(&env, root, &build_phase_use
  (build_env))` — the `metadata/USE` consumer.
- Rust `FEATURES` gates reading `std::env`: `feature_token_present`
  (`ebuild_phases.rs:1550`), `phase_features_value` (`:1581`),
  `distlocks`/`force_mirror` (`:1097-1106`), `feature_enabled`
  (`pretend.rs:4132`), plus `ebuild_merge.rs` callers.
  `MergeOptions::features` (`ebuild_merge.rs:364`) already carries the
  resolved list (used for `PORTAGE_UPDATE_ENV`, `:3457-3465`).
- `portage_profile::Config`: `other_vars`, `incremental_sources` +
  `resolved_incremental` (sorted like real), `iuse_implicit` (`:701`),
  `iuse_effective` (`:716`), `use_expand*`; `make.globals` is read from
  `<config_root>/usr/share/portage/config/make.globals` (`:2252`) —
  **absent under a fixture `config_root`**, so fixture tests must seed
  `FEATURES`/`PORTAGE_COMPRESS`/multilib in the fixture `make.conf`/
  profile. `portage_repo::effective_use_flags` (`:2509`) is the single
  USE resolver; `candidate_use_flags_display` (`:5406`) wraps it for
  IUSE-declared flags.

### 2.3 Observed gaps (evidence, `TEST/findings/l2.md`)

| symptom | real | portuale | slice |
|---|---|---|---|
| `metadata/USE` | `abi_x86_64 amd64 elibc_glibc kernel_linux` | `abi_x86_64` / empty | S1/S2 |
| `metadata/FEATURES` | resolved incremental list | raw process env | S1/S2 |
| installed libdir | `/usr/lib64/…` | `/usr/lib/…` (jq can't find `oniguruma.pc`) | S1/S2 |
| `.keep_*` | `.keep_porttest_emptydirs-0` | `…-` (no `SLOT`) | S2 |
| `environment.bz2` | resolved env | bare env | S1/S2 |
| `PORTAGE_REPO_NAME` / revisions | `porttest` / `{}` | unset | S2 |
| `--buildpkgonly` env | same as `-b` | `&[]` | S2 |
| Rust-side gates | resolved list | process env | S3 |

Repro: `L2_REBUILD=1 L2_MODE=payload-tolerant L2_BUILD_MODE=deep
TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-merge.txt` → dies at
`app-misc/jq` configure (`Package 'oniguruma' not found`).

---

## 3. Scope

**In:** (1) a cited, real-derived export-set builder over `Config`;
(2) effective `USE` = enabled ∩ `IUSE_EFFECTIVE` (real's `PORTAGE_USE`
value; `PORTAGE_USE` itself is `environ_filter`ed, only `USE` is
exported), also used for `metadata/USE`; (3) per-entry `SLOT`,
`PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS`; (4) resolved
`FEATURES`/`PORTAGE_FEATURES` in the phase env; (5) the same on
`--buildpkgonly` and `--resume`; (6) `SOURCE_DATE_EPOCH` as an ordinary
config scalar through the same builder (L3 G0.6); (7) the Rust-side
`FEATURES` gates reading the resolved list where a config is in scope;
(8) shell-quoting in the brush `phase_setup_script`.

**Out (file, don't absorb):** gpkg metadata members and `NEEDED.ELF.2`
fields (#39); the transforms themselves (#38 — this item only makes
their gates true); `PORTAGE_DOCOMPRESS`/`PORTAGE_DOSTRIP` arrays (bash
defaults); `ENV_UNSET`/`filter_calling_env` beyond the whitelist;
`KV`/`AA`/`ESYSROOT`/`BROOT` EAPI-edge gating; `userpriv`/`fakeroot`
parity; the `depend` phase env (byte-identical — `--regen` goldens);
resolver/merge/scheduler/compare-stack changes; any Python mirror or
contract `CASES` (real-execution-only item, `AGENTS.md` step 4).

---

## 4. Gates (owner decisions — answer before S1 code)

- **G1 Export set.** **S0 evidence (2026-09-13) revised this gate.**
  Real's `environment.bz2` is the accumulated *setup-phase full dump*
  (real imports the whole process env as the top config layer,
  `config.py:551,567`; `environ_filter` is applied to every export and
  the `environ_whitelist` restriction only from the second phase on,
  `config.py:3275-3305`). Portuale's 45-key curated env cannot match it;
  the porttest track's `metadata/environment.bz2 differs` row is
  unfixable under a whitelist-only builder. **Revised recommendation
  (deepseek's route):** export `(resolved config scalars ∪ process env)
  − environ_filter − PORTUALE_COMPUTED`, plus synthesized empty
  `USE_EXPAND` placeholders (`config.py:2247`) and the profile
  arch/multilib family. `PORTUALE_COMPUTED` = every key `phase_env_vars`
  already computes (`D`, `ED`, `T`, `WORKDIR`, `HOME`, `PORTAGE_BUILDDIR`,
  `FILESDIR`, `DISTDIR`, `ROOT`, `EROOT`, `PATH`, `P`, `PN`, `PV`, `PR`,
  `PVR`, `PF`, `CATEGORY`, `EAPI`, `EBUILD_PHASE`, `PORTAGE_TMPDIR`,
  `EMERGE_FROM`, `PORTAGE_BIN_PATH`, `PORTAGE_PYM_PATH`, …, each with its
  `doebuild_environment()` setter line) and always wins. Mirror
  `environ_filter` as a `const` with citation. **Fallback** (the
  pre-S0 recommendation, only if the user prefers a narrower diff
  surface): whitelist ∩ `other_vars` ∪ `PROFILE_FAMILY`, and the S2
  acceptance bar drops exact `environment.bz2` parity to "no
  config-resolved key missing" — a weaker bar L3's VDB diff will keep
  surfacing. Owner: **user** — **decided 2026-09-13: full layer**
  (implemented as `portage_profile::phase_environ`, S1).
- **G2 Empty values.** Export keys that are *set to empty* as empty
  (`PORTAGE_COMPRESS=""` is real's documented "disable", `ecompress:254`);
  omit keys that are *unset*. `build_config_env`'s skip-empties shape is
  not reused. Owner: agent.
- **G3 `USE` shape.** `USE` = `PORTAGE_USE` = `effective_use_flags(…)` ∩
  `iuse_effective(candidate IUSE)`, sorted, space-joined (`config.py:
  2261`). The **same value** feeds `write_post_install_metadata` /
  `package_after_install`'s `use_flags` (`metadata/USE`, `Packages`
  `USE`); `GraphEntry::use_flags_display` stays for `--pretend` bracket
  output only. Note `PORTAGE_USE` is in real's `environ_filter`
  (`special_env_vars.py`), so only `USE` is exported. Side-by-side
  (S0 oracle): `porttest/docs` old `""` → new `abi_x86_64 amd64
  elibc_glibc kernel_linux`; `dev-libs/oniguruma` old `abi_x86_64` → new
  `abi_x86_64 amd64 elibc_glibc kernel_linux` (= real's `metadata/USE`).
  Owner: **user** — **decided 2026-09-13: `PORTAGE_USE` everywhere.** **Stop rule:** if the new value is *wrong* (not
  merely more complete) on any fixture, stop and escalate.
- **G4 Rust `FEATURES` gates.** Pass the resolved list explicitly where a
  `Config` is in scope (the `emerge` paths); keep `std::env` as the
  standalone `ebuild <file>` fallback with a doc comment. CLI-visible
  meanings unchanged (`--buildpkg=n` still wins, `-buildpkg-live` still
  negates). If threading grows past the inventoried sites, file **#37b**
  and land S1/S2 first. Owner: user only if a CLI meaning would change.
- **G5 Where it lives.** `portage_profile::phase_environ(&Config) ->
  Vec<(String, String)>` (pure, unit-testable, no binary) + `emerge_build::
  entry_phase_env(...)` for the per-entry part. `ebuild_phases` stays
  config-free (standalone `ebuild <file>` keeps working);
  `pretend::build_config_env` becomes a wrapper or is deleted at both
  call sites. `run_buildpkgonly` receives the resolved env (not a
  `Config`) to stay host-testable. Owner: agent.
- **G6 Brush quoting.** `phase_setup_script` gets single-quote escaping
  before any config text reaches it. Owner: agent.
- **G7 No feature gate.** Land ungated — these are bug fixes toward real;
  the L2 bed is the safety. Owner: user (default: agree).

**Evidence bar:** S2 is accepted only when, on the L2 porttest track, the
portuale archive's `metadata/USE`, `metadata/FEATURES`, `.keep_*-<slot>`
and *normalized* `environment.bz2` match real's for `emptydirs`, `docs`,
`splitdebug`, and `NEEDED` paths start `/usr/lib64`. "It compiles and
`environment.bz2` is bigger" is not acceptance.

---

## 5. Slices

### S0 — Recon table (M, 2-3 h, no product code)

1. Unpack real vs portuale `porttest/{docs,splitdebug,emptydirs}` archives
   (`TEST/logs/_l1-pkgcache/porttest/*` vs the last L2 run, or rebuild
   with `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`);
   normalize `environment.bz2` with `TEST/compare/normalize.py`'s
   `norm_environment` (import it, don't copy); list every key present in
   real and absent/different in portuale.
2. For each key: real setter (`config.py` / `doebuild.py` / profile
   `make.defaults` / `make.globals` / bash default) → target slice
   (S1-S3) or "out of scope → filed as …".
3. Same for the S5 real-set case: `MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_*`
   presence in each env; oniguruma `NEEDED` path.
4. Write the table into `TEST/findings/l2.md` under `l2-bpkgonly-env`
   (extend, don't replace). Confirm §2.3 rows; file anything extra as a
   new finding.

**Acceptance:** every env-caused L2 diff has a row, a cited setter, and a
slice; no code changed.

### S1 — `phase_environ()` builder + `USE` shape (F, 5-8 h)

1. Transcribe `environ_filter` as a `const` in `portage-profile` with
   line citations (data, refreshed when the vendored portage moves).
   Define `PORTUALE_COMPUTED` (G1) with per-entry citations. Keep the
   `environ_whitelist` transcription only if G1's fallback route is
   chosen — under the revised route it is documentation, not data.
2. `phase_environ(&Config)`: `(other_vars ∪ process env) −
   environ_filter − PORTUALE_COMPUTED`, plus the synthesized empty
   `USE_EXPAND` placeholders (for every `Config::use_expand` name not
   otherwise set) and the profile arch/multilib family; `FEATURES`
   **and** `PORTAGE_FEATURES` from `resolved_incremental("FEATURES")`
   (fallback `other_vars`, then nothing); export set-but-empty (G2);
   deterministic key order. `SOURCE_DATE_EPOCH`, `PORTAGE_COMPRESS*`,
   `MAKEOPTS`, `CBUILD` etc. fall out of the set — pin them in tests,
   do not special-case. Real's phase-1 semantics is the spec; the
   per-phase whitelist re-export (phase 2+) is not replicated (fresh
   shell, §see S0 note).
3. `effective_phase_use(config, repos, category, package, version) ->
   String`: `effective_use_flags` ∩ `iuse_effective(IUSE)`, sorted,
   space-joined (G3). Reuse `candidate_use_flags_display`'s candidate/
   md5-cache lookup; do not add a second USE resolver.
4. Unit tests (synthetic profile under a temp `config_root` with a seeded
   `make.conf` — no `make.globals` there, trap §7.5): `USE` is exactly
   `abi_x86_64 amd64 elibc_glibc kernel_linux` for empty IUSE and gains
   `foo` for `IUSE="foo"` when enabled, never a global flag the package
   doesn't declare; `FEATURES` folds a `-token` and includes a `make.conf`
   token; `PORTAGE_COMPRESS=""` exported as empty, `PORTAGE_COMPRESS_FLAGS`
   omitted when unset; `SRC_URI`/`RDEPEND` absent; `HOME`/`DISTDIR` absent
   even when set in `make.conf`; `SOURCE_DATE_EPOCH` passes through;
   `MULTILIB_ABIS`/`LIBDIR_amd64`/`ABI`/`ARCH` present.
5. Present the G3 side-by-side to the user; record the answer here.

**Exit:** unit tests green; a doc comment mapping each rule to its real
source line; F review of `PORTUALE_COMPUTED`; G3 answered.

### S2 — Thread it (M, F review, 5-8 h)

1. `emerge <atom>`: replace `build_config_env` at `pretend.rs:4050` and
   `:11705` with `phase_environ(&config)`. Extend `entry_build_env` with
   `SLOT` (`entry.slot` or `"0"`, never `""`), `PORTAGE_REPO_NAME`
   (candidate repo), `PORTAGE_REPO_REVISIONS` (`"{}"` unless tracked),
   `USE` from S1.3 (`portage_use`). Keep `extra_env` as the **last** layer.
2. `FEATURES`: since `extra_env` wins, the base `phase_features_value()`
   can stand as the standalone fallback; **unit-test** that the last
   `FEATURES` pair wins under both backends (bash `cmd.envs` — later
   wins; brush `export` in order — later wins; verify, don't assume).
3. `--buildpkgonly`: `run_buildpkgonly` gains the resolved env + per-entry
   closure (G5); `run_package` gains an `extra_env` param forwarded to
   `run_commands`, and passes the real `USE` to `package_after_install`
   (fixes the `Packages` `USE` field on this path too). Standalone
   `ebuild <file> package` keeps `&[]`/`""` (documented "no graph, no
   USE" precedent stays true for that path only).
4. `--resume` source path (`pretend.rs:4043-4050`) uses the same builder.
5. Brush: single-quote escaping in `phase_setup_script` (G6) + a unit
   test with a `$(…)`-bearing value.
6. `depend` phase: assert (test) its env is byte-identical to before.
7. Tests: unit on `entry_build_env` shape and last-wins; Rust e2e
   (`tests/test_portuale.py` pattern) building an `emptydirs`-like
   `fixtures/` ebuild with seeded `make.conf` asserting `.keep_*-0` and
   `metadata/USE`; `python3 -m pytest tests -q` unchanged and green.

**Exit:** porttest track `l2-bpkgonly-env` rows pass on both `-b` and
`--buildpkgonly`; `--buildpkgonly` and `-b` archives are metadata-equal
modulo `BUILD_TIME`/`BUILD_ID`; oniguruma-class `get_libdir` → `lib64`
(container proof deferred to S4 if no container in this sitting — label
**unverified end-to-end**).

### S3 — Resolved `FEATURES` for Rust gates (M, F review, 3-6 h)

1. Inventory every `std::env::var("FEATURES")` on the build/merge path
   (`ebuild_phases.rs:1097,1106,1551,1582`, `pretend.rs:4132`,
   `ebuild_merge.rs` callers); classify: resolve-time (`buildpkg`,
   `binpkg-multi-instance`, `binpkg-signing`, `buildpkg-live`),
   phase-time (`sandbox`/`usersandbox`/`{network,ipc,mount,pid}-sandbox`,
   `distlocks`, `force-mirror`), CLI-only (keep raw).
2. Pass `MergeOptions::features` (already resolved) into the phase-time
   gates by parameter; keep the env read as the standalone fallback with
   a doc comment saying so.
3. Tests: fixture `make.conf` `FEATURES="-sandbox"` disables the sandbox
   despite `sandbox` in the process env; `FEATURES=buildpkg` in
   `make.conf` yields a binpkg without `--buildpkg`.
4. **Stop rule:** if threading exceeds the inventory, file #37b, land
   S1/S2.

**Exit:** no `std::env::var("FEATURES")` on the build execution path
without a documented fallback reason; L3's `make.conf`-based determinism
block (`docs/030` §1.4 trap 4) is honoured symmetrically.

### S4 — Container proof + allowlist cleanup (F, 3-6 h + wall clock)

1. `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt`:
   delete `KNOWN_FINDINGS` rows `l2-bpkgonly-env|…` (`l2-portuale-
   builder.sh:62-64`) and the yaml entry (`known-divergences.yaml:82-95`);
   #38/#39 rows stay. Run clean.
2. Real set: `L2_REBUILD=1 L2_MODE=payload-tolerant L2_BUILD_MODE=deep
   TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-merge.txt`.
   Acceptance: oniguruma in `/usr/lib64`, jq configures. Record the new
   stop point in `TEST/findings/l2.md`; classify new findings env (fix
   here) / #38 / #39 / new (file). Never widen the allowlist to pass.
3. L1 porttest pair re-run **with** the portage upgrade: prebuilt merges
   must not move; any `environment` diff is a finding.
4. L0 once (build path touched; cheap insurance).
5. Mark `l2-bpkgonly-env` FIXED in `TEST/findings/l2.md` with the commit.

**Exit:** no allowlist entry for #37 anywhere; porttest track green from
a clean run; the real set's next blocker is *not* env completeness.

### S5 — Docs (S/M, 1-2 h)

- `docs/what-this-proves.md`: append one paragraph (never rewrite prior
  ones) with a live-verified command (build `porttest/docs`, grep the
  archive's `metadata/USE`/`FEATURES`; oniguruma libdir before/after).
- `scope-backlog.md` §K: close/narrow the "curated whitelist" bullet;
  `backlog-tasks.md:73` → DONE (or narrowed, with #37b if filed).
- `docs/030_L3-source-build-parity.deepseek.md` G0.5/G0.6 status pointer.
- `docs/agent-context.md` one-line memory note only if a next-session
  scoper must know it.
- §10 below: findings filed while executing.

---

## 6. Fixtures and tests

| What | Where | Notes |
|---|---|---|
| `porttest/emptydirs` | `TEST/images/overlay/porttest/porttest/emptydirs/` | isolates `SLOT` in `.keep_*` |
| `porttest/docs`, `porttest/splitdebug` | same tree | `metadata/USE`/`FEATURES`, `get_libdir` on amd64; later #38's targets |
| `app-misc/jq` / `dev-libs/oniguruma` | real set (`atomlists/l1-merge.txt`) | the end-to-end acceptance |
| Rust unit | `rust/portage-profile/src/lib.rs`, `rust/portuale/src/{emerge_build,ebuild_phases,ebuild_package}.rs` | pure map, `USE` shape, last-wins, quoting, `depend` byte-identity |
| Rust e2e | `tests/test_portuale.py` pattern | real-execution-only; no Python mirror |
| Python contract | `tests/test_emerge_pretend_contract.py` | must stay byte-identical (no CLI surface change) |

Fixture rule (`AGENTS.md` step 5): a fixture that passes without the
resolved env is worthless; check name collisions under `fixtures/repo/`
and the overlay first; assert on archive metadata / `environment.bz2`,
not phase stdout.

**Verification pass (step 8):** `cargo fmt --check`, `cargo clippy
--release --all-targets` (zero warnings), `cargo test --release`,
`python3 -m pytest tests -q`, then L2 porttest, L1 (with portage
upgrade), L0.

---

## 7. Risks, traps, stop rules

1. **Wrong `USE` is worse than missing `USE`** (G3 stop rule).
2. **Clobbering computed vars** — `extra_env` wins; `PORTUALE_COMPUTED`
   is the guard; test that a `make.conf` `HOME=`/`DISTDIR=` never reaches
   the phase.
3. **`depend` phase** must stay byte-identical (`--regen` goldens); the
   builder never runs for `depend`.
4. **Brush quoting** (G6) — unquoted `export CFLAGS=$(…)` executes.
5. **Fixture `config_root` has no `make.globals`** — seed explicitly; a
   test passing on the host and failing in CI is this.
6. **`environment.bz2` churn** is expected on source builds and forbidden
   on L1 prebuilt merges.
7. **Version skew** — `environ_whitelist` contents differ across portage
   versions; record the image's portage (3.0.82.2) with every run.
8. **Determinism direction** — every change moves a var from process env
   to resolved config, never the reverse.
9. **Scope pull** from #38/#39 — land the plumbing, not their features.
10. **Stop rule (real set):** if jq still dies on an env-shaped error
    after S2, file the exact variable/consumer with a repro; no ad-hoc
    exports. **Stop rule (S3):** growth past the inventory → #37b.

---

## 8. Review checklist (attach to each slice)

- `depend`-phase env byte-identical?
- `extra_env` still the last (winning) layer?
- Any var moved *from* resolved config *to* process env? (Forbidden.)
- `USE` = enabled ∩ `IUSE_EFFECTIVE` everywhere (phase env **and**
  `metadata/USE`); `use_flags_display` only for `--pretend` output?
- Set-but-empty exported, unset omitted?
- Every new env pair's code comment cites `config.py`/`doebuild.py`/
  profile source?
- Brush values shell-quoted?
- Rust unit + e2e added; no Python mirror / contract `CASES` touched?
- `TEST/findings/l2.md` evidence appended with exact commands?

## 9. Definition of done

- [ ] S0 table complete; every row cited and routed.
- [ ] G1-G7 answered and recorded in §4.
- [ ] Porttest track: all env-caused diffs gone; `l2-bpkgonly-env`
      allowlist rows deleted.
- [ ] `--buildpkgonly` ≡ `-b` env-shaped metadata.
- [ ] Oniguruma → `/usr/lib64`; jq configures (commands in `l2.md`).
- [ ] `SOURCE_DATE_EPOCH` reaches the phase from config (L3 G0.6).
- [ ] No undocumented `std::env::var("FEATURES")` on the build path.
- [ ] Full verification pass green; L1/L0 unchanged apart from fixed rows.
- [ ] Docs updated (S5); no dead allowlist entries.

## 10. Delegation brief (for subagents)

Hand over: this file as scope authority; project + generation from
`list_projects`/`index_status`; the G1-G7 answers (a subagent never
re-decides G1/G3 silently); §2.2's exact symbols; the oracle path
(§2.1); the rule that any negative claim about real is checked in
`3rdparty/portage/`, not inferred; the evidence bar (§4) and test matrix
(§6); `TEST/findings/l2.md` for repro commands. Expected return per
slice: diff, commands + results, archive/env evidence, new findings with
repros. No container → implement + unit-test S1-S3, label **unverified
end-to-end**.

## 11. Findings filed while executing

### S0 — 2026-09-13 (recon only; table in `TEST/findings/l2.md`)

- `l2-env-setup-full-dump` — real exports the accumulated setup-phase
  full config dump (160 keys vs portuale's 45); a whitelist-only builder
  cannot pass the `environment.bz2` acceptance row. Root cause: real
  imports the whole process env (`config.py:551,567`) and only filters
  from phase 2 on. Fix ref: §4 G1 (revised) / S1/S2. Owner decision open.
- `l2-env-aa-exported` — portuale exports `AA` (empty) on EAPI 8 where
  real pops it (`config.py:3331-3333`, `eapi_exports_AA` false for
  EAPI ≥ 4). Fix ref: #37 S2 (drop `AA`; EAPI floor 5+ means always).
- `l2-env-o-exported` — portuale exports `O` (the ebuild dir) where
  real's `environ_filter` drops it (`special_env_vars.py:300`). Fix ref:
  #37 S2 (add to the builder filter / drop from `extra_env`).
- Note (pre-existing, not this item): real's later phases re-export only
  `environ_whitelist`, so an ebuild `unset` persists across phases;
  portuale's fresh shell per phase re-adds everything. File separately
  if a fixture ever shows it (bug 189417 behaviour).

### S1 — 2026-09-13 (builder landed, not threaded)

- Landed `rust/portage-profile/src/phase_environ.rs`: `ENVIRON_FILTER`,
  `ENV_BLACKLIST`, `PORTUALE_COMPUTED` (transcribed/cited), `portage_use`
  (enabled ∩ (IUSE ∪ `IUSE_EFFECTIVE`), sorted) and `phase_environ(&Config,
  Option<PhaseUse>)` = `(other_vars ∪ whole process env) − filter −
  blacklist − computed`, then folded incrementals (`FEATURES` +
  `PORTAGE_FEATURES`, `ENV_UNSET`, `USE_EXPAND*`, `IUSE_IMPLICIT`) and
  the per-package `USE`/`IUSE_EFFECTIVE`/`USE_EXPAND`-derived values
  (`config.py:2218-2252`); plus `config_env_all()` (test-overridable
  whole-env accessor) in `lib.rs`. Four unit tests; workspace clippy 0
  warnings; `cargo test --release` green; pytest 1561 passed with only
  the 4 pre-existing no-TTY `--ask` failures (same set without the
  change).
- Correction to S0/plan text: real's `environ_filter` contains
  `PORTAGE_USE`, so only `USE` is exported (not `USE` + `PORTAGE_USE`).
- Correction to the synthetic-profile assumption: `ELIBC`/`KERNEL` must
  be in `USE_EXPAND` (as real `base/make.defaults` has them) for
  `iuse_effective` to contain `elibc_glibc`/`kernel_linux` — a fixture
  that omits them silently drops the implicit flags from `USE`.
- Design note for S2: `phase_environ` takes the package's `IUSE` +
  enabled set because real derives `IUSE_EFFECTIVE` and every
  `USE_EXPAND` variable value per package (oracle: `PYTHON_SINGLE_TARGET=""`
  for `emptydirs` despite the `*/*` `package.use` entry). `pkg == None`
  (standalone `ebuild <file>`) exports placeholders only.
- Not yet covered (S2): `PORTAGE_COMPRESSION_COMMAND` (dynamic,
  `doebuild.py:750`), `SLOT`/`PORTAGE_REPO_*` per entry, dropping `AA`/`O`
  from `run_commands`' own `extra_env`, brush quoting.

### S2 — 2026-09-13 (threaded; L2 porttest green)

- `entry_build_env`/new `entry_phase_env_tail` thread
  `phase_environ` per entry: resolved `USE`/`IUSE_EFFECTIVE`/
  `USE_EXPAND` from `candidate_effective_use_flags` + `portage_use`,
  plus `SLOT`/`PORTAGE_REPO_NAME`/`PORTAGE_REPO_REVISIONS`; the merge
  path via `MergeOptions::resolved_config` (Arc), `--buildpkgonly` via
  a `config` param into `run_buildpkgonly` →
  `run_package(build_env, use_flags)`. `run_commands` no longer exports
  `AA`; `phase_env_vars` no longer sets `O`; brush exports are
  single-quoted (`shell_single_quote`, G6).
- L2 porttest (`L2_REBUILD=1`, `TEST/logs/l2-20260913T131426Z`):
  `.keep_porttest_emptydirs-0`, `metadata/USE`/`FEATURES`/`repository`
  match real; `ecompress` + `estrip`/`splitdebug` now fire (docs carry
  `BIG.txt.bz2`, splitdebug the `/usr/lib/debug`+`.build-id` tree);
  0 unexplained, cross-install 0/0.
- Residuals filed (not this slice): `l2-env-pkg-vars-unexported`
  (export-state of `DEFINED_PHASES`/`KEYWORDS`/`LICENSE`),
  `l2-env-compression-command` (`PORTAGE_COMPRESSION_COMMAND`),
  `l2-env-profile-only-vars` (a `${VAR}` substitution in
  `PROFILE_ONLY_VARIABLES`), `l2-env-path-aclocal` (phase `PATH`
  order); and the fixture-compiled `.debug`/`.build-id` payload bytes
  (now allowlisted under `l2-gpkg-dostrip-splitdebug`, #38's strict
  payload question).
- Correction found while threading: `candidate_effective_use_flags`
  must treat a missing `IUSE` cache key as an empty IUSE (the porttest
  md5-cache omits it), not as unreadable metadata — otherwise exactly
  the empty-IUSE packages keep `USE=""`.

(append future entries here as S3-S5 run: command, expected, actual,
root cause, fix ref / backlog id)
