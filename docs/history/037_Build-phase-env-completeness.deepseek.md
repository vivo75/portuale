# #37 — Build-phase env completeness — agent plan (deepseek draft)

> **Historical planning/investigation doc, retired 2026-09-15.** Its outcome is `docs/backlog-tasks.md`'s status line for this item; the extracted ground truth, gotchas and dead ends are in [`../recap-of-backlog-ops-2026-09-15.md`](../recap-of-backlog-ops-2026-09-15.md). Kept verbatim below for citation/provenance only.


Status: **plan only, not executed.** Written 2026-09-13 against `main`
@ `ec13936`. Backlog: `docs/backlog-tasks.md:73` (#37), `scope-backlog.md`
§K "Build phase env is a curated whitelist" (`:659-668`). Findings:
`TEST/findings/l2.md` §S3 `l2-bpkgonly-env` and §S5 "scope widened".
L3 contract: `docs/030_L3-source-build-parity.deepseek.md` §S3.1 +
gate **G0.5** / **G0.6** (`:357-367`, `:503-528`). This item is the
**producer prerequisite shared by L2's real set and all of L3**; nothing
L2-real or L3 can go green before it lands.

**Read first:** `AGENTS.md` (steps 1-9), `docs/agent-context.md` "Real
ebuild phase execution", `TEST/findings/l2.md` (S3 + S5 → the exact
evidence table), `docs/030_L3-source-build-parity.deepseek.md` §1.4
(traps 3-4) and §S3, `docs/029_portuale-as-builder.deepseek.md` §S3/S5,
then the module doc comments of `rust/portuale/src/{emerge_build.rs,
ebuild_phases.rs,ebuild_package.rs,pretend.rs}` and
`rust/portage-profile/src/lib.rs`. Real authority:
`3rdparty/portage/lib/portage/package/ebuild/config.py::environ()`
(`:3263-3350`), `_config/special_env_vars.py` (`environ_whitelist`
`:83`, `environ_filter` `:257`), `doebuild.py::doebuild_environment()`
(`:381-545`, `PORTAGE_REPO_NAME` at `:483`),
`_emerge/EbuildPhase.py::_setup_repo_revisions` (`:73-108`),
`bin/phase-functions.sh:769`. Run `graft ask "build phase environment
resolved config"` before grepping.

Model tiers (same convention as 022-030):

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

**Do #37 first, and treat it as one focused slice with a hard semantic
core — not a bag of small variable fixes.** The finding is not "six
missing variables"; it is an architectural asymmetry:

- real portage runs phases with `config.environ()` — *everything* the
  resolved config says, filtered only by `environ_filter` and the
  calling-env whitelist;
- portuale runs phases with a hand-curated base env
  (`ebuild_phases.rs:1387` `ENVIRON_WHITELIST`, `:1950`
  `phase_env_vars`) plus a `MergeOptions::build_env` that currently
  carries only nine compiler/make flags (`pretend.rs:5948` `BUILD_VARS`
  → `:6002` `build_config_env`) and the entry's enabled-IUSE-only `USE`
  (`emerge_build.rs:516` `build_use_env`).

The concrete symptoms are all one bug: `USE` lacks implicit profile
flags (`amd64 elibc_glibc kernel_linux`), `FEATURES` is the raw process
env instead of the resolved incremental list, `MULTILIB_ABIS`/
`DEFAULT_ABI`/`LIBDIR_*` never reach `toolchain-funcs::get_libdir`
(so oniguruma installs to `/usr/lib` and `app-misc/jq`'s `econf` cannot
find it), `SLOT` is unset (`.keep_porttest_emptydirs-` instead of
`-0`), `PORTAGE_REPO_NAME`/`PORTAGE_REPO_REVISIONS` are unset (so the
gpkg `repository`/`REPO_REVISIONS` members and the vdb `repository`
file are never written), and `--buildpkgonly` threads **no**
`build_env` at all (`emerge_build.rs:152` →
`ebuild_package.rs:448` → `run_commands(..., &[])`).

Everything else about the "L2 real set blocked at jq" story follows
from this. #38 (packaging transforms) cannot even be *triggered* until
the resolved `FEATURES`/`PORTAGE_COMPRESS` values reach the phase env,
so #37 strictly precedes it.

### 0.2 Frontier or cheap model?

**Frontier for the semantic core; mid-tier for the mechanical
threading; frontier for the real-set verification.** The split follows
the failure modes:

- the dangerous work is *deciding what real `config.environ()` means*
  for this codebase — which config fields become strings, which of
  portuale's own computed path vars must win over a same-named config
  key, when `ENV_UNSET`/`environ_filter` remove something, and whether
  Rust-side `FEATURES` gates (isolation, buildpkg, multi-instance) also
  switch to the resolved list. Getting this wrong silently changes
  every saved `environment.bz2` (L1/L2/VDB diffs) and can poison the
  contract suite;
- once §4's gates are answered and the interfaces frozen, slices B-D
  are mechanical threading, reviewable, and can be executed by a mid
  model with the tests below;
- the L2-real bring-up (does `jq` configure? does `get_libdir` pick
  `lib64`? do multi-instance archives gain `BUILD_ID`?) is again
  frontier triage.

| Slice | Content | Tier | Why |
|---|---|---|---|
| A | `resolved_environ()` builder + pure unit tests | **F** | the semantic core; every later slice consumes it |
| B | thread into `emerge <atom>` source path + phase env | M (F review) | mechanical once A is gated |
| C | thread into `--buildpkgonly`/`run_package` | M | narrow, one call chain |
| D | resolved `FEATURES` for Rust gates (G1.4) | **F** | execution-model semantics, not string plumbing |
| E | real-set verification + allowlist cleanup | **F** | cross-language triage, evidence bar |
| F | docs closeout | S/M | mechanical |

If only a cheap model is available it can do B/C/F after A and the
gates; A/D/E must be frontier, or the result will be a build env that
*looks* resolved and is subtly wrong.

### 0.3 Difficulty, by axis

| Axis | 1-5 | Notes |
|---|---|---|
| Config-resolution semantics | 4-5 | `config.environ()` is the spec; portage-profile already resolves the data, but the export/filter/override rules are subtle |
| Rust plumbing | 2 | the call chains are short and already carry `build_env` on one side |
| Bash/Ebuild reality | 4 | what actually consumes the vars (`get_libdir`, `econf`, `phase-functions.sh`, multi-instance) only proves out in a container |
| Verification design | 3-4 | unit tests pin the map; only L2/L3 prove the outcome |
| Environment | 3 | one container run ≈ minutes; the porttest track is fast |
| **Overall** | **4** | small diff, deep semantics |

---

## 1. Ground truth

### 1.1 What exists today (verified against `ec13936`)

**The two call paths into phase execution:**

1. `emerge <atom>` source build/merge:
   - `pretend.rs:11705` `merge_options.build_env = build_config_env(&config)`
     (resume path `:4043-4050`; binpkg path `:11705`), then
     `emerge_build.rs:500-501` `per_entry.build_env = entry_build_env(options, entry)`
     → `ebuild_merge.rs:2719-2729` `run_commands(..., &options.build_env)`;
     scheduler path does the same at `emerge_build.rs:858-869`;
     `--getbinpkg`/`run_merge_plan` at `:926-942`.
   - `entry_build_env` (`emerge_build.rs:558-566`) = `options.build_env`
     (nine `BUILD_VARS`, `pretend.rs:5948-5951`) + per-package
     `package.env` vars + `USE` (enabled IUSE only, `:516-528`).
2. `--buildpkgonly`:
   - `pretend.rs:11750` `run_buildpkgonly(entries, &repos, &root,
     &portage_tmpdir, &package_options, keep_going)` — **no config, no
     `build_env`**;
   - `emerge_build.rs:118-180` `run_package(&path, root, portage_tmpdir,
     options)` at `:152`;
   - `ebuild_package.rs:448-474` `run_commands(ebuild_path, &["install"],
     ..., &[])` — empty `build_env`;
   - `package_after_install` (`:487+`) then runs `__dyn_package` with
     `use_flags = ""` on this path (`:473`) and `compute_environment`
     only (`:501`).

**The phase env builder** (`ebuild_phases.rs`):
- `compute_environment` (`:397`) computes paths/identity only.
- `phase_env_vars` (`:1950-2137`) base vars; `FEATURES` at `:2043`
  comes from `phase_features_value()` (`:1581` = raw `std::env::var("FEATURES")`),
  `USE` at `:2055` from `phase_standalone_base_env` (`:841-931`,
  config-resolved **only for standalone `ebuild` runs**), `extra_env`
  extends last at `:2125` (so it overrides).
- Bash backend: `run_one_phase_bash` (`:2390`) does
  `cmd.env_clear(); cmd.envs(process env ∩ whitelist); cmd.envs(vars)`.
- Brush backend: `phase_setup_script` (`:2149-2190`) unsets
  non-whitelisted inherited vars, then `export`s `vars`.
- Standalone base (`:841-931`) already resolves `candidate_use_flags_display`
  + `build_config_env` from a freshly-loaded `portage_profile::Config` —
  **the machinery exists; it is just not on the `emerge` paths.**

**The resolved config already carries the data:**
- `portage_profile::Config` (`lib.rs:385`): `other_vars`
  (`:911`, all make.globals + make.defaults + make.conf + env scalars),
  `resolved_incremental(key)` (`:932`, folds `incremental_sources`),
  `use_flags` (`:386`), `use_expand` (`:676`),
  `use_expand_unprefixed` (`:687`), `use_expand_implicit` (`:696`),
  `iuse_implicit` (`:701`), `iuse_effective` (`:716`), `archlist`
  (`:761`), `package_env_vars` (`:606`).
- `config_features_list`/`config_features_string` (`pretend.rs:5973-5990`)
  already produce the resolved `FEATURES` list; `MergeOptions::features`
  already receives it (`:4043`, `:11714`), but the **phase env** does not.
- `build_config_env` (`pretend.rs:6002-6013`) exports only
  `BUILD_VARS` = `CFLAGS CXXFLAGS CPPFLAGS LDFLAGS FFLAGS FCFLAGS ASFLAGS
  MAKEOPTS CHOST CBUILD CTARGET` (`:5948-5951`).

**Reference semantics (real portage):**
- `config.environ()` (`config.py:3263-3350`) iterates **every string
  config value**, skips `environ_filter` (`special_env_vars.py:257`),
  and when `$T/environment` exists only exports `environ_whitelist`
  (`:83`) + `CCACHE_/DISTCC_/QEMU_` regex names. It then forces
  `PORTAGE_FEATURES = FEATURES`, `USE = PORTAGE_USE` (i.e. the
  *filtered* effective USE), and pops `AA`/`MERGE_TYPE`/`ESYSROOT` per
  EAPI.
- `doebuild_environment()` (`doebuild.py:381-545`) sets the dynamic
  vars (`D`/`ED`/`T`/`WORKDIR`/`HOME`/`PORTAGE_BUILDDIR`/`FILESDIR`/
  `PORTAGE_BASHRC`/`PORTAGE_COLORMAP`/…), `PORTAGE_REPO_NAME` at
  `:483`, and `PORTAGE_REPO_REVISIONS` via
  `EbuildPhase._setup_repo_revisions` (`EbuildPhase.py:73-108`,
  `json.dumps(sort_keys=True)`), consumed by
  `phase-functions.sh:762-769` (`repository`/`REPO_REVISIONS`
  build-info files).
- `bin/misc-functions.sh:239-250` and `:147-152` gate dostrip/
  ecompress on the **`FEATURES` value in the phase env** — this is why
  #38 is blocked here (see `docs/038_Packaging-transforms.deepseek.md`).

### 1.2 The observed gaps (evidence, not theory)

From `TEST/findings/l2.md` S3/S5 (real vs portuale, same fixtures,
`--buildpkgonly` and `-b`):

| symptom | real | portuale | consequence |
|---|---|---|---|
| `metadata/USE` | `abi_x86_64 amd64 elibc_glibc kernel_linux` | `abi_x86_64` or empty | wrong USE everywhere |
| `metadata/FEATURES` | resolved incremental list | raw process env | `binpkg-*`/`splitdebug`/`sandbox` gating wrong |
| installed libdir | `/usr/lib64/...` | `/usr/lib/...` | `jq` can't find `oniguruma.pc` |
| `.keep_*` | `.keep_porttest_emptydirs-0` | `...-` (no `SLOT`) | vdb + archive path drift |
| `environment.bz2` | resolved env | bare env | every metadata diff upstream |
| `PORTAGE_REPO_NAME` | `porttest` | unset | no `repository` file, no gpkg `repository` |
| repo revisions | `{}` | unset | no `REPO_REVISIONS` member |

Repro (from `TEST/findings/l2.md` S5, container):
`L2_REBUILD=1 L2_MODE=payload-tolerant L2_BUILD_MODE=deep
TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-merge.txt` — portuale
builds 13 real packages, then dies at `app-misc/jq` configure
(`Package 'oniguruma' not found`).

---

## 2. Scope

**In scope (this item):**

1. A resolved-environ builder that mirrors real `config.environ()`
   semantics from `portage_profile::Config`.
2. Threading it into both execution paths (`emerge <atom>` and
   `--buildpkgonly`), including `emerge --resume`'s source path.
3. Per-entry additions: resolved effective `USE` (IUSE ∪ implicit),
   `SLOT`, `PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS`,
   `PORTAGE_FEATURES`, `USE_EXPAND`-derived vars.
4. Resolved `FEATURES` in the phase env, and (G1.4) in the Rust
   execution gates that currently read `std::env::var("FEATURES")`.
5. `SOURCE_DATE_EPOCH` (L3 G0.6): it is a config scalar, so it must
   come through the same builder, not an ad-hoc whitelist entry.

**Out of scope (do not touch here):**

- gpkg metadata members (`SIZE`/`IUSE`/`IUSE_EFFECTIVE`/`REQUIRES`/…)
  and `NEEDED.ELF.2`'s ELF-class field — **#39**.
- Actually producing `.debug`/compressed docs — **#38** (this item only
  makes its triggers visible).
- `package.env`/`bashrc` hooks beyond what already exists.
- Full `userpriv`/`fakeroot` execution-model parity.
- A config-writing mode; `--autounmask-write` stays a non-goal.

---

## 3. Gates (decide before writing code)

- **G1.1 — Environ scope.** Recommendation: **full real semantics**:
  every string value in the resolved config, minus `environ_filter`,
  minus keys portuale computes itself (see G1.3). A curated whitelist
  will fail again on the next real package (`LDFLAGS` is in today; the
  next package needs `RUSTFLAGS`, `PKG_CONFIG_PATH`, `MULTILIB_*`, a
  `USE_EXPAND` var, …). `environment.bz2` parity is the verification
  target and it wants the real set.
- **G1.2 — Where the builder lives.** Recommendation:
  `portage_profile::resolved_environ(&Config) -> Vec<(String, String)>`
  for the run-wide part (pure, unit-testable without the binary), plus
  `emerge_build::entry_environ(config, entry, repos, metadata)` for the
  per-package part. `pretend.rs::build_config_env` becomes a thin
  wrapper or is replaced at both call sites. Do **not** put the builder
  in `ebuild_phases`: that module must stay usable by standalone
  `ebuild <file>` with no config.
- **G1.3 — Ownership/override rule.** Portuale already computes dynamic
  path/identity vars in `phase_env_vars` (`D`, `ED`, `T`, `WORKDIR`,
  `HOME`, `PORTAGE_BUILDDIR`, `FILESDIR`, `P`, `PN`, `PV`, `PR`, `PVR`,
  `CATEGORY`, `PF`, `EAPI`, `ROOT`, `EROOT`, `PATH`, `FINDIR`…). The
  builder must exclude these so a config scalar with the same name
  cannot clobber them (`extra_env` wins over the base list). Write the
  exclusion as a named constant with a doc comment citing
  `doebuild_environment()` as the setter for each, and unit-test it.
- **G1.4 — Rust `FEATURES` gates.** Recommendation: **yes, in this
  slice** for the execution model that L3 pins in `make.conf`
  (`docs/030` §1.4 trap 4): `sandbox`/`usersandbox`, the namespace
  sandboxes, `buildpkg`, `binpkg-multi-instance`, `binpkg-signing`,
  `buildpkg-live`, `nostrip`/`instrip` and the collision/preserve-libs
  merge features. There are only 13 `feature_token_present`/
  `feature_enabled` call sites; `MergeOptions::features` already carries
  the resolved list (`ebuild_merge.rs:356-363`). If the blast radius
  turns out larger than the call sites suggest, split it as **#37b**
  (filed, not silently skipped). *Open judgment call — re-open with the
  user if evidence disagrees.*
- **G1.5 — Standalone `ebuild <file>`.** No resolved graph, so no
  per-entry vars; keep `phase_standalone_base_env`'s current behavior
  but route it through the same builder so the two can't drift. `ebuild
  <file> package`/`merge` stay on the "no graph, no USE" precedent.
- **G1.6 — Filtering correctness.** Only string values are exported;
  real drops long values (`SRC_URI`, deps) via `environ_filter`. Do not
  export `SRC_URI`/`*DEPEND` (E2BIG bug 262647). Portuale's
  `ENVIRON_WHITELIST` (`ebuild_phases.rs:1387`) is a *calling-env* guard
  on the bash backend, not the resolved-set filter; keep it as the
  second line of defense and mirror the real `environ_filter` in the
  builder.

**Evidence bar:** A/C are accepted only when a container-built archive's
`metadata/USE`/`FEATURES` and `environment.bz2` match real's for the
same fixture, and `get_libdir` picks `lib64` on `amd64`. "It compiles
and `environment.bz2` is bigger" is not acceptance.

---

## 4. Slices

### Slice A — `resolved_environ()` builder (F, 4-8 h)

**Goal:** a pure function that turns `portage_profile::Config` into the
run-wide phase-env pairs, with tests that pin semantics, not just
coverage.

Steps:
1. Read `config.environ()` (`config.py:3263-3350`) and
   `special_env_vars.py` end to end; tabulate, in a doc comment, every
   rule: iterate `other_vars` + joined `incremental_sources` + computed
   `USE_EXPAND` vars; skip `environ_filter`; force
   `PORTAGE_FEATURES`/`USE`; drop `AA`/`MERGE_TYPE` per EAPI (portuale
   is EAPI 5+ and treats them uniformly, per `agent-context.md`).
2. Decide the exclusion set (G1.3) and document each entry with its
   `doebuild_environment()` setter.
3. Compute `USE_EXPAND`-derived vars from `Config::use_expand*`
   (`ABI=amd64` from `DEFAULT_ABI`, `LINGUAS=…`, etc.) and the
   implicit flags (`amd64 elibc_glibc kernel_linux`) into the `USE`
   string.
4. Export `PORTAGE_COMPRESS`/`PORTAGE_COMPRESS_FLAGS`/
   `PORTAGE_COMPRESS_EXCLUDE_SUFFIXES` (make.globals scalars — they fall
   out of G1.1, pin them in a test) and `PORTAGE_DOCOMPRESS` family if
   the resolved config carries them (arrays are `phase-helpers.sh:21-23`
   defaults; do not duplicate).
5. Unit tests (in `portage-profile`): a synthetic profile with
   `ARCH`/`ELIBC`/`KERNEL` + `USE_EXPAND_UNPREFIXED="ARCH"`,
   `MULTILIB_ABIS`/`DEFAULT_ABI`/`LIBDIR_amd64`, an incremental
   `FEATURES` stack with a `-token`, and `SOURCE_DATE_EPOCH`; assert
   the exact map: `USE` contains `amd64 elibc_glibc kernel_linux`,
   `FEATURES` is the folded list, `ABI`/`MULTILIB_ABIS`/`LIBDIR_amd64`
   present, excluded dynamic keys absent, `SRC_URI` absent.

**Exit:** unit tests green; a doc comment mapping each rule to its real
source line; an F review of the exclusion set.

### Slice B — `emerge <atom>` source path (M, 4-8 h; F review)

**Goal:** the resolved environ reaches `install` and every `src_*`
phase; the per-entry vars are added on top.

Steps:
1. Replace the `build_config_env` call at `pretend.rs:11705` (and the
   resume site `:4043-4050`) with the new builder.
2. Extend `emerge_build::entry_build_env` (`:558-566`) with per-entry
   pairs resolved from `GraphEntry` + the located candidate's md5-cache:
   `SLOT`, `PORTAGE_REPO_NAME` (entry `repo_name`, falling back to the
   candidate repo), `PORTAGE_REPO_REVISIONS` (`json` object; `{}` when
   no revision is tracked — real's empty case, see `EbuildPhase.py:73-108`),
   and the **effective `USE`**: `build_use_env` (`:516`) must become
   the full resolved enabled set (IUSE enabled ∪ implicit ∪
   `USE_EXPAND`-derived flags), not just IUSE-declared. Reuse
   `portage_repo::candidate_use_flags_display`'s inputs (the resolver
   already computed `entry.use_flags_display`; the implicit part comes
   from the config), or extend the graph entry — prefer the smallest
   change that keeps `USE` identical to what
   `write_post_install_metadata` needs for #39.
3. Make `phase_env_vars` prefer an explicit `FEATURES` pair over the
   raw process env (`phase_features_value()` at `:1581`) so all three
   backends (bash, brush, misc-functions) see the resolved list. Keep
   the raw-env read as the standalone fallback.
4. Tests:
   - Rust unit: `entry_build_env` includes `SLOT`, repo name, full
     `USE`; exclusion set still wins over a same-named config key.
   - Rust e2e (container or `test_portuale.py`): build
     `porttest/emptydirs` + a compiled fixture from source; assert the
     archive `metadata/USE` contains the implicit flags, `.keep_*`
     carries the slot, and `NEEDED.ELF.2` paths start `/usr/lib64`.
   - `python3 -m pytest tests -q` must stay green (no behavior change
     on the contract surface).

**Exit:** the L2 porttest track loses the `l2-bpkgonly-env` conflation
on the `-b` path; oniguruma installs to `/usr/lib64`.

### Slice C — `--buildpkgonly` / `run_package` path (M, 3-6 h)

**Goal:** no path runs a build with an empty `build_env`.

Steps:
1. Give `PackageOptions` (or `run_package`/`run_buildpkgonly`) a
   `build_env: Vec<(String, String)>` sourced from the same builder at
   the `pretend.rs:11750` call site, where `config` is in scope.
2. `ebuild_package::run_package` (`:448-474`) must pass it to
   `run_commands` instead of `&[]` and use the effective `USE` for
   `package_after_install` instead of `""` (`:473`). This also fixes
   the `Packages` index `USE` field on this path.
3. `run_buildpkgonly` (`emerge_build.rs:118-180`) resolves the
   candidate before packaging, so build the per-entry pairs there (it
   already loads repos via `locate_candidate`); prefer a shared helper
   with slice B, not a second implementation.
4. Tests: Rust unit on the `run_package` → `run_commands` forwarding;
   e2e fixture archive built with `--buildpkgonly` gains `SLOT`/`USE`/
   `FEATURES` exactly as slice B's `-b` build.

**Exit:** `README`'s `l2-bpkgonly-env` rows stop reproducing; the
`--buildpkgonly` porttest archives are metadata-equal to the `-b` ones
(modulo `BUILD_TIME`/`BUILD_ID`).

### Slice D — resolved `FEATURES` in Rust gates (F, 4-8 h)

**Goal:** the execution model (isolation, binpkg behavior) is driven by
the same resolved list the bash env sees — required by L3's
determinism block living in `make.conf` (`docs/030` §1.4 trap 4).

Steps:
1. Inventory the 13 `feature_token_present`/`feature_enabled` sites
   (`ebuild_phases.rs:1550`, `pretend.rs:4132` + callers) and classify:
   phase-execution (sandbox/namespace — must resolve), resolve-time
   (`buildpkg`, `binpkg-multi-instance`, `binpkg-signing`,
   `buildpkg-live` — must resolve), or CLI-only (keep raw).
2. Where config is in scope, pass the resolved list down explicitly
   instead of reading `std::env`. Where it isn't (standalone `ebuild`),
   keep the process-env fallback.
3. Do not change the CLI-visible meanings (`--buildpkg=n` still wins,
   `-buildpkg-live` still negates).
4. Tests: unit — resolved `FEATURES="-sandbox"` disables the sandbox
   even with `sandbox` in the process env; `FEATURES=buildpkg` from
   `make.conf` produces a binpkg without `--buildpkg`.

**Exit:** L3's make.conf-based determinism block is honored
symmetrically; no leftover `std::env::var("FEATURES")` on the build
execution path without a documented fallback reason.

### Slice E — verification, allowlist cleanup, real set (F, 4-10 h + wall clock)

Steps:
1. `TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt` —
   delete every `KNOWN_FINDINGS` entry and `known-divergences.yaml`
   entry that is now a plain pass (`l2-bpkgonly-env`,
   `l2-bpkgonly-keep-contents`; keep #38's until its slice lands).
2. Re-run the real set (`L2_REBUILD=1 L2_MODE=payload-tolerant
   L2_BUILD_MODE=deep TEST/run/l2-portuale-builder.sh
   TEST/atomlists/l1-merge.txt`); record how far past `jq` it gets in
   `TEST/findings/l2.md` with the new stop point.
3. L3 sanity: `TEST/run/l3-source-parity.sh TEST/atomlists/l3-smoke.txt`
   if the bed exists (`docs/030` §S1); otherwise record that #37's
   `environment.bz2`/USE findings are gone by archive inspection.
4. Update `TEST/findings/l2.md`: mark `l2-bpkgonly-env` FIXED with the
   landing commit; keep a "residual" note for anything deliberately
   left (e.g. G1.4 split).

**Exit:** no allowlist entry for #37 anywhere; the L2 porttest track is
green from a clean run; the real set's next blocker is **not** env
completeness.

### Slice F — docs + backlog close (S/M, 2-3 h)

- `docs/what-this-proves.md`: append a new paragraph (never rewrite the
  prior ones) with a runnable, live-verified example — e.g. the
  container command that builds `porttest/docs` and greps the archive's
  `metadata/USE`/`FEATURES`.
- `scope-backlog.md` §K: delete/replace the "curated whitelist" bullet;
  `backlog-tasks.md:73`: mark #37 done (or narrow what remains).
- `docs/030_L3-source-build-parity.deepseek.md`: update the S3/G0.5
  status pointer so L3's next reader knows #37 landed.
- `docs/agent-context.md` "current state" memory note only if it
  changes what a next-session scoper must know (keep it one line).

---

## 5. Fixtures and tests

| What | Where | Notes |
|---|---|---|
| `porttest/emptydirs` | `TEST/images/overlay/porttest/porttest/emptydirs/` | isolates `SLOT` in `.keep_*` |
| `porttest/docs` | same tree | `dodoc` tree; later #38's compress target |
| `porttest/splitdebug` (+ `toolchain-funcs`) | same | `get_libdir` on `amd64`; later #38 |
| `app-misc/jq` / `dev-libs/oniguruma` | real set | the end-to-end acceptance |
| Rust unit tests | `rust/portage-profile/src/lib.rs`, `rust/portuale/src/{emerge_build,ebuild_phases,ebuild_package}.rs` | pure map + threading |
| Rust e2e | `rust/portuale` tests / `tests/test_portuale.py` | real-execution-only, no Python mirror |
| Python contract | `tests/test_emerge_pretend_contract.py` | must stay byte-identical (no CLI output change) |

Fixture rule (`AGENTS.md` step 5): a fixture that passes without the
resolved env is worthless — check name collisions under
`fixtures/repo/` and `TEST/images/overlay/porttest/porttest/` first.
Prefer asserting on `environment.bz2`/archive metadata over asserting
on a phase's stdout.

**Verification pass (step 8):** `cargo fmt --check`, `cargo clippy
--release --all-targets`, `cargo test --release`, `python3 -m pytest
tests -q`, then the L2 porttest container run. Periodically also L0
(this slice touches the build path, and L2-real is its own gate).

---

## 6. Risks, traps, stop rules

1. **`environment.bz2` hash churn.** L1 merges prebuilt pkgs, so it
   should not move; but `ebuild_merge`'s vdb environment regeneration
   (`PORTAGE_UPDATE_ENV`) does — re-run L1's porttest pair and treat
   any new diff as a finding, not noise.
2. **Double-export / clobbering.** `extra_env` extends last in
   `phase_env_vars` (`:2125`): a resolved config key named `HOME` or
   `DISTDIR` will overwrite portuale's computed value. The G1.3
   exclusion set is the guard; test it.
3. **`PORTAGE_USE` vs `USE`.** Real exports `USE =
   configdict["PORTAGE_USE"]` (filtered by IUSE/implicit IUSE), not the
   raw global set. If the effective set carries a flag the package
   doesn't declare, ebuilds will see a `use()` that real would have
   hidden. Pin `USE` = enabled ∩ (IUSE ∪ implicit) per package; this is
   also what `write_post_install_metadata`'s `*DEPEND` reduction needs.
4. **`ENV_UNSET`.** Present in `ENVIRON_WHITELIST`; real's `environ()`
   honors it. Read the real path before assuming "export everything".
5. **Brush vs bash.** Both backends consume `phase_env_vars`; if the
   resolved map grows a value with `$`/backticks, the brush
   `phase_setup_script` `export {name}={value:?}` is not shell-quoted
   (`:2139-2148`). Either shell-quote it or keep portuale-owned values
   out of the resolved map.
6. **Version skew.** The transform gates in #38 compare portuale's
   *vendored* `bin/misc-functions.sh` against the image's *installed*
   portage; for #37 the same applies to `phase-functions.sh`'s
   `REPO_REVISIONS` writer. Record the installed portage version with
   every run (`TEST/findings/l2.md` already does: 3.0.82.2).
7. **Stop rule.** If after slices A-C the L2 real set still dies before
   `jq` on an env-shaped error, stop and file the specific variable/
   consumer with a repro rather than widening the environ dump
   indefinitely. If G1.4's resolved-gates change grows beyond the 13
   call sites, split it as #37b and land A-C first.

---

## 7. Delegation brief (for subagents)

Before delegating any slice, the parent must hand over:

- project + generation/freshness from `list_projects`/`index_status`,
  and the fact that this plan is the scope authority;
- the exact files from §4 and the real-source anchors from §1;
- the G1 gates' answers as already decided, with any open judgment call
  called out explicitly (never let a subagent re-decide G1.1/G1.4
  silently);
- the evidence bar from §3 and the test matrix from §5;
- the instruction that any negative claim ("real never sets X") must be
  checked against `3rdparty/portage/` source, not inferred;
- a pointer to `TEST/findings/l2.md` for the current repro commands.

Expected return: per slice, the diff, the test commands run + results,
the archive/environment evidence, and any newly discovered gap with a
repro. A subagent that cannot run containers may implement and unit-test
A-C but must label the slice **unverified end-to-end**.
