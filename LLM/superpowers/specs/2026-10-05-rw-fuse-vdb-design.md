# #317 — read-write FUSE view of the VDB database backends: design

Status: **implemented 2026-10-05** (plan `docs/02.317-rw-fuse.opus.md`,
done). Deviations found while building it, all recorded in the slice
commits: the scratch directory is named after the database file (not the
pid) so the next mount of the same database clears a dead daemon's
leftovers; publish and live renames keep every inode (the kernel's cached
dentries); a live rewrite is stored at `flush` (the kernel ignores
`release` errors) and the mount asks for `FUSE_ATOMIC_O_TRUNC`; a rename
inside a live entry rewrites the whole entry so the temp's mode/mtime and
the stale stamp match disk; volatile lock files accept `chown`.
Backlog: [`backlog-tasks-2026-10.md`](../../backlog-tasks-2026-10.md) Tier 2 #317.
Parent feature: #305 / feat#157 ([`feat-157-authoritative-vdb-database.md`](../../feat-157-authoritative-vdb-database.md),
decision D2 "read-only FUSE first"). Builds on the read-only view
`rust/portuale/src/vdb_view.rs` (S7) and its fuser adapter `vdb_fuse.rs`.
Branch: `backlog/317-rw-fuse` in both repos.

## 1. Goal

`portuale vdb mount --rw KIND:PATH MOUNTPOINT` serves a `sqlite` or `redb`
database as `/var/db/pkg` such that **real Portage 3.0.82.2 `emerge` and
`ebuild` can merge, replace and unmerge packages through it**. The changes
land in the database, not on disk.

Success test: the L1 bed's glibc + bash gate with the *real Portage*
consumer merging through a `--rw` mount over sqlite, and again over redb,
gives the same report as today (0 hard / 0 unexplained against portuale on
files).

Out of scope: arbitrary POSIX use of the tree (stray files, nested
directories, symlinks); `files:` (a pass-through, already writable on
disk); writes from several hosts.

## 2. What real Portage writes (the contract)

From `docs/evidence/305-s0-vartree-write-order.md` (real `vartree.py`,
`locks.py`), to be pinned by an `strace` capture in S0:

| Real operation | Where | Syscalls on the tree |
|---|---|---|
| slot lock `_slot_lock` (`vartree.py:533-552`) | `<cat>/.<pn>:<slot>.portage_lockfile` | `mkdir -p <cat>`, `open(O_CREAT\|O_RDWR, 0660)`, `fcntl` lock, `_lockfile_was_removed` (`locks.py:419-500`): `link` to a `.hardlock-*` name, `stat` both and compare `st_ino`/`st_dev`, `-inode-test` link, `unlink`s; at unlock `unlink` of the lock file |
| `vardbapi.lock()` | `var/db/.pkg.portage_lockfile` | outside the mount |
| `_bump_mtime` | category dir, VDB root | `utime` |
| temp entry (rows 3-15) | `<cat>/-MERGING-<pf>/` | `rmtree` of a stale one, `mkdir`, file copies (`open/write/close`), `write_atomic` (temp name in the same dir, then `rename`), `COUNTER` plain write, `metadata` written atomically then the stamp appended |
| old entry deleted (row 14) | `<cat>/<pf>/` | `rmtree`: `unlink` every file, `rmdir`; `rmdir` of an emptied category |
| publish (row 16) | `-MERGING-<pf>` → `<pf>` | `rename` |
| W4 `removeFromContents` (rows 17, 18, 21) | another live entry's `CONTENTS` | `write_atomic` (temp + `rename`) |
| `PORTAGE_UPDATE_ENV` (row 20) | live `environment.bz2` | in-place rewrite (`O_TRUNC`, write, close) |
| unmerge | `<cat>/<pf>/` | `rmtree` |
| `aux_update` | live field files | `write_atomic` |

## 3. Architecture

A write layer sits above the read-only view. Read precedence per name:
**volatile → staged → database snapshot** (the view's existing
generation-pinned reads).

### 3.1 Volatile names (memory only, never stored)

Lock files (`.*.portage_lockfile`), hardlock names (`*.hardlock-*`,
`*-inode-test`), and category directories created by `ensure_dirs` that
hold no entry yet. They support `create`, `open`, `write`, `link`,
`unlink`, `getattr` with **stable inode numbers** (a link shares its
target's inode), `mkdir`/`rmdir` of a category. `fcntl` locks: the daemon
does not implement `getlk`/`setlk`, so the kernel keeps POSIX locks
locally (enough: every locker runs on this host). They vanish at unmount.

### 3.2 Staged entries (scratch disk)

`mkdir <cat>/-MERGING-<pf>` creates a staged entry backed by a directory in
a scratch area (`$TMPDIR/portuale-vdb-rw-<pid>/`, cleaned at mount start).
Files created, written, renamed, unlinked, `chmod`ed and `utime`d inside it
act on the scratch copy. The database stores no owner, so `chown` to the owner `getattr`
already reports (the mounting user) is a no-op and any other `chown` is
`EPERM`. The staged directory's mtime follows real filesystem
rules: it bumps on create, unlink and rename inside it, and keeps an
explicit `utime`.

A stale `-MERGING-<pf>` (staged, or a database pending row shown by the
view) can be `rmtree`d: a staged one is discarded; a pending row is
`discard_pending`.

### 3.3 Publish points (one database transaction each)

1. **`rename(<cat>/-MERGING-<pf>, <cat>/<pf>)`**: `insert_entry` of an
   `EntryImage` built from the scratch copy (bytes, modes, mtimes,
   `dir_mode`, `dir_mtime_ns` = the staged directory's mtime).
   `metadata_stamp`: `Absent` when there is no `metadata` file; `Valid`
   when its `#dir_mtime=` stamp equals that mtime; otherwise `Stale` (real's
   own validity rule, bug #290428). Same transaction: `set_counter` to
   max(stored counter, the entry's `COUNTER`), so an `mrg` running beside
   a sqlite mount never reuses it. `insert_entry` replaces a live `<pf>`
   (both database backends delete the installed row first). The scratch
   copy is removed after the commit.
2. **Live-entry file change**: `close` of a live file opened for writing
   (`environment.bz2` in place) or `rename(tmp, <field>)` inside a live
   entry (`write_atomic`) → `replace_file`. The temp name lives in scratch
   until the rename; a temp closed without a rename and later unlinked
   never reaches the database.
3. **Unmerge**: `unlink` of files in a live entry only hides them in the
   view (recorded in memory); `rmdir <cat>/<pf>` once every file is hidden
   → `delete_entry`. A daemon that dies mid-`rmtree` leaves the entry
   installed. `rmdir` of a non-empty entry: `ENOTEMPTY`.

Deviation from a `begin_entry`/`put_entry_file` mapping, chosen so the real
stamp and directory mtime survive: nothing reaches the database before the
rename. A daemon crash mid-merge leaves a scratch directory (cleared at the
next mount), like real Portage leaves a stale `-MERGING-` on `files`.

### 3.4 Everything else

`EPERM`: files or directories at the root other than categories, nested
directories inside an entry, symlinks, hard links outside the volatile
names, renames other than §3.3, `setxattr`. `utime` on a category or the
root: accepted, no effect (the view derives those mtimes, S7.3).

## 4. Stores outside the tree

Real `emerge` keeps writing world, world_sets, the preserved-libs
registry, config memory (`var/lib/portage`) and the counter
(`var/cache/edb/counter`) on disk. `--rw` takes `--root ROOT` (default
`/`). On unmount (`fusermount3 -u`, or SIGINT/SIGTERM/SIGHUP with
`--foreground`) the daemon reads those files under ROOT and, in one
transaction, sets world, world_sets, preserved libs and config memory,
and the counter to max(file, database). `portuale vdb import-stores
files:ROOT KIND:PATH` does the same by hand (for a daemon that died
without unmounting).

## 5. Errors and concurrency

- A failed publish transaction → `EIO` to the syscall; the scratch copy
  stays, so the caller sees a disk error and real `emerge` aborts.
- `fsync`/`fsyncdir` on staged files: no-op (durability is the publish).
- fuser's single request loop serialises every operation; real merges
  already serialise VDB writes under their locks.
- redb: a `--rw` mount holds the file read-write, so `mrg`, `vdb` writers
  and readers that need it free (`status`, `verify`) get Busy until
  unmount. sqlite: `mrg` can run beside the mount (busy timeout).
- Read-only mounts stay exactly as today (`EROFS` everywhere).

## 6. Testing

- **Rust (portuale, `vdb_view`/write layer)**: replay the §2 sequences
  against sqlite and redb and compare with the same sequence run on a
  plain directory (the files truth), via `vdb verify`: new merge,
  same-slot replace, unmerge, W4 `CONTENTS` rewrite, in-place
  `environment.bz2`, `aux_update`, a lock cycle (inode equality of the
  hardlink), stale `-MERGING-` removal, a crash mid-`rmtree` and
  mid-merge, publish failure → `EIO`, and the `EPERM` cases. Stamp rule:
  valid/stale/absent each pinned.
- **Host (needs `/dev/fuse`)**: real `ebuild <fixture ebuild> merge` and
  `emerge -C` in a scratch ROOT through a `--rw` mount; `vdb verify` equal
  to the same commands on a files ROOT.
- **L1 bed (pmtest)**: `L1_PORTAGE_VDB_MOUNT=sqlite|redb` — the real
  Portage consumer container gets `--device /dev/fuse`, converts its VDB,
  mounts `--rw` over `/var/db/pkg`, merges, unmounts, converts back to
  files for the normal snapshot. Pass = the usual 0 hard / 0 unexplained.

## 7. Slices

| Slice | Content |
|---|---|
| S0 | Feasibility + capture: FUSE inside a rootless bed container (`--device /dev/fuse`, container root mounts); `strace` of real `ebuild merge`/unmerge/replace on a files VDB to pin §2. Stop and ask the owner if FUSE in the container fails. |
| S1 | `--rw` flag + volatile layer (§3.1) |
| S2 | Staged entries and publish via `insert_entry` (§3.2, §3.3.1, stamp rule, counter) |
| S3 | Live-entry rewrites and `rmtree` → `delete_entry` (§3.3.2-3) |
| S4 | Store import at unmount + `vdb import-stores` (§4) |
| S5 | Host real `ebuild` merge/unmerge test |
| S6 | pmtest `L1_PORTAGE_VDB_MOUNT` + gate on sqlite and redb |
| Z | Man page, what-this-proves, backlog close-out |

Merge-path gate (AGENTS.md): this changes no portuale merge code; S6 is the
glibc + bash proof for the new path.
