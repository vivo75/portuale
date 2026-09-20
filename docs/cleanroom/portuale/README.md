# Clean-room Portage specification: the `emerge` application

This directory is a **clean-room functional specification** of everything
the `emerge` application does. It describes *observable behaviour* —
command-line surface, action flows, inputs, outputs, exit codes, on-disk
effects — as seen from outside the implementation. It is written so that
an independent team ("Portage", the clean-room implementation) can
reproduce `emerge`'s behaviour without reading Portuale's source.

Language: English throughout.

## How to read this specification

| File | Contents |
|------|----------|
| [`01-overview.md`](01-overview.md) | Product definition, multicall-binary dispatch, global concepts (ROOT, config root, repositories, VDB), design constraints |
| [`02-emerge-cli.md`](02-emerge-cli.md) | Complete CLI surface: every action, every option, parsing rules, `--help`, defaults injection |
| [`03-emerge-flows.md`](03-emerge-flows.md) | Every action's end-to-end flow with diagrams: pretend/query, source merge, buildpkgonly, binpkg merge, unmerge, depclean, prune, clean, deselect, resume, sync/regen/metadata, search, info, config, list-sets, check-news |
| [`04-resolver-and-output.md`](04-resolver-and-output.md) | Config resolution, repository discovery, candidate selection, dependency resolution, merge-order computation, and all output rendering (list, tree, columns, JSON, blockers, slot conflicts) |
| [`05-execution.md`](05-execution.md) | Real execution: ebuild phase runner, fetch, package, filesystem merge, unmerge, binary merge |
| [`06-support-and-function-index.md`](06-support-and-function-index.md) | Support subsystems (binpkg formats, elog, color, env-update, install-mask, merge engines, ELF/soname tracking, resume DB, locks, privileges, preserved-libs, remote mode, `mrg` applet, difflib) plus the complete per-module function index |

## Conformance vocabulary

- **MUST** — behaviour the contract suite pins; diverging breaks compatibility.
- **SHOULD** — behaviour real Portage exhibits that the suite checks loosely.
- **MAY** — explicitly documented narrowing where the specification permits
  a simpler behaviour (each is marked as a *documented cut*).

## Source grounding

Every section cites the behaviour it specifies as
`rust/portuale/src/<module>.rs:<line>` (function or block start) and the
supporting crates (`rust/portage-repo`, `rust/portage-profile`,
`rust/portage-fetch`, `rust/portage-dep`, `rust/portage-versions`,
`rust/mrg-director`, …). Line numbers refer to the Portuale tree at the
time of writing; function names are the stable reference.
