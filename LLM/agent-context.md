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

