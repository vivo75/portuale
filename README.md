<img src="docs/images/logo.svg" alt="portuale logo" width="120" />

# portuale — a Rust reimplementation of Portage

`portuale` is a drop-in Rust reimplementation of Gentoo's package
manager: **same behaviour as [Portage](https://wiki.gentoo.org/wiki/Project:Portage)
(and then some)**, developed as a **friendly fork** — a separate,
cooperating codebase, verified against the Python original by a shared,
black-box, jointly-owned test suite. It builds, merges, and unmerges
real Gentoo packages today. It was built one reviewed slice at a time,
starting from `portage.versions`; a few portage features are still
unported — see **Status** below.

The four hard goals it is built to (see
[`docs/agent-context.md`](docs/agent-context.md) for the full rationale):

1. **Portability of change, not of source.** A behaviour change on either
   side lands with contract-suite cases the other side must then pass —
   not line-for-line structural mirroring.
2. **Measurably faster than Python**, proven by a CI benchmark gate, not
   assumed.
3. **Runs on a minimal Linux system** — static musl build, zero dynamic
   runtime dependencies.
4. **Tests are written in Python for both implementations**, driven
   black-box via the CLI.

## Status

The core `emerge` / `ebuild` loop is **real and live** — it has built,
merged, and unmerged `app-arch/unzip`, `sys-fs/fuse`, and
`app-arch/xz-utils` end to end against an actual Gentoo tree. Shipped:
full `emerge --pretend` dependency resolution (atoms, slots, USE deps,
blockers, slot conflicts, `USE_EXPAND`, `REQUIRED_USE`, autounmask,
profile/`make.conf` config, overlays, the real `resolver/output.py`
layout with ANSI colour); real ebuild phase execution via an embedded
`brush` (Rust-native bash) driving unmodified `bin/*.sh`; real `SRC_URI`
fetch; real filesystem merge/unmerge with `CONFIG_PROTECT`,
`collision-protect`, preserve-libs, and `env_update`; `emerge <atom>`,
`-C`/`--unmerge`, `--depclean`/`--prune`, `--config`, `--deselect`,
`--buildpkgonly`, `--getbinpkg`/`--getbinpkgonly`; xpak + gpkg binary
packages.

It is **not yet a complete replacement**. The distance to one is now
dominated by a single large item — a full backtracking resolver — plus a
short incremental tail (scheduler odds and ends, the deliberate
sandbox/`FEATURES` cuts, some binhost/gpkg gaps). See
[`docs/scope-backlog.md`](docs/scope-backlog.md) for the honest
distance-to-parity assessment, and
[`docs/what-this-proves.md`](docs/what-this-proves.md) for the
slice-by-slice record with its cited-source grounding.

## Layout

`git ls-files` is authoritative; this is the shape. Upstream Portage lives
in the gitignored `3rdparty/portage/` checkout; `bin/` is the vendored
Portage bash phase runtime (see [`bin/README.md`](bin/README.md) and
[`3rdparty/README.md`](3rdparty/README.md)).

```
rust/                      Rust workspace
  portage-versions/        shared lib: vercmp / ververify
  portage-dep/             shared lib: Atom + match_from_list (v1 subset) + wildcard matcher
  portage-use-reduce/      shared lib: use_reduce(flat=True)
  portage-required-use/    shared lib: check_required_use
  portage-profile/         shared lib: USE / ACCEPT_KEYWORDS from a profile chain + make.conf
  portage-repo/            multi-repo/metadata/vdb access + resolution + dep-graph walk
  portage-fetch/           shared lib: SRC_URI fetch (Manifest digests, mirrors)
  *-harness/               neutral CLI harnesses (contract + benchmark testing)
  portuale/                the real emerge / ebuild multicall binary
fixtures/                  -> ../pmtest/fixtures (symlink): the one fixture tree,
                           read by the Rust tests here and by the contract suite there
musl/                      musl static-build smoke test (minimal-Linux CI gate)
docs/                      all project documentation (see below)
```

The test suite itself is **not** in this tree: it lives in the sibling
`pmtest` repository, which tests portuale as one of several package
managers. See "Test" below and
[`docs/agent-context.md`](docs/agent-context.md) ("Where the tests
live") for the old-path → new-home map.

## Build

```sh
cd rust && cargo build --release
```

Produces `rust/target/release/portuale`; create `emerge` and `ebuild`
symlinks next to it for multicall dispatch (the tests do this
automatically).

## Test

```sh
# in this repo: the whole Rust workspace (reads ./fixtures -> ../pmtest/fixtures)
cd rust && cargo test --release

# in the sibling pmtest repo: the black-box contract suite
cd ../../pmtest && python3 -m pytest pytests-contract-suite -q
```

The contract suite, the container differential bed, the benchmark and
the primitive-differential scripts live in the sibling `pmtest` repo,
which resolves this tree through its `managers/managers.yaml` registry
(`PMTEST_PM=portuale`, the default) and rebuilds the binary itself. The
fixture tree lives there too: `fixtures/` here is a symlink to it, so
`cargo test` reads the same files the contract suite does — and needs
the sibling checkout to be present.

Full pre-slice verification also runs `cargo fmt --check` and
`cargo clippy --release --all-targets` (zero warnings).

## Run

See [`docs/history/running-it.md`](docs/history/running-it.md) for
live-verified per-slice examples (historical). Quick taste:

```sh
rust/target/release/portuale emerge --pretend sys-apps/portage
rust/target/release/versions-harness vercmp 1.0-r1 1.0
```

## Documentation

| Doc | What it is |
|---|---|
| [`AGENTS.md`](AGENTS.md) | **Start here for agent work.** The "next slice" workflow and the verification / commit rules. |
| [`docs/agent-context.md`](docs/agent-context.md) | The full context: goals, hard constraints, architecture decisions, the bash-backend investigation, current state, and the open backlog. |
| [`docs/what-this-proves.md`](docs/what-this-proves.md) | The living, append-only per-slice record — every feature, with its real-portage source grounding. |
| [`docs/scope-backlog.md`](docs/scope-backlog.md) | What real portage behaviour is *not* yet ported (either side), the standing non-goals, and the distance to a drop-in replacement. |
| [`docs/history/running-it.md`](docs/history/running-it.md) | Runnable per-slice examples (historical). |
| [`docs/brush-pin.md`](docs/brush-pin.md) | The `brush` (embedded bash) dependency pin and its re-pin checklist. |
| [`docs/diagrams/operation-diagrams.md`](docs/diagrams/operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code. |
| [`docs/history/`](docs/history/) | Superseded planning documents (the original derivation) and pre-compaction snapshots of docs that get periodically purged (e.g. the scope-backlog's shipped-item narrative). |
