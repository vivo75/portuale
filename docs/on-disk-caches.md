# Portage on-disk caches and databases: access patterns

Source-grounded audit of every store Portage keeps under `$EROOT`
(`3rdparty/portage/lib/portage/const.py:48-59`:
`VDB_PATH=var/db/pkg`, `CACHE_PATH=var/cache/edb`,
`PRIVATE_PATH=var/lib/portage`, `NEWS_LIB_PATH=var/lib/gentoo`).
Question answered per store: **when is it written, when is it read,
when is it modified in place** — and whether a better alternative exists.

Global rule: **nothing is patched in place**. Every store is a full-file
rewrite via temp-file + rename (`write_atomic` / `atomic_ofstream`,
`cache/flat_hash.py:60-94`, `cache/metadata.py:146-174`,
`util/mtimedb.py:103-116`, `util/__init__.py:682-692`
`writedict → write_atomic`). In-place mutation happens only inside the
VDB package dir itself (add/remove/rename single files) and for
append-only logs.

## 1. `/var/db/pkg` — the installed DB (`dbapi/vartree.py`)

Authoritative state. One dir per `CAT/PF` with ~20 one-line files
(`SLOT`, `USE`, `COUNTER`, `repository`, …) plus multi-line `CONTENTS`,
`NEEDED.ELF.2`, `environment.bz2`, `EBUILD`, and the consolidated
`metadata` snapshot (`dbapi/vartree.py:68-108`).

- **Written (create):** `dblink.merge()` / `treewalk()` on merge
  (`_MergeProcess.py`), `cpv_inject()` (`vartree.py:609-616`:
  `ensure_dirs` + `COUNTER` via `write_atomic`),
  `_write_metadata_file()` at end of merge — atomic body first, then
  `_stamp_metadata_file()` appends `#dir_mtime=`, because the rename
  itself bumps the dir mtime (`vartree.py:188-229`).
- **Read:** every resolver query via `aux_get()` → `_aux_get()`
  (`vartree.py:909-1057`) — tries the `metadata` snapshot first
  (accepted only if `#format=` matches and
  `#dir_mtime == st_mtime_ns`, else falls back to one `open()` per
  field); keys outside the snapshot set fall back to a
  `bunzip2 -c environment.bz2` grep pipe (`_aux_env_search`,
  `vartree.py:1059-1127`). `cpv_all()` is a `listdir` walk;
  `getcontents()` reads `CONTENTS` for unmerge/depclean. One
  mtime-keyed in-process `_aux_cache["packages"]`
  (`vartree.py:939-967`) avoids re-reads within a run.
- **Modified in place:** the only store with true in-place edits.
  `aux_update()` (`vartree.py:1129-1156`: `_bump_mtime` before *and*
  after, per-file `setfile`/`unlink`, then reconsolidate `metadata`),
  `writeContentsToContentsFile()` rewrites `CONTENTS` + `NEEDED`
  (`vartree.py:1482-1499`), `move_ent()` renames dirs on slotmove,
  unmerge removes the dir. `_bump_mtime()` utimes the category and base
  dirs so consumers can use dir mtimes as cache keys (bug #290428,
  `vartree.py:570-587`). Locking: `lockdir(dbroot)` for counter/merge,
  per-`cp:slot` `lockfile` (`_slot_lock`), `lockfile(_conf_mem_file)`
  for config-protect state.

## 2. `/var/cache/edb/dep` — ebuild metadata cache (`dbapi/porttree.py:236,266-321`)

Derived cache keyed by ebuild+eclass hashes (`validation_chf`
`md5`/`mtime`, `cache/template.py:233-261`). Default backend is
`md5-dict` (`cache/flat_hash.py`, one file per CPV); the old `pms`
format is `cache/metadata.py`; `cache/sqlite.py` is a single-file
alternative; `cache/volatile.py` is memory-only.

- **Written:** on cache miss only, after the `depend` phase
  (`_write_cache`, `porttree.py:584-601`). `flat_hash`:
  `mkstemp` + `rename` (`flat_hash.py:60-94`). `metadata`: skips the
  write when content + mtime are identical, else
  `NamedTemporaryFile` + `rename` + `utime(mtime)`
  (`metadata.py:98-179`). `egencache` regenerates in bulk;
  `metadata-transfer` controls copying the repo pregen cache in.
  `sqlite` buffers and `commit()`s on `close_caches()`
  (`porttree.py:429-439`); `sync_rate` batching
  (`cache/template.py:38-40,148-152`).
- **Read:** every `portdb.aux_get()` tries `_pregen_auxdb`
  (repo-shipped) → `_ro_auxdb` → writable `auxdb`; first
  `validate_entry()` hit wins, corrupt writable entries are deleted
  (`porttree.py:603-658`). Unprivileged or non-writable-depcachedir
  users get `volatile.database` + a read-only disk mirror
  (`porttree.py:299-313`).
- **Modified in place:** never — single-file replace only.

The pregen side lives in the repo itself (`metadata/md5-cache`,
`repository/config.py:590-605`): read-only for normal runs, produced
by `egencache`, never patched by the client.

## 3. `/var/cache/edb/{mtimedb,counter}` (`_legacy_globals.py:20-27`)

- **`mtimedb`** (JSON
  `{info,ldpath,resume,resume_backup,starttime,updates,version}`,
  `util/mtimedb.py`): slurped whole at startup (tolerates
  `ENOENT`/`EACCES`/bad JSON); `commit()` rewrites via
  `atomic_ofstream` + `chmod 644` only when the dict differs from
  `_clean_data` (`mtimedb.py:94-116`). Writers: `--resume` /
  `--keep-going` lists (`emaint/modules/resume/resume.py:17-56`),
  `emaint sync` (`sync.py:288`). Never modified in place, and
  **commits take no lock** — concurrent emerges can clobber each
  other's resume list.
- **`counter`** (one integer, `vartree.py:420,1304-1400`): read by
  `get_counter_tick_core()` (one line); on corrupt/mismatched content
  it falls back to a **full VDB scan** (`cpv_all` +
  `aux_get(COUNTER)` per package, `vartree.py:1363-1368`). Written by
  `counter_tick_core()` under `lockdir(dbroot)` via `write_atomic`,
  once per merge. Never modified in place; `ENOENT` tolerated
  ("files under `/var/cache` may disappear").

## 4. `$PKGDIR/Packages{,.gz}` + `$EROOT/var/cache/edb/binhost/…/Packages` (`dbapi/bintree.py`)

- **Written:** local index on `emerge --buildpkg` / `quickpkg` inject
  and on `eclean` / remove — full-index rewrite via `atomic_ofstream`
  under `lockfile(Packages)`
  (`bintree.py:1994-2030,2053-2090,2260`). Remote copies cached after
  fetch via `atomic_ofstream`, carrying `TIMESTAMP` / `TTL` /
  `DOWNLOAD_TIMESTAMP` headers (`bintree.py:1497-1546,1816`).
- **Read:** `--usepkg` resolution loads the local plus each binhost
  cached `Packages`; `TTL` / `--getbinpkg-refresh` / frozen selects
  cached vs re-fetch (`bintree.py:1530-1545`).
- **Modified in place:** never — but a **full O(n) rewrite per
  single-package inject/remove**, the worst scaling on this page.

Binpkg payloads (`$PKGDIR/All/*.tbz2|*.gpkg.tar`) and `DISTDIR`
tarballs (default `/var/cache/distfiles`,
`cnf/make.conf.example:113-124`) are write-once, hash-verified, never
patched (`package/ebuild/fetch.py:1274-1481`: stat + Manifest check →
download-to-temp → rename; per-file `lockfile` when
`FEATURES=distlocks`).

## 5. `/var/lib/portage/{world,world_sets,config,preserved_libs_registry,repo_revisions}`, `/var/lib/gentoo/news` (`const.py:50-55`)

Tiny intent/state files, all the same pattern: `load()` with an mtime
check, mutate in memory, sorted full rewrite via `write_atomic` under
`lockfile` (`_sets/files.py:274-335`).

- `world` / `world_sets`: read on every `@world` expansion; written on
  successful merge, `--deselect`, and the global-updates migration
  (`_global_updates.py:65-210`).
- `config` (CONFIG_PROTECT memory, `grabdict`/`writedict`,
  `util/__init__.py:383,682`): read **and rewritten per
  merge/unmerge** (`vartree.py:2909,3271,5108,5482`) under `_fs_lock`
  — O(n) whole-file churn per package.
- `preserved_libs_registry`: read/written on preserve-libs
  merge/unmerge paths.
- `news-*.unread` (`news.py:101,482`): written at sync time, read by
  `emerge --check-news`, cleared entry-wise by `eselect news read`.

## 6. Logs (`elog/mod_save.py`, `PORTAGE_LOGDIR`, default `/var/log/portage`)

Write-once per event (`pf:timestamp.log` under `elog/<cat>/`,
`open(..., "w")`); never read or modified by Portage itself (only
`emaint logs --clean` deletes by age). No consistency problem.

## Are better alternatives worth it?

1. **VDB: keep the files.** External tools read `/var/db/pkg`
   directly, and per-file granularity + dir-mtime validation + the
   `metadata`-snapshot fast path already fix read amplification
   without a daemon. A SQLite/daemon VDB would speed full scans but
   breaks compat and adds crash-recovery plus static-binary (musl, no
   dynamic deps) cost. Cheaper wins: avoid the `environment.bz2`
   subprocess fallback on hot paths (cache the keys resolution really
   needs).
2. **Dep cache: keep flat files, add no new format.** `flat_hash`
   mkstemp+rename is crash-safe and lock-free for readers; `sqlite`
   buys cheaper iteration / `get_matches` but pays with `database is
   locked` timeouts (15 s, `sqlite.py:50`) and pid-aware reconnects.
   ~40k tiny files per repo is acceptable, and a reimplementation
   only needs to *read* `md5-cache` and memoise in memory.
3. **Fix two genuine scaling bugs instead of redesigning:** (a)
   `Packages` full rewrite per inject — append + periodic compaction
   (or a per-package sidecar index) removes the only O(n)-per-package
   write; (b) `config`-memory whole-file churn per merge and the
   `counter`-recovery full VDB scan — a per-path key dir / persisted
   high-water mark removes both. `mtimedb` wants a `lockfile` around
   `commit()` if parallel `--resume` ever matters.
4. **Portuale consequences:** must *write* byte-identical VDB
   (`metadata` file + double `_bump_mtime` + `COUNTER` / `counter`
   sequence, else autoclean ordering diverges), `world` /
   `world_sets`, and the local `Packages` index on `--buildpkg`; may
   treat the dep cache as read-only, `mtimedb` / resume as optional,
   and `DISTDIR` / binpkg payloads as an immutable content store.

## Suitable database engines per store

No single engine fits all six stores, and two must stay plain files
for compat. Assumes portuale's hard goals (`agent-context.md`:
musl-static binary, pure-Rust preferred, drop-in replacement): anything
needing C/C++ (LMDB, RocksDB, MDBX) is a portability cost even when
technically faster.

- **redb** (CoW B+tree KV, MVCC, ACID, single file, pure Rust, stable
  format in 2.x): best fit anywhere a KV index is justified. Single
  writer transactions match portage (all writes already serialized
  under `lockdir` / `lockfile`).
- **LMDB / MDBX** (`heed`, C): fastest point reads, zero-copy,
  lock-free readers — but fixed `mapsize`, mmap breaks on NFS, and the
  C dep hurts the static build. What redb copies without the C.
- **fjall** (LSM KV, pure Rust, LZ4, blob separation): right only for
  bulk-ingest workloads (`egencache`); read amplification +
  background compaction threads are overkill for read-mostly ~1 KB
  values. **sled** (stagnant, space-amp issues) and **RocksDB**
  (C++, tuning burden) are not picks for new work.
- **SQLite** (`rusqlite` bundled, C but statically linkable):
  already portage's optional dep-cache backend (`cache/sqlite.py`,
  15 s lock timeout); helps only where secondary-index / `get_matches`
  queries exist, with known `database is locked` pain.
- **Content-addressed store** (Nix/OSTree-style hash-named blobs +
  refs, implementable by hand): the right model for `DISTDIR` /
  binpkg payloads, not for mutable indexes.
- **Plain atomic files** (`write_atomic` + rename): already
  crash-safe, debuggable, hand-editable, NFS-safe — optimal for tiny
  single-value state.

| Store | Suitable engine | Verdict |
|---|---|---|
| VDB (`/var/db/pkg`) | redb sidecar index only (LMDB if C were free) | Files stay canonical: `qlist`/`equery`/scripts/rescue read the dirs directly; `metadata` snapshot + dir-mtime protocol already fixed read amplification. A full migration breaks the ecosystem for zero crash-safety gain. Index tables (`meta`, `owners`, `soname`) would be rebuilt lazily, validated against dir mtimes like the `metadata` file. LSM is wrong (B+tree point-lookup workload); SQLite buys nothing without relational queries. |
| Dep cache (`/var/cache/edb/dep` + `md5-cache`) | **redb** (SQLite exists; LMDB faster but C; fjall overkill) | The one migration worth doing: one file instead of ~40k inodes, MVCC readers (removes the `volatile` + `_ro_auxdb` split, `porttree.py:299-313`), atomic bulk `egencache` commit. Keep repo-shipped `md5-cache` text as the interchange format — repos stay dumb file trees. |
| `Packages` index + binhost cache | Any tiny KV or append-journal + compaction | Keep the text wire format (binhosts serve `Packages{,.gz}`; `TTL`/`TIMESTAMP` in `bintree.py:1530-1545`). The fix is removing the O(n) full rewrite per inject (`bintree.py:1994-2090`), not a query engine. |
| `world` / `world_sets` / `mtimedb` / `counter` / `config` / registry / news | None — atomic files optimal | All < hundreds of entries, read fully, rewritten fully; `world` files are hand-edited by users (a binary DB destroys that). Real fixes, not engines: `lockfile` around `mtimedb.commit()` (concurrent `--resume` clobber), persisted `counter` high-water mark (avoid the full-VDB recovery scan, `vartree.py:1363-1368`). |
| `DISTDIR` / binpkg payloads | CAS layout (hash-named + GC + hardlink) | Dedup across `PORTAGE_RO_DISTDIRS`, verification-free reads; `Manifest` stays the text index. |
| Logs (`elog/`, build logs) | None | Write-once, read-rarely, tailed/grepped, pruned by age. A DB adds write amplification and breaks `tail`/`grep`/logrotate. |

For portuale concretely: dep-cache *reader* against `md5-cache` +
in-memory memo (already done, see `performances-tuning.md`) is enough;
add a redb cache only if the 40k-file layout ever shows up in profiles.
Keep VDB / `world` / `Packages` writes byte-identical files, and never
introduce a C-linked engine into the `emerge` / `ebuild` applets — that
breaks the musl-static goal for at most single-digit percent on a
workload dominated by process spawn and file I/O, not KV latency.
