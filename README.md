<img src="docs/images/logo.svg" alt="portuale logo" width="120" />

# portuale — a package manager for Gentoo, in Rust

`portuale` is a package manager for Gentoo written in Rust. It started as a
reimplementation of [Portage](https://wiki.gentoo.org/wiki/Project:Portage)'s
`emerge` and `ebuild`. `mrg` was added later as a second front end with the
same role as `emerge`, plus extra features and options. Both resolve
dependencies, fetch sources, run the ebuild phases, merge and unmerge files,
and handle binary packages (xpak and gpkg). It builds, merges and unmerges
real Gentoo packages.

The main goal is a healthy installation: dependencies hold, the build order
is legal, packages are as new as possible, removal does not break running
programs, and error messages help. The charter,
[`docs/charter.md`](docs/charter.md), defines these five properties and
what counts as correct.

## Relation to Portage

- portuale is not a fork and does not aim to replace Portage. We hope
  Portage stays well maintained for a long time.
- `portuale emerge` keeps the options of `emerge`, with the same meaning and
  intentions. It may differ in build order and in the messages it prints.
- Portage is a teacher, no longer the oracle. We watch how it evolves and
  learn from it. Behaviour is derived from the PMS, the EAPIs and the
  Devmanual; where those are silent, the owner decides.
- The version number stays derived from Portage's release (see
  [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)).
- The sibling `pmtest` repository is kept to support Portage's development
  and as a regression and comparison tool.

## Status

Shipped and used on real Gentoo trees: `emerge --pretend` dependency
resolution (slots, USE dependencies, blockers, `REQUIRED_USE`, autounmask,
profile and `make.conf` configuration, overlays); ebuild phase execution with
a real `bash` or the embedded `brush` shell; `SRC_URI` fetch; merge and
unmerge with `CONFIG_PROTECT`, collision protection and preserved
libraries; `--unmerge`, `--depclean`, `--prune`, `--config`, `--deselect`,
`--buildpkgonly`, `--getbinpkg`, `--getbinpkgonly`; xpak and gpkg binary
packages; `mrg` remote merge of gpkg packages over SSH.

Not everything Portage does is ported. Some Portage behaviour is kept for
now only because no check yet shows that dropping it is as good; it is
removed gradually (see the charter). The agent-side record of what is done
and open is in [`LLM/`](LLM/README.md).

## Layout

`git ls-files` is authoritative; this is the shape. Upstream Portage lives
in the gitignored `3rdparty/portage/` checkout; `bin/` is the vendored
Portage bash phase runtime (see [`bin/README.md`](bin/README.md) and
[`3rdparty/README.md`](3rdparty/README.md)).

```
Cargo.toml                 Rust workspace root (cargo runs from the repo root)
crates/                    the workspace crates
  portage-versions/        shared lib: vercmp / ververify
  portage-dep/             shared lib: Atom + match_from_list (v1 subset) + wildcard matcher
  portage-use-reduce/      shared lib: use_reduce(flat=True)
  portage-required-use/    shared lib: check_required_use
  portage-profile/         shared lib: USE / ACCEPT_KEYWORDS from a profile chain + make.conf
  portage-repo/            multi-repo/metadata/vdb access + resolution + dep-graph walk
  portage-fetch/           shared lib: SRC_URI fetch (Manifest digests, mirrors)
  portage-util/            shared filesystem helpers
  portage-vdb/             installed-package database and its backends
  mrg-director/            trait contracts behind the `mrg` front end
  *-harness/               neutral CLI harnesses (used by the pmtest suite)
  portuale/                the multicall binary: emerge, ebuild, mrg, vdb, portageq
fixtures/                  -> ../pmtest/fixtures (symlink): the one fixture tree,
                           read by the Rust tests here and by the contract suite there
musl/                      musl static-build smoke test (minimal-Linux CI gate)
docs/                      all project documentation (see below)
```

The test suite itself is **not** in this tree: it lives in the sibling
`pmtest` repository, which tests portuale as one of several package
managers. See "Test" below and
[`LLM/agent-context.md`](LLM/agent-context.md) ("Where the tests
live") for the old-path → new-home map.

## Build

```sh
cargo build --release
```

Produces `target/release/portuale`. The binary dispatches on its own
name: create `emerge`, `ebuild` and `mrg` symlinks next to it (the tests do
this automatically), or run `portuale <applet> ...`.

## Test

```sh
# in this repo: the whole Rust workspace (reads ./fixtures -> ../pmtest/fixtures)
cargo test --release

# in the sibling pmtest repo: the black-box contract suite
cd ../pmtest && python3 -m pytest pytests-contract-suite -q
```

The contract suite, the health checker, the container differential bed,
the benchmark and the primitive-differential scripts live in the sibling
`pmtest` repo,
which resolves this tree through its `managers/managers.yaml` registry
(`PMTEST_PM=portuale`, the default) and rebuilds the binary itself. The
fixture tree lives there too: `fixtures/` here is a symlink to it, so
`cargo test` reads the same files the contract suite does — and needs
the sibling checkout to be present.

Full pre-slice verification also runs `cargo fmt --check` and
`cargo clippy --release --all-targets` (zero warnings).

## Run

Live-verified per-slice examples are in
[`LLM/what-this-proves.md`](LLM/what-this-proves.md). Quick taste:

```sh
target/release/portuale emerge --pretend sys-apps/portage
target/release/versions-harness vercmp 1.0-r1 1.0
```

## Documentation

| Doc | What it is |
|---|---|
| [`docs/charter.md`](docs/charter.md) | Read first. What portuale is, its relation to Portage, what counts as correct, how the code must look. |
| [`docs/architecture.md`](docs/architecture.md), [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md) | How the code is organised; how to build, test and contribute. |
| [`AGENTS.md`](AGENTS.md) | **Start here for agent work.** The "next slice" workflow and the verification / commit rules. |
| [`LLM/agent-context.md`](LLM/agent-context.md) | The full context: goals, hard constraints, architecture decisions, the bash-backend investigation, current state, and the open backlog. |
| [`LLM/what-this-proves.md`](LLM/what-this-proves.md) | The living, append-only per-slice record — every feature, with its Portage source grounding. |
| [`LLM/scope-backlog.md`](LLM/scope-backlog.md) | What Portage behaviour is not yet ported, and the standing non-goals. |
| [`LLM/lessons-of-backlog-ops-2026-10-03.md`](LLM/lessons-of-backlog-ops-2026-10-03.md), [`LLM/recap-of-backlog-ops-2026-10-03.md`](LLM/recap-of-backlog-ops-2026-10-03.md) | How to work on backlog items (condensed lessons) and the per-item status/residue/cut index for the closed batches. |
| [`docs/brush-pin.md`](docs/brush-pin.md) | The `brush` (embedded bash) dependency pin and its re-pin checklist. |
| [`docs/diagrams/operation-diagrams.md`](docs/diagrams/operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code. |
