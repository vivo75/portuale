# Agent context: portuale, a Rust reimplementation of Portage

This file is the single entry point for (re)deriving where portuale
stands and what to do next: goals, hard constraints, architecture
decisions, the phase-execution/bash-backend findings, and pointers to the
live state + open backlog. The original porting-strategy prompt is
[`history/porting-strategy-prompt.md`](history/porting-strategy-prompt.md);
the session-to-session operating rhythm is [`../AGENTS.md`](../AGENTS.md).

As with any settled decision below: if you disagree, say so explicitly
and re-open it — don't silently override it.

## Context

Portage (this repository) is the Gentoo package manager, written in Python.
Portuale is a Rust reimplementation of it, developed as a **friendly
fork**: a separate, cooperating codebase, not a hostile competitor. It is
a **working package manager** — it resolves, builds, merges, and unmerges
real Gentoo packages — and the aim is a **drop-in, same-behaviour
replacement** (and then some), reached one reviewed, contract-tested
slice at a time. `scope-backlog.md` Part 2 is the honest list of what
real portage still does that portuale doesn't.

**EAPI floor**: EAPI 0, 1, 2, 3, 4, and 6 are deprecated and removed in
this repo/fork — no ebuild uses them, and all profiles are EAPI 5 or
higher (5, 7, 8 are the live versions). Any EAPI-conditional logic being
read or ported only needs to account for EAPI 5+ as the real, live
baseline — branches that only apply to EAPI 0/1/2/3/4/6 are dead code and
can be ignored rather than faithfully ported. (Portuale's own `portage-*`
crates go further, as a deliberate simplification confirmed with the user:
no EAPI parametrization at all within the 5+ floor — every EAPI in that
range is treated identically. See `what-this-proves.md` for the many places
this precedent is invoked.)

## Team structure

- A Python team continues to own and evolve the existing Python codebase.
- A separate Rust team builds and owns the Rust implementation.
- The two teams work independently, each writing idiomatic code in their
  own language. Do not force Rust to mimic Python's structure line-for-line
  — that trades away idiomatic Rust for a cosmetic diffability that doesn't
  hold up in practice.

## Hard goals (non-negotiable)

1. **Portability of change, not of source.** After the initial port, a
   behavior change made in either codebase must be reproducible in the
   other. The mechanism for this is a **shared, jointly-owned Python
   test suite acting as an executable behavioral spec** — not structural
   mirroring of source code. A change lands with new/updated test cases;
   the other implementation is "in sync" when it passes them, regardless
   of how differently it's implemented internally.
2. **Rust must be measurably faster than Python**, not just assumed faster
   because it's Rust. This must be proven by benchmarks, tracked over time
   in CI as a regression gate (not a one-time claim).
3. **The Rust binary must run on a minimal Linux system**: statically
   linked (musl target), zero dynamic runtime dependencies, no assumption
   of glibc or a package manager being present. Prefer pure-Rust
   dependencies; avoid dynamically-linked C libraries.
4. **Tests are written in Python for both implementations.** Black-box,
   driven via CLI/subprocess against each implementation's executable(s)
   — not white-box bindings into Rust internals. This keeps the contract
   suite implementation-agnostic and neutral to whatever the long-term
   architecture turns out to be.

## Open / deliberately undecided

- **The end state is a real, complete, usable Portage** — a drop-in
  same-behaviour replacement, and then some. What's still open is whether
  it *replaces* Python Portage in Gentoo or stands permanently alongside
  it (like `uutils` vs GNU coreutils). Do not pick an architecture that
  forecloses either: subprocess/CLI-based testing keeps both open,
  in-process FFI embedding (e.g. PyO3) would not — avoid it.

## Scope

1. **Core library**: version comparison (`portage.versions`), atom/dep
   parsing and matching, config resolution, dependency graph (depgraph).
2. **`emerge` and `ebuild` executables** — both dry-run resolution
   (`--pretend`, `--json`) and real execution: ebuild-phase execution,
   filesystem-mutating merge / unmerge / package / config, binpkg
   build+merge, the parallel build scheduler, world-file management. See
   "Real ebuild phase execution + filesystem merge" below and
   `what-this-proves.md`.

### `emerge`/`ebuild` binary shape

Ship `emerge` and `ebuild` as **one multicall binary** (busybox-style),
dispatching behavior based on `argv[0]` via symlinks/hardlinks pointing at
a single executable. This is both a good minimal-Linux fit (one static
binary, no duplicated code) and drop-in compatible with tooling that
invokes `emerge`/`ebuild` by name directly. **Shipped**: `rust/portuale`.
A bare `portuale` (or `portuale --help`/`-h`) lists the applets with a
one-line description and exits 0; an unrecognized applet name still
errors. `emerge --help` is a grouped tour of every action/option portuale
implements (`pretend.rs`'s `HELP_TEXT`, mirrored in
`emerge_pretend_reference.py` and pinned by the contract suite).

## Test/benchmark harness architecture

- For pure-library-level parity (versions, atom parsing, etc.), define a
  neutral **CLI test-harness binary** on each side (not the real product
  CLI) exposing the library surface as subcommands, with an identical
  argv/output contract between the Python and Rust harnesses.
- For `emerge`/`ebuild`, black-box test against the **real CLIs directly**
  (with symlinks set up in the test `PATH` so multicall dispatch is
  exercised as in real usage), since they're in scope as actual products,
  not just internal library surface.
- The harness needs **two modes**:
  - *Correctness mode*: one operation per process invocation, pytest-driven,
    exhaustive edge cases.
  - *Benchmark mode*: batch input (many operations per single process
    invocation) to avoid fork/exec overhead dominating the measurement.
- Benchmark data: a **real, vendored Gentoo tree snapshot** (not purely
  synthetic stress data) — realistic scale and distribution of versions/
  atoms/deps. `bench/extract_snapshot.py` refreshes
  `gentoo_snapshot.json` against a live tree using real
  `portage.versions.pkgsplit` as the authority.
- CI gates on both: correctness suite must pass on both implementations;
  benchmark suite must show Rust ahead of Python and must not regress
  over time.
- Rust CI also gates on a **musl static build** smoke-tested inside a
  minimal (`scratch`/busybox-level) container.
- **Container-based real-system differential test bed** (`TEST/`, see
  [`TEST/README.md`](../TEST/README.md) and
  [`history/real-world-testing.md`](history/real-world-testing.md)): runs portuale
  *and* the real `emerge` against a pinned real Gentoo tree inside
  throwaway `podman` containers and diffs the results. **L0**
  (`TEST/run/l0-resolver.sh`) — `emerge -pv` for ~120 real atoms
  (firefox, plasma-meta, `@world`, …), comparing merge lists / USE /
  order / errors / exit codes. **L1**
  (`TEST/run/l1-merge-from-binpkg.sh`) — both PMs merge an identical
  prebuilt binpkg set into a fresh `/` and the resulting filesystem + VDB
  snapshots are diffed. It is **live and exercised** — the only check
  that catches resolver / merge-path regressions at real-tree scale (the
  fixture-based pytest contract suite cannot). Needs the
  `localhost/test-portuale:latest` image (`sudo TEST/create-container.bash`).
  It is **slower and heavier** than the pytest/`cargo test` pass, so it
  is not part of every slice's verification — but running L0 (and L1
  where merge behaviour changed) is **advisable periodically, and
  especially after a big merge from another branch or a change to the
  resolver / merge-order / phase code**, to confirm nothing regressed at
  scale. Findings go in `TEST/findings/`; adjudicated non-bugs in
  `TEST/compare/known-divergences.yaml`. **L1 must run *with* the portage
  upgrade** (do not pass `L1_SKIP_PORTAGE_UPGRADE=1`) — the image's base
  portage predates the VDB consolidated `metadata` file portuale mirrors,
  so skipping the upgrade makes every merged package show a spurious
  `metadata` finding. Last clean full runs (`2026-09-09`): L0 clean
  96/120 / parity 0.800 (the 24 divergent are merge-order timing or the
  parked backtracking-disclosure); L1 porttest 0 hard findings after the
  16-commit director/solver/regen/binpkg merge.

## Ownership

- Python team: `pym/portage` core + the Python-side test harness.
- Rust team: the Rust crate + the Rust-side test harness.
- The **shared pytest contract suite is jointly owned** (separate repo or
  shared submodule) — neither team may unilaterally narrow it to make
  their side pass.

## Current state

portuale is a **working package manager**, used on real systems. It
resolves, builds, merges, unmerges, and manages the world file for real
Gentoo packages, with real ebuild-phase execution and real filesystem
mutation. The `--pretend` resolver is validated against real `emerge`
at real-tree scale (`TEST/` L0: ~120 real atoms, 96/120 byte-identical
plans; L1: filesystem+VDB merge parity, clean).

For the authoritative, cited-source record of every shipped capability
read **[`what-this-proves.md`](what-this-proves.md)** (the living
per-slice ledger) and `git log`. The per-slice "current state" narrative
that used to live here is snapshotted at
[`history/agent-context-current-state-2026-09-10.md`](history/agent-context-current-state-2026-09-10.md).

**2026-09-12 memory note:** backlog #24 (slot-operator rebuild undo
path, S1–S7) is **closed** — the rebuild is a walked graph node, the
`_eliminate_rebuilds` undo and the `_slot_change_probe` slot-move
detector are in, L0 is regression-free, and the v2 residue
(`#24b`–`#24f`, `IUSE_EFFECTIVE`, `--rebuild-if-*` through the same
path) is filed in `scope-backlog.md` §A with its upstream tests. The
one open judgment call (G0.4's S4 acceptance bar, `docs/024-S4-review.md`
D-1) is recorded in `docs/024-oracle.md` §"Verdict", awaiting the
owner.

For what is **genuinely still open** — real portage behaviour not ported
to either side, the deliberate cuts, the standing non-goals — see
**[`scope-backlog.md`](scope-backlog.md)** (Part 2 = remaining work,
Part 3 = non-goals). Keep that file current when a slice closes an entry.
When scoping the next slice, re-ground candidates in current code
(`what-this-proves.md` / `git log` / the source), never in a stale list.

### `helpers/` reference material

- `helpers/devmanual/` — a full local checkout of the Gentoo
  devmanual (`function-reference/`, `tools-reference/`, per-phase
  `ebuild-writing/functions/*/text.xml`). Ground real ebuild-helper
  (`doins`, `dodir`, `insinto`, …) or phase-ordering semantics against it.
- `helpers/emerge_-1v_--debug_--getbinpkgonly__sys-fs--fuse.log` — a real
  `emerge --getbinpkgonly` debug trace; still useful for the remaining
  2.E fetch-ordering tail (`scope-backlog.md`).

## Real ebuild phase execution + filesystem merge

Live. `ebuild <file> install|merge|unmerge|package` and non-`--pretend`
`emerge <atom>` / `--getbinpkg[only]` / `--buildpkg[only]` / `-C` /
`--depclean` / `--prune` / `--config` / `--deselect` / `@set` all run
real ebuild phases (via `brush` over unmodified `bin/*.sh`) and mutate
the filesystem + VDB for real — `rust/portuale/src/{ebuild_phases,
ebuild_merge,ebuild_unmerge,ebuild_package,emerge_build,emerge_getbinpkg}.rs`.
Real eclass `inherit()`, real `SRC_URI` fetch (Manifest digests,
`mirror://`, `RESTRICT`), real `CONFIG_PROTECT` / preserve-libs /
`env_update` / `os.lchown`, `emerge -jN` parallel scheduler +
`--load-average` + build-log capture, `--keep-going`, `--resume`.
`app-arch/unzip`, `sys-fs/fuse`, `app-arch/xz-utils` live-verified end
to end. Per-feature cited-source detail + v1 cuts:
`what-this-proves.md`; what is still missing: `scope-backlog.md` Part 2.
The prior per-slice narrative of this section is in
[`history/agent-context-current-state-2026-09-10.md`](history/agent-context-current-state-2026-09-10.md).

### What "install a package into the filesystem" actually is

Splits into two separable pieces, grounded in real Python source (still
useful orientation even though both are now shipped):

1. **Phase execution** — shelling into a bash interpreter to run the real
   ebuild phase functions in order: `pkg_pretend` (always) → `pkg_nofetch`
   (restricted packages only) → `pkg_setup` (always) → `src_unpack` →
   `src_prepare` → `src_configure` → `src_compile` → `src_test` (source
   builds + tests enabled) → `src_install` (source builds) →
   `pkg_preinst`/`pkg_postinst` (if defined) → `pkg_prerm`/`pkg_postrm`
   (already-installed only) → `pkg_config` (user-requested only) →
   `pkg_info` (always). This calling-order table is documented per-
   function in `helpers/devmanual/ebuild-writing/functions/`.
   The devmanual also has `function-reference/` and `tools-reference/`
   covering the ebuild-helper commands (`doins`, `dodir`, `insinto`,
   etc.) used inside those phases.

2. **The vdb merge** — copying `${D}` into `${ROOT}` and recording
   `CONTENTS` under `/var/db/pkg/...`. Real implementation:
   `dblink.merge()`/`treewalk()`/`mergeme()` in
   `lib/portage/dbapi/vartree.py` (~6500 lines). Module-level `merge()`
   (`vartree.py:6231`) forks a `MergeProcess` that calls `dblink.merge()`
   (`:5958`) → `treewalk()` (`:4191`) → `mergeme()` (`:5323`, the actual
   copy loop, via `portage.util.movefile.movefile`).

   The Python orchestrator tying both together is `doebuild()`
   (`lib/portage/package/ebuild/doebuild.py:768`): for `mydo == "merge"`
   it runs the `install` phase via `spawnebuild()` (`:1592-1623`) and
   only calls `merge()` if that succeeds; `qmerge` skips straight to
   `merge()` assuming `install` already ran; a bare `install` just runs
   the phase without merging. Env setup (`D`, `S`, `WORKDIR`, `T`,
   `FILESDIR`, `EBUILD_PHASE`, `PORTAGE_BUILDDIR`, etc.) happens in
   `doebuild_environment()` (`:381`).

### The bash-execution backend (resolved)

Ebuild phases run bash. The choice was system `bash` vs an embedded
Rust bash (`reubeno/brush`). Outcome:

- **`ebuild --shell bash|brush` / `emerge --shell bash|brush`** select the
  backend explicitly (system-bash subprocess `_doebuild_spawn()`-shaped,
  vs embedded `brush_core::Shell`). **The default is `bash`** — brush's
  `declare -f` corrupts real eclass functions with redirected here-docs
  (`toolchain-funcs`), which breaks `emerge <atom>` for compiled
  packages; brush stays opt-in and static-musl-friendly.
- Several upstream brush bugs were found (brace-less function definitions
  — **merged** as
  [#1274](https://github.com/reubeno/brush/pull/1274); a `declare -f`
  here-doc serialization bug; a pipeline-function-stage deadlock). The pin
  is a **thin `vivo75/brush` fork** = upstream `main` + three
  cherry-picked fixes (`docs/brush-pr/`), merged from upstream
  periodically; drop the fork once the PRs land.
- `rusty_bash` was ruled out (not an embeddable library).

**[`brush-pin.md`](brush-pin.md) is the source of truth for the current
pin, the staged fixes, and the re-pin checklist.**

## How portuale actually runs, session to session

The session-to-session operating rhythm — the "next slice" workflow, the
lockstep/fixture/test rules, the full verification pass, and the
commit/push rules — lives in **[`../AGENTS.md`](../AGENTS.md)**. Read it
before scoping or implementing a slice.

## How to use this doc

"Context" through "Ownership", "Real ebuild phase execution", "What
'install a package' actually is", and "The bash-execution backend" are
settled, citation-backed decisions/findings — not things to re-derive.
"Current state" is a pointer to the live records (`what-this-proves.md`,
`git log`, `scope-backlog.md` — keep that one current). For brush, check
the live `reubeno/brush` state against `brush-pin.md`. If something here
conflicts with current reality, or a genuinely open decision isn't
covered, ask before proceeding rather than assuming.
