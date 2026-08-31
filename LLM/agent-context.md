## Context

Portage (this repository) is the Gentoo package manager, written in Python.
Portuale is a Rust reimplementation of it, developed as a **friendly
fork**: a separate, cooperating codebase, not a hostile competitor. It is
a **working package manager** — it resolves, builds, merges, and unmerges
real Gentoo packages — and the aim is a **drop-in, same-behaviour
replacement** (and then some), reached one reviewed, contract-tested
slice at a time. `scope-backlog.md` Part 2 is the honest list of what
Portage still does that portuale doesn't.

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

### `emerge`/`ebuild` binary shape

Ship `emerge` and `ebuild` as **one multicall binary** (busybox-style),
dispatching behavior based on `argv[0]` via symlinks/hardlinks pointing at
a single executable. This is both a good minimal-Linux fit (one static
binary, no duplicated code) and drop-in compatible with tooling that
invokes `emerge`/`ebuild` by name directly. **Shipped**: `crates/portuale`.
A bare `portuale` (or `portuale --help`/`-h`) lists the applets with a
one-line description and exits 0; an unrecognized applet name still
errors. `emerge --help` is a grouped tour of every action/option portuale
implements (`pretend.rs`'s `HELP_TEXT`, pinned by the contract suite).

## Ownership

- Python team: `pym/portage` core + the Python-side test harness.
- Rust team: the Rust crate + the Rust-side test harness.
- The **shared pytest contract suite is jointly owned** (separate repo or
  shared submodule) — neither team may unilaterally narrow it to make
  their side pass.

### What "install a package into the filesystem" actually is

Splits into two separable pieces, grounded in Portage's Python source (still
useful orientation even though both are now shipped):

1. **Phase execution** — shelling into a bash interpreter to run the real
   ebuild phase functions in order: `pkg_pretend` (always) → `pkg_nofetch`
   (restricted packages only) → `pkg_setup` (always) → `src_unpack` →
   `src_prepare` → `src_configure` → `src_compile` → `src_test` (source
   builds + tests enabled) → `src_install` (source builds) →
   `pkg_preinst`/`pkg_postinst` (if defined) → `pkg_prerm`/`pkg_postrm`
   (already-installed only) → `pkg_config` (user-requested only) →
   `pkg_info` (always). This calling-order table is documented per-
   function in `3rdparty/devmanual/ebuild-writing/functions/`.
   The devmanual also has `function-reference/` and `tools-reference/`
   covering the ebuild-helper commands (`doins`, `dodir`, `insinto`,
   etc.) used inside those phases.

2. **The vdb merge** — copying `${D}` into `${ROOT}` and recording
   `CONTENTS` under `/var/db/pkg/...`. Portage's implementation:
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

