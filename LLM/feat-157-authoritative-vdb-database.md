# #157 — swappable installed-package database (files / sqlite / redb)

> Paths and identifiers written before 2026-10-10 may predate the #336 refactor: see [`renames.tsv`](renames.tsv).

Status: **implemented 2026-10-05** on branch `backlog/305-vdb-backends`
(slices S0–S8; S9, `emerge` selection, not done: backlog #318; read-write FUSE, D2: #317).
Proposed 2026-09-25 (revised the same day: three backends, `mrg` first).
What shipped and how it was checked: [`what-this-proves.md`](what-this-proves.md)
(#305 paragraph), evidence in `docs/evidence/305-*.md`.
Design: [`vdb_to_db.md`](../docs/vdb_to_db.md). Read it first; this file holds
only the slices, the gates and the open decisions.
Backlog: [`backlog-tasks-2026-10.md`](backlog-tasks-2026-10.md) Tier 2
**#305** (filed 2026-10-04; #157 was taken by then, so "feat#157" stays
only as this design's label).
Plan: [`02.305-vdb-backends.opus.md`](02.305-vdb-backends.opus.md).
Branch: `backlog/305-vdb-backends` (in **both** repos).
Merge-path gate: **yes** for S4 and S5 (glibc + bash test merge per
`agent-context.md` "Merge-path safety gate", on each database backend
as it lands). S1 is a refactor of the merge path too: gate it on
`files`.

## 0. Owner decisions (2026-09-25)

| # | Decision |
|---|---|
| D0 | The selected backend is authoritative for its root. |
| D1 | Converters go both ways; implemented as "convert from any backend to any other". |
| D2 | Read-only FUSE first; read-write deferred (export covers writers until then). |
| D3 | Native `portageq` helper replaces `portageq-wrapper`. |
| D4 | `world`, `world_sets`, `preserved_libs_registry`, config memory, `counter` live in the backend. |
| D5 | Three swappable backends: `files` (historic), `sqlite` (preferred), `redb`. |
| D6 | Only `mrg` needs backend selection at first. `emerge` now or later (recommendation: later; it stays on `files`). |

## 1. Key constraints (from the design)

- **One engine for both applets.** `mrg` hands off to `pretend::run`,
  the function `emerge` runs (`portuale/src/mrg.rs:1385`). Backends
  therefore live under the engine, behind a process-wide registry
  (`root → Arc<dyn InstalledDb>`, default `FilesDb`). Selection is the
  only per-applet code.
- **redb allows one process only.** For redb, the `portageq` helper
  asks the parent over a pipe, FUSE is an offline view, and converters
  take turns with `mrg`.
- **Remote `mrg`:** a `client:<path>` VDB is written by bash on the
  client and stays in `files` format; a `server:<path>` VDB may use any
  backend.
- **`emerge` stays byte-identical** to real Portage and is still what
  the L0–L3 beds grade. They are unaffected until S9, which is optional.

## 2. Slices

| Slice | Content | Verification |
|---|---|---|
| S0 | Evidence: real `vartree` write order (`_bump_mtime`, metadata write-then-stamp, `counter_tick_core`); an inventory of the ~140 hand-built `var/db/pkg` path sites; the round-trip corpus = host VDB + every pmtest fixture VDB | Evidence file only |
| S1 | `portage-vdb` crate: `InstalledDb` / `WriteTxn`, `FilesDb` (today's code moved), registry; route **every** VDB access through it; adapt `mrg_director::PackagesDb` onto it | No behaviour change: contract suite, `cargo test`, L0 and L1 byte-identical; glibc + bash gate on `files` |
| S2 | `SqliteDb` (feature `vdb-sqlite`) + `portuale vdb convert` / `verify` + a conformance suite run against every backend | `verify` byte-identical files → sqlite → files on the round-trip corpus; conformance green on `files` and `sqlite` |
| S3 | `mrg --vdb-backend=files|sqlite|redb` (+ `--vdb-path`, `PORTUALE_VDB_BACKEND`); read path on sqlite; generation replaces per-entry `statx` | `mrg --pretend` prints the same plan on `files` and `sqlite`; strace shows no per-entry `statx` on sqlite |
| S4 | Write path on sqlite: merge / unmerge / replace / W4 / counter / D4 in one transaction; `merging` state + startup sweep | `mrg` merge on sqlite, then convert to `files`, equals the same merge on `files`; glibc + bash gate on sqlite |
| S5 | `RedbDb` (feature `vdb-redb`), 64 KiB blob chunks, parent pipe for the helper | Conformance + convert round-trip on redb; glibc + bash gate on redb |
| S6 | Native `portageq` (`has_version` / `best_version`) + the vendored `bin/portageq-wrapper` shim | Contract pins against real `portageq` output; L1 gate passes without `3rdparty/portage` |
| S7 | Read-only FUSE (`portuale vdb mount`) | eix / qlist / real `emerge -p` through the mount (over sqlite) match a `files` conversion |
| S8 | R4 / R6 index consumers (`find_owners`, `installed_reverse_dependents`) on the database backends | Unit tests + depclean / collision pins on all backends |
| S9 | *(optional)* `emerge` selection through a `make.conf` variable | Beds stay on `files`; conformance on `emerge` |

## 3. Open questions (owner)

1. **Default paths:** per `EROOT`, e.g.
   `${EROOT}/var/lib/portage/vdb.{sqlite,redb}`?
2. **`mrg` default backend:** explicit only (`--vdb-backend` >
   `PORTUALE_VDB_BACKEND` > `files`), or pick `sqlite` automatically
   when its file exists?
3. **Real Portage on the same host:** detect an on-disk VDB newer than
   the selected database and refuse or warn, or document "convert
   again"?
4. **Versioning:** tie the database schema version to
   `METADATA_FILE_FORMAT_VERSION`, or keep them independent?
5. **`portageq` transport:** the parent pipe for redb only, or for all
   backends?
6. **Cargo features:** are `vdb-sqlite` / `vdb-redb` off in the minimal
   static build profile?
