# Documentation

Human documentation for `portuale`. The root [`README.md`](../README.md) is
the overview (what it is, build, test, run); this directory holds the rest.
Material written for and by coding agents (backlog, plans, slice history,
evidence) lives in [`../LLM/`](../LLM/README.md).

## Charter

| Doc | What it is |
|---|---|
| [`charter.md`](charter.md) | What portuale is, its relation to Portage, what counts as correct (the five health properties), how the code must look. Read it first. |

## Design and internals

| Doc | What it is |
|---|---|
| [`diagrams/operation-diagrams.md`](diagrams/operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code. |
| [`diagrams/emerge-source-merge.md`](diagrams/emerge-source-merge.md), [`diagrams/emerge-unmerge.md`](diagrams/emerge-unmerge.md), [`diagrams/emerge-getbinpkgonly.md`](diagrams/emerge-getbinpkgonly.md), [`diagrams/emerge-pretend-world.md`](diagrams/emerge-pretend-world.md) | Per-operation code walkthroughs. |
| [`on-disk-caches.md`](on-disk-caches.md) | Every on-disk cache and database (`/var/db/pkg`, `/var/cache/edb`, `$PKGDIR`, `/var/lib/portage`, logs) and which alternative backends are worth it. |
| [`vdb_to_db.md`](vdb_to_db.md) | The installed-package database backends (files, SQLite, redb). |
| [`performances-tuning.md`](performances-tuning.md) | How `emerge -puD --getbinpkg` went from 77 s to 4.5 s, and what is left. |
| [`architecture.md`](architecture.md) | Crate map and the data flow of one `emerge` run. |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Build, test, comment and commit rules, version numbers. |
| [`cleanroom/`](cleanroom/) | Clean-room descriptions of Portage and portuale subsystems, written earlier. They describe behaviour at the time and are not the current specification. |

## Using portuale

| Doc | What it is |
|---|---|
| [`remote-merge.md`](remote-merge.md) | `mrg`: merging binary packages onto a remote host over SSH (design and options). |
| [`remote_emerge_examples.md`](remote_emerge_examples.md) | Worked `mrg` examples. |

## Dependencies and standards

| Doc | What it is |
|---|---|
| [`brush-pin.md`](brush-pin.md) | The pinned `brush` (embedded bash) dependency, its staged upstream fixes, and the re-pin checklist. |
| [`brush-pr/`](brush-pr/README.md) | The fixes prepared for upstream `brush`. |
| [`glep/`](glep/README.md) | Reference copies of the Gentoo GLEPs portuale implements. |

## Elsewhere

- Co-located READMEs: [`../bin/README.md`](../bin/README.md) (the vendored
  phase runtime), [`../3rdparty/README.md`](../3rdparty/README.md).
- Tests: the contract suite, fixtures and the container differential test
  bed live in the sibling `pmtest` repository
  ([`../../pmtest/differential-test-bed/README.md`](../../pmtest/differential-test-bed/README.md)).
