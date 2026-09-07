# Solver backends: `portage-solver` / `portage-atom-pubgrub` / `portage-atom-resolvo` — inclusion analysis

New standalone document (deliberately **not** an edit to `agent-context.md` /
`scope-backlog.md` / `what-this-proves.md`, to avoid rebase conflicts with
`main`). Question: can the three external crates be included **as is**, or do
they need modifications? Invariant for either answer: **the Portage solver
stays functional**, selected at runtime by `--solver=${solver}`.

Grounding: crates.io + docs.rs as of 2026-09-07, `rust/mrg-director/src/lib.rs`,
`rust/portage-repo/src/lib.rs:11032-11109` (`ResolveRequest` / `Resolver` /
`active_resolver`), `rust/portuale/src/pretend.rs` CLI parsing.

## 1. What the three crates are

All three live in one upstream repo, `lu-zero/portage-cli` (MIT, same as
portuale-compatible), and are versioned on crates.io independently:

| Crate | Latest seen | Role | Depends on |
|---|---|---|---|
| `portage-solver` | 0.3.0 | Solver-agnostic vocabulary + `Solver` trait. Facts (`PackageRepository`, `VersionFacts`, `PackageDeps`, `DepClass`, `RequiredUse`), USE policy (`UseConfig`, `UseFlagState`, `IUseDefault`, `resolve_effective_use`), solution (`SelectedPackage`, `DepEdge`, `InstalledPackage`, `TargetSpec`, `Violation`, `Plan`), plus `trait Solver { add_installed(); resolve_targets() -> Plan; set_with_bdeps / set_prefer_newest_slot / set_prefer_update / set_rebuild_tree / set_cross_active / set_root_deps_rdeps / add_host_installed / add_sysroot_installed (all default no-op) }`. Depends **only** on `portage-atom` + `thiserror`. Knows nothing of pubgrub/resolvo. | `portage-atom ^0.11.1`, `thiserror ^2` |
| `portage-atom-pubgrub` | 0.8.0 | `portage-atom` ↔ PubGrub bridge. `PortagePackage` (Cpn+slot), `PortageVersionSet` (PMS ops → `Ranges<Version>`), `PortageDependencyProvider` over a repository: all five `*DEPEND` classes, `||`/`^^`/`??` as virtual choice packages, slot/sub-slot ops (`:=`, `:*`), hybrid USE-conditionals (eager for user-decided, virtual two-version nodes for solver-decided), USE-deps, `::repo`, installed favored/locked, blocker post-validation, labeled graph + toposort, `:=` rebuild + `upgrade_to` re-solve fixpoint. Canonical/richer API; its eventual `Solver` impl is documented as a thin translation. | `portage-atom ^0.11.1`, `portage-solver ^0.3.0`, `pubgrub ^0.4`, `thiserror ^2`, `version-ranges ^0.1` |
| `portage-atom-resolvo` | 0.8.1 | `portage-atom` ↔ resolvo (CDCL SAT) bridge. Same PMS coverage goals (version matching, transitive solve, newest-first, `||`→Union, USE-conditionals, blockers weak/strong, multi-slot, slot ops, sub-slot, `::repo`, USE-deps, DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND, arena interning, `InstalledSet` favored/locked, PDEPEND-relaxed toposort). Documents itself as best-effort subset of the pubgrub API. | `portage-atom ^0.11.1`, `resolvo ^0.12` |

Maturity caveat (their own READMEs, verbatim intent): **both bridges warn they
are largely AI-generated / slop-coded, unaudited, may contain bugs or
incomplete PMS coverage — use at own risk.** `portage-cli` itself says the
same about the workspace. Treat as experimental input, not as a vetted
upstream like `pubgrub`/`resolvo` themselves.

The mature pieces are the *engines underneath*: `pubgrub 0.4` (pubgrub-rs,
used by `uv`, generic over package/version/versionset) and `resolvo 0.12`
(mamba-org, CDCL SAT, used by `rip`/`rattler`/`pixi`). Those two are
well-tested; the Portage-specific mapping layers on top are the young part.

## 2. Hard-constraint check (portuale `agent-context.md`)

- **musl-static / pure Rust: PASS (with a waiver).** Transitive deps are pure
  Rust, zero C linkage: `pubgrub` pulls `indexmap`, `log`, `priority-queue`,
  `rustc-hash`, `thiserror`, `version-ranges` (+ `smallvec` under it);
  `resolvo` pulls `ahash`, `elsa`, `event-listener`, `futures`, `indexmap`,
  `itertools`, `petgraph`, `tracing` (async-std/tokio/human_bytes/tabwriter
  are optional features — verify default-features before enabling);
  `portage-atom` pulls `gentoo-interner`, `smallvec`, `smol_str`, `thiserror`,
  `winnow`. No glibc/dynamic dependency. The static story is untouched.
- **Near-zero-dependency discipline for the `emerge` path: NEEDS A WAIVER.**
  `emerge`/`ebuild` today hand-roll parsing and allow only documented
  exceptions (`brush`, `regex`, `md-5`, `filetime`, `libc`, each with a module
  doc-comment waiver). Pulling `portage-solver` + one bridge + `pubgrub` /
  `resolvo` + `portage-atom` + `thiserror` + the transitives above is ~10 new
  crates on the hot path — the same class of decision as the `clap`-for-`mrg`
  waiver (`scope-backlog.md` Part 2.H / Part 3). Precedent says: put the new
  deps behind the `mrg`-director seam (or a dedicated bridge crate), not
  directly in `portage-repo`/`pretend.rs`, and record the waiver in the
  bridge's module doc comment. `thiserror` specifically duplicates the
  workspace's hand-rolled `pub enum Error` posture (`refactor-01 S4`) — either
  map `thiserror` errors at the seam (`From<...> for String` / `portage_repo::Error`)
  or justify the new macro dependency.
- **Determinism: ADAPTER MUST ENFORCE.** The contract suite pins
  Rust==Python byte-identical output; the resolver is deterministic today
  (sorted traversals, no timing lines). PubGrub/resolvo version ordering,
  tie-breaks, and error derivation order must be pinned/normalized in the
  adapter — never leak `HashMap` iteration, timing, or engine-native error
  strings into output.
- **Dual-language lockstep: NEEDS A SCOPE DECISION.** AGENTS.md §4 requires
  `rust/…` + `python/emerge_pretend_reference.py` behaviourally identical,
  verified empirically. A Rust-only SAT solver has no Python mirror. Options:
  (a) `--solver=portage` (default) stays dual-language and contract-pinned;
  alternative solvers are **portuale-only** (same status as the `mrg` applet:
  Rust-only, `test_portuale.py`, no reference implementation); or (b) port a
  Python equivalent (infeasible for CDCL/PubGrub). (a) is the only workable
  reading — record it explicitly so the contract suite is not silently
  narrowed (jointly-owned suite rule).
- **Benchmarks must still show Rust ahead** — an adapter that shells out or
  re-parses per query would regress this; feed the bridges from already-loaded
  md5-cache/vdb structures, no per-atom subprocess.

## 3. Why "as is" does not work — the seam mismatch

Our seam (`portage-repo/src/lib.rs:11086-11109`):

```rust
pub trait Resolver { fn resolve(&self, req: &ResolveRequest) -> Result<GraphResult, Error>; }
pub fn active_resolver() -> Box<dyn Resolver> // today: always BacktrackingResolver
```

`ResolveRequest` owns **40+ fields** (config incl. full `USE_ORDER` chain,
every `package.*` file, autounmask levels, binpkg/binhost knobs, `--update` /
`--deep` / `--newuse` / `--changed-*` / `--with-*`, backtrack_max, rebuild
knobs, …). `GraphResult` is the merge list **plus** all Portage notices
(`SlotConflict` w/ pullers + `pkg_use_display`, autounmask keyword/USE/license/mask
change lists, abi rebuilds, circular deps, pprovided atoms, …).

Their seam (`portage-solver::Solver`):

```rust
fn add_installed(&mut self, pkg: InstalledPackage);
fn resolve_targets(&mut self, targets: &[TargetSpec]) -> Result<Plan, SolveError>
```

`Plan` is selected packages + labelled graph + install order + advisories
(dropped deps, ceded USE, flag requirements, violations) — **not** our
`GraphResult`. Consequences:

1. **A translation layer is mandatory, not optional.** A new crate (e.g.
   `rust/portage-solver-bridge`) must implement `portage_repo::Resolver` for
   each backend: `ResolveRequest` → external `PackageRepository` +
   `UseConfig`/`desired_use` + `InstalledPackage`s, then `Plan` →
   `GraphResult` (merge order via our `merge_order.rs`, notices
   reconstructed or explicitly cut). "One `impl Resolver` plus an
   `active_resolver` branch" (`mrg-director/src/lib.rs:53-73`) is exactly this
   work — the director already names it.
2. **Policy stays in the caller by their design.** Both bridges state the
   solver **never resolves USE policy** — the consumer computes
   fully-resolved USE (profile ∘ make.conf ∘ package.use ∘ IUSE-defaults) and
   hands it in via `desired_use`/`UseConfig`. Masking (keyword, license,
   PROPERTIES/RESTRICT, `package.mask`), slot-operator rebuild fixpoints,
   `--autounmask*` in-loop levels, backtracking masks, merge-list ordering,
   blocker/slot-conflict *notices* — all stay portuale-side. The bridges pick
   versions; portuale still decides visibility and explains failures in real
   `depgraph.py`/`output.py` language. Expecting engine-native conflict
   strings to replace `_show_slot_collision_notice` would be a behaviour
   break; map `SolveError` → our notices.
3. **Atom/version vocabularies duplicate.** We ship `portage-versions` +
   `portage-dep` (hand-rolled, no EAPI parametrization, no interner).
   The bridges speak `portage-atom 0.11` (`Cpn`/`Cpv`/`Dep`, interner,
   PMS-9 version ordering). Do **not** swap one for the other wholesale —
   that re-opens every contract test. Translate at the seam (our parsed
   atoms/versions → their interned types) and keep both parsers.
4. **Feature deltas between the two bridges.** PubGrub is the canonical model
   (`:=` rebuild + `upgrade_to` re-solve fixpoint, Level-C `REQUIRED_USE`
   auto-satisfaction via solver-decided flags, `format_no_solution`);
   resolvo is a best-effort subset (weaker conflict reporting per its own
   checklist). A single `Plan→GraphResult` mapping must therefore handle
   per-backend capability flags (e.g. ceded-USE advisories only from pubgrub),
   or the `--solver=resolvo` output will silently lack fields `--solver=pubgrub`
   provides.

## 4. `--solver=${solver}` runtime selection — proposed shape

Real `emerge` has **no** `--solver` flag, so this is a portuale-only extension
(like `emerge --shell bash|brush`): it must never collide with real-emerge
error strings.

- **Spelling:** `--solver=portage|pubgrub|resolvo`, default `portage`.
  Bare `--solver` (no `=value`) and `--solver <value>` (space form): reject or
  treat as usage error exit 2 (mirror the `--exclude=`/`--backtrack=` strict-`=`
  handling in `pretend.rs:6964-6993`, not the `insert_optional_args` integer
  family — a solver name is not an integer). Unknown value →
  `emerge: invalid --solver parameter: "<v>"`, exit 2.
- **Plumbing:** parse in `pretend.rs` (+ `emerge_options.rs` tables +
  `HELP_TEXT` + Python `emerge_pretend_reference.py` mirror for the *parsing*
  only), thread a `solver: SolverKind` field through `ResolveRequest`
  (default `Portage`), and branch in `active_resolver()`:
  `active_resolver(&req)` or `active_resolver(kind) -> Box<dyn Resolver>`
  returning `BacktrackingResolver | PubGrubResolver | ResolvoResolver`.
  `mrg`'s clap table (`mrg.rs`) gains the same `--solver=` value option and
  forwards `--solver=<v>` through `to_emerge_argv`.
- **Invariant:** `portage` remains the default and fully functional; the
  contract suite pins `--solver=portage` (and bare invocations) Rust==Python.
  Alternative solvers are Rust-only paths exercised by `test_portuale.py` +
  Rust unit tests in the bridge crate (same split as `--config`/`--regen`
  phase-output tests). Document the split wherever the flag is documented so
  a future reader does not mistake a Rust-only solver divergence for a
  contract break.
- **JSON/help:** `--help` documents the three values + default; `--json`
  provenance trace (if extended) tags which solver produced the plan.

## 5. Verdict and recommended slices

**Verdict: cannot be included as is; needs a bridge + waivers + CLI work.**
The engines (`pubgrub`, `resolvo`) and vocab (`portage-solver`) are
dependency-clean (pure Rust, MIT, musl-safe). The Portage mappings
(`portage-atom-pubgrub`, `portage-atom-resolvo`) are young, self-declared
unaudited, speak a different seam (`Solver`/`Plan`, not
`Resolver`/`GraphResult`), duplicate our atom/version crates, and leave all
Portage policy (USE resolution, masking, autounmask, notices, merge order) to
the caller. Dropping them in without an adapter would break determinism,
output fidelity, and the dual-language contract.

Recommended slices (each per AGENTS.md: fixtures + `CASES` + pinned tests +
  docs + full verify pass; commit/push only when asked):

1. **`--solver` CLI surface (no new deps).** Add `solver: SolverKind`
   (`Portage` default) to `ResolveRequest`, `--solver=` parsing + help +
   invalid-value errors in Rust **and** the Python mirror, `active_resolver`
   branching with only `BacktrackingResolver` wired, contract tests pinning
   default + `--solver=portage` equivalence and unknown-value exit 2.
   Proves the invariant (Portage solver functional, runtime-selected) before
   any third-party code lands.
2. **Bridge crate + `portage-solver` vocab (no solving yet).** New
   `rust/portage-solver-bridge` with dependency waiver doc-comment; implement
   `portage_repo::Resolver` stubs (`PubGrubResolver`, `ResolvoResolver`) that
   translate `ResolveRequest` → facts/USE/installed inputs and `Plan` →
   `GraphResult`, initially returning the Portage result or a clean
   "not yet implemented" error. Pins the seam translation without engine
   behaviour.
3. **PubGrub backend behind the flag.** Wire `portage-atom-pubgrub`
   (pin exact version), feed `desired_use` from our resolved USE, map
   `SolveError` → our slot-conflict/autounmask notices, normalize ordering.
   Rust-only tests (`test_portuale.py` + bridge unit tests); contract suite
   unchanged except the shared `--solver` parsing.
4. **Resolvo backend behind the flag** (same shape, noting its subset:
   document which advisories it cannot produce).
5. **Cross-check mode (optional).** `portage-solver`'s stated purpose is
   running both bridges behind one `Solver` trait and diffing `Plan`s — wire
   as a debug/diagnostic (`--solver=both`? or `PORTAGE_SOLVER_CHECK`), never
   as user-visible output.

Pins: exact crate versions in `Cargo.toml` (do not float `0.x`), default
features audited for `resolvo`, `cargo fmt --check` / `cargo clippy
--release --all-targets` zero-warn / `cargo test --release` /
`python3 -m pytest tests -q` per slice.

## 6. Direct `pubgrub 0.4` / `resolvo 0.12` vs. reusing lu-zero's bridges

Grounded in the pinned checkout (`3rdparty/portage-cli` @ `a0465cd`,
`repos.toml` `[portage-cli]`). Line counts via `wc -l` on that checkout.

**Verdict: reuse lu-zero's bridges. The direct route requires strictly more
plumbing and more effort — everything the direct route needs, plus a
~10k-line rewrite of what the bridges already implement.**

### What the direct route would force us to write

`pubgrub` and `resolvo` are generic engines with no Portage knowledge. Using
them directly means implementing their provider traits from scratch **and**
every Portage semantic on top:

- `pubgrub`: `Package` + `VersionSet` + `DependencyProvider`
  (`choose_package_version`, prioritization, `get_dependencies`, ...).
- `resolvo`: `Interner` + `DependencyProvider` (pool interning,
  `filter_candidates`, `get_dependencies`, `sort_candidates`, ... —
  see `portage-atom-resolvo/src/provider.rs`, 1541 lines just for this trait).

Then the Portage semantics neither engine owns: PMS version ranges (`~`,
`=*`, slot/sub-slot), all five `*DEPEND` classes, `||`/`^^`/`??` as virtual
choice packages, hybrid USE-conditionals (eager + solver-decided virtual
nodes), all six USE-dep variants, `::repo`, weak/strong blockers, installed
favored/locked, multi-slot coexistence, `:=` rebuild tracking +
`upgrade_to` re-solve fixpoint, Level-C `REQUIRED_USE`, post-solve validation
(`validate.rs`: 1978 lines), labeled graph + PDEPEND-relaxed toposort
(`graph.rs`: 1527 lines). That is exactly what the two bridges contain:

- `portage-atom-pubgrub/src`: ~6.3k lines (`convert.rs` 1625, `graph.rs`
  1527, `validate.rs` 1978, `package.rs` 395, `version_set.rs` 374,
  `provider/` 2.5k incl. 73 tests, `solver_impl.rs` 388).
- `portage-atom-resolvo/src`: ~4.3k lines (`provider.rs` 1541, `pool.rs`
  568, `version_match.rs` 316, 49 solver tests).

The direct route re-types all of it, plus a new test suite proving PMS
parity — the most error-prone part, and the part lu-zero already debugged
(incl. a post-solve ordering nondeterminism fix recorded in
`portage-atom-pubgrub/docs/use-and-solver-boundary.md`).

### What reusing lu-zero still leaves us (unavoidable in both routes)

The caller-side plumbing is identical either way, because the engines never
speak Portage policy (their documented boundary: the solver is "a solver over
facts"; resolved USE, masking, notices all stay with the caller):

- `ResolveRequest` (40+ fields) → bridge `PackageRepository` + per-version
  `desired_use` + installed packages — same input mapping whether the
  provider underneath is theirs or ours.
- `Plan` → `GraphResult` (merge order via `merge_order.rs`, slot-conflict /
  autounmask / abi / circular notices reconstructed) — same output mapping.
- `portage-atom` ↔ `portage-dep`/`portage-versions` translation at the seam
  (do not swap parsers wholesale).

So the comparison is: **lu-zero route = caller adapter + audit** vs.
**direct route = caller adapter + ~10k-line bridge rewrite + test suite**.
The adapter cost is fixed; the bridge cost is saved in full.

### Bonus reference: `portage-resolve` (18.5k lines, do not depend on)

`portage-cli/portage-resolve/src` (lib + 19 modules) is lu-zero's own
caller-side adapter: `repo.rs` (3822 lines: `PackageRepository` impl +
keyword/mask/license acceptance), `conflicts.rs` (1432: post-solve
reverse-dep conflicts vs. installed), `effective_use.rs`, `force_mask.rs`,
`package_use.rs`, `roots.rs`, `root_closure.rs`, ... — i.e. a worked example
of the exact `ResolveRequest`-equivalent → facts mapping we must build
against our own config/vdb types. It is explicitly unpublishable past
`v0.0.1` (depends on their `portage-repo` → brush fork via git), so **copy
ideas, never add a Cargo dependency on it**.

### Caveats of the lu-zero route (audit list, not blockers)

- Self-declared unaudited/AI-generated bridges: pin exact versions, review
  `convert.rs` + `validate.rs` + ordering-sensitive paths, run their 120+
  tests plus our fixture suite before trusting output.
- Asymmetry: only the pubgrub bridge implements `portage_solver::Solver`
  (`solver_impl.rs`, 388 lines, thin translation — `grep "impl Solver"`
  finds no counterpart in the resolvo crate). `--solver=resolvo` needs either
  that same thin impl written once, or driving `PortageDependencyProvider`
  directly. Small, bounded work — not a reason to go direct.
- Their `use-and-solver-boundary.md` confirms the policy split we rely on:
  profile/make.conf/package.use/ACCEPT_* resolution lives in the caller, the
  bridge only consumes per-version `desired` sets. Our `effective_use_flags`
  already computes exactly that — the feed exists.
