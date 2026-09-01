# Prompt: Plan a Python-to-Rust friendly fork of Portage

Use this prompt to (re)derive the porting strategy for Portage from scratch.
It encodes the goals, hard constraints, and architectural decisions already
reached, so the plan can be regenerated or handed to a fresh LLM session
without repeating the discovery conversation. If you disagree with a
decision below, say so explicitly and re-open it — don't silently override it.

## Context

Portage (this repository) is the Gentoo package manager, written in Python.
The goal is to create a Rust implementation as a **friendly fork**: a
separate, cooperating codebase, not a hostile competitor and not (yet)
committed to being a full replacement.

**EAPI floor**: EAPI 0, 1, 2, 3, 4, and 6 are deprecated and removed in
this repo/fork — no ebuild uses them, and all profiles are EAPI 5 or
higher (5, 7, 8 are the live versions). Any EAPI-conditional logic being
read or ported (e.g. `bin/ebuild.sh`'s `__check_bash_version`, EAPI-gated
behavior in eclasses or `lib/portage`) only needs to account for EAPI 5+
as the real, live baseline — branches that only apply to EAPI 0/1/2/3/4/6
are dead code and can be ignored rather than faithfully ported.

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

- **End state is undecided**: this may become two permanent sibling
  implementations (like `uutils` vs GNU coreutils) or a strangler-fig
  migration where Rust eventually replaces Python. Do not pick an
  architecture that forecloses either option. Subprocess/CLI-based testing
  satisfies this; in-process FFI embedding (e.g. PyO3) would not, so avoid
  it for now.

## Scope of the first port

Include, in this order of foundational-ness:

1. **Core library**: version comparison (`portage.versions`), atom/dep
   parsing and matching, config resolution, dependency graph (depgraph).
2. **`emerge` and `ebuild` executables**, restricted to **dry-run / read-only
   behavior only**: dependency resolution, `--pretend` output, parsing and
   validation. No real merges, installs, or filesystem mutations in the
   first port — that's deliberately deferred to limit blast radius while
   the parity test suite is still young.

### `emerge`/`ebuild` binary shape

Ship `emerge` and `ebuild` as **one multicall binary** (busybox-style),
dispatching behavior based on `argv[0]` via symlinks/hardlinks pointing at
a single executable. This is both a good minimal-Linux fit (one static
binary, no duplicated code) and drop-in compatible with tooling that
invokes `emerge`/`ebuild` by name directly.

### Deferred: ebuild phase execution

Real phase execution (`pkg_setup`, `src_compile`, `src_install`, etc.) is
**out of scope for the first port** because ebuilds are bash scripts.
When it is tackled later:

- The Rust executor **shells out to the system bash** (not an embedded
  interpreter) — this is a deliberate, accepted dynamic dependency at that
  later stage, in tension with the minimal-Linux goal, which is why it's
  deferred rather than solved now.
- The bash version check **must mirror Python's existing EAPI-variable
  floor exactly**, not a flat constant. See `bin/ebuild.sh` function
  `__check_bash_version` (as of this writing: absolute minimum bash 4.4,
  rising to 5.0 or 5.3 depending on the ebuild's declared EAPI via
  `BASH_COMPAT`). The practical baseline going forward is bash >= 5.3.0.
  Per the EAPI floor above, only the branches covering EAPI 5+ are
  actually reachable in this repo — the function's own EAPI 0–4/6
  branches (e.g. `___eapi_bash_3_2`/`___eapi_bash_4_2`) are dead code
  here and don't need porting. Re-read that function before implementing
  — don't rely on this description as the source of truth, it will drift.

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
  atoms/deps.
- CI gates on both: correctness suite must pass on both implementations;
  benchmark suite must show Rust ahead of Python and must not regress
  over time.
- Rust CI also gates on a **musl static build** smoke-tested inside a
  minimal (`scratch`/busybox-level) container.

## Ownership

- Python team: `pym/portage` core + the Python-side test harness.
- Rust team: the Rust crate + the Rust-side test harness.
- The **shared pytest contract suite is jointly owned** (separate repo or
  shared submodule) — neither team may unilaterally narrow it to make
  their side pass.

## First execution step — DONE, see README.md for current state

The original plan here was to pilot the whole pipeline end-to-end on the
smallest meaningful slice (`portage.versions`, a minimal `emerge`/`ebuild`
multicall skeleton, musl build + scratch-container smoke test) before
committing further, and only tackle depgraph/config resolution and
broader `emerge`/`ebuild` behavior once that pilot proved out the
mechanism (harness contract format, CI gating, benchmark methodology,
musl packaging).

That step is long since complete, and the pilot has since gone well
beyond it — real atom matching, `use_reduce`, a working
`emerge --pretend` with recursive DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND
resolution, real profile/make.conf-derived USE/ACCEPT_KEYWORDS,
package.mask/.unmask/.accept_keywords/.use, blockers, overlays, slot
conflicts, multiple/versioned/slotted top-level atoms, `-v`'s USE display,
and full CLI-surface recognition for both `emerge` and `ebuild`, among
other slices. **`README.md`'s "What this proves" section is the
living, incrementally-updated record of everything actually shipped** —
read that, not this section, for current state. This section is kept for
historical context (why the pilot started where it did), not as a
to-do list.

## How to use this prompt

Treat the "Context" through "Ownership" sections above as settled
decisions, not a menu — they're the goals and hard constraints, and
should still hold regardless of how much has been built since. For
what to actually *do* next, don't scaffold the (already-completed) first
execution step above — read `README.md` for what exists and
`git log` for how work has been landing (one small, fully-shipped,
documented-and-tested "slice" at a time), and scope the next slice from
there. If something in this document conflicts with current reality (e.g.
the bash version floor logic has since changed, or repository structure
has moved), or you find a genuinely open decision not covered above, ask
before proceeding rather than assuming.
