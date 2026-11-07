# Architecture

`portuale` is a package manager for Gentoo written in Rust. It began as a
reimplementation of Portage's `emerge` and `ebuild`, and keeps their options
and intentions, but it is no longer a fork and does not aim to match
Portage's output (see [`charter.md`](charter.md)).
It resolves, builds, merges, and unmerges Gentoo packages.
Agent working material lives in `LLM/`.

## Crate map

The workspace root is `Cargo.toml`. All crates live under `crates/`.

- `portage-versions`: version comparison (`vercmp`, `ververify`).
- `portage-dep`: atom parsing and matching.
- `portage-use-reduce`: USE-conditional dependency reduction.
- `portage-required-use`: `REQUIRED_USE` checking.
- `portage-profile`: profile chain and `make.conf` resolution into one `Config`.
- `portage-repo`: repository access, metadata cache, the resolver, and merge order.
- `portage-fetch`: `SRC_URI` fetching (Manifest digests, mirrors).
- `portage-util`: filesystem helpers shared by the other crates.
- `portage-vdb`: the installed-package database and its backends.
- `mrg-director`: the trait contracts (solver, package database, repository
  cache, binpkg fetch, merge engine, binpkg index, news, scheduler policy)
  that the `mrg` front end is written against, so each part can be swapped.
- `harness-common`: shared scaffolding for the test-harness binaries.
- `versions-harness`: neutral CLI harness over `portage-versions`.
- `atom-harness`: neutral CLI harness over `portage-dep`.
- `use-reduce-harness`: neutral CLI harness over `portage-use-reduce`.
- `required-use-harness`: neutral CLI harness over `portage-required-use`.
- `portuale`: the multicall binary itself. It runs as `emerge`, `ebuild`,
  `mrg`, `vdb` or `portageq`, depending on its own name.

Dependencies flow upward toward `portuale`:

```
portage-versions, portage-use-reduce, portage-required-use, portage-util  (leaves)
  |                    |
  +--> portage-dep     +--> portage-profile, portage-fetch, portage-vdb
         |                       |
         +------> portage-repo <--+
                      |
         +--> mrg-director
         |
         +--> portuale (also uses every crate above directly)
harness-common --> versions-harness, atom-harness,
                   use-reduce-harness, required-use-harness
```

## The `mrg` front end

`mrg` is a second front end with the same role as `emerge` and its own
extra options. It lives in `crates/portuale/src/mrg.rs`. Today it parses
the `emerge` option set with `clap`, translates it to canonical long-form
arguments, and hands them to `pretend::run`, the function the `emerge`
applet also runs. So resolution, merge order and display are shared with
`emerge` for now. It adds these options:

- `--solver=portage|pubgrub|resolvo` chooses the dependency solver
  (default `portage`, the built-in resolver).
- `--remote-*` options (hostname, user, port, root, vdb, ledger and more)
  merge gpkg binary packages onto a remote host over SSH. Resolution runs
  on the server; the phases run on the client. The design is in
  `docs/remote-merge.md`, and `crates/portuale/src/remote/` holds the code.

`mrg-director` defines the traits for a front end that uses its own
components. Some of them (merge engine, scheduler policy, fetcher) already
carry production traffic; others have a single implementation so far.

## Data flow of one `emerge -pv <atom>`

1. **Argument parsing** (`portuale`: `src/main.rs`, `src/emerge_options.rs`,
   `src/pretend/args.rs`). The multicall binary dispatches on its own name.
   `emerge` parses flags and atoms into options.
2. **Config and profile resolution** (`portage-profile`:
   `src/resolve/mod.rs`, `src/profile_chain.rs`, `src/make_conf.rs`).
   `resolve_config` folds the profile chain, `make.conf`, the environment,
   and the `package.*` files into one `Config`.
3. **Repository and metadata cache** (`portage-repo`:
   `src/metadata_cache.rs`, `src/md5_dict.rs`). Candidates come from each
   configured repository. Ebuild metadata comes from `metadata/md5-cache`,
   never by running the ebuild.
4. **The dependency resolver** (`portage-repo`: `src/resolve/`,
   `src/resolve/pretend.rs`). The resolver walks dependencies, checks
   visibility, slots, USE, licenses, and blockers, and builds the merge graph.
5. **Merge order** (`portage-repo`: `src/merge_order/`). The graph is sorted
   into install order, with cycle handling.
6. **Display** (`portuale`: `src/pretend/merge_list.rs`,
   `src/pretend/display/`). The ordered list prints in a layout modelled on
   `emerge --pretend`, with verbose counters under `-v`.

Worked traces of representative invocations live in
`docs/diagrams/operation-diagrams.md`.

## Then what a merge adds

A pretend run stops at display. A merge keeps going:

- **Fetch** (`portage-fetch`: `src/lib.rs`; `portuale`: `src/fetch.rs`).
  `SRC_URI` entries download into `DISTDIR` and verify against Manifest digests.
- **Ebuild phases** (`portuale`: `src/ebuild_phases/`). Phases run through
  a genuine `bash` subprocess by default, or the embedded `brush` shell with
  `--shell brush`. The phase scripts in `bin/` run unmodified.
- **Binary packages** (`portuale`: `src/binpkg/`). Builds can be packed and
  unpacked as xpak or gpkg archives.
- **Merge into `$ROOT`** (`portuale`: `src/ebuild_merge/`). The built image
  copies into the target root with collision and `CONFIG_PROTECT` handling.
- **The VDB backends** (`portage-vdb`: `src/lib.rs`). Each merged package is
  recorded under the installed-package database. The file-tree backend is
  the default; SQLite and redb come with the `vdb-sqlite` and `vdb-redb`
  features (on in the default build). See `docs/vdb_to_db.md` and `docs/on-disk-caches.md`.
- **The brush pin** (`docs/brush-pin.md`) tracks the embedded-shell revision.

## Where the tests live

Unit tests live in this repo, next to the code they cover.
The black-box contract suite, the health checker, the fixture tree, and
the container differential test bed live in the sibling `pmtest`
repository. They are a regression and comparison tool; see
`docs/CONTRIBUTING.md`.
`fixtures/` here is a symlink into that tree, so both sides read the same files.

## Where code notes live

`LLM/code-notes/<crate>/<path>.md` holds the long context behind short code
comments: Portage citations, traps, and history. `LLM/code-notes/INDEX.md`
lists every notes file.
