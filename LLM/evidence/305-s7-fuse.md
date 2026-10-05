# #305 (feat#157) — S7.4 FUSE view with outside tools

**Status: run 2026-10-05 on the execution host after `/dev/fuse` appeared**
(the first attempt, `ec3b5d0d`, found no device). Build: portuale
`bfc8c1ca` (default features: `vdb-sqlite`, `vdb-redb`, `vdb-fuse`).
Host VDB: 2130 entries in 102 categories, world 207 atoms, counter 23336.
Tools: Portage 3.0.82.2, portage-utils 0.97.1, eix 0.36.9, gentoolkit
equery 0.8.1.

**Result: every outside tool gives the same output through the view as
from the original `/var/db/pkg`,** except a timing line and one
readdir-order effect in `qsize` (both explained below).

## How it was run

The view is mounted inside a private mount namespace (`sudo unshare -m`,
`mount --make-rprivate /`) and bind-mounted over `/var/db/pkg`, so each
tool reads it at its normal path and the host is never touched.
`/var/cache/edb` is bind-mounted from a copy in both runs, so an
`emerge` as root cannot write its `vdb_metadata.pickle` into the host.
The "orig" run uses the same namespace setup without the view.

```sh
B=portuale
$B vdb convert --from files:/ --to sqlite:$S/vdb.sqlite
# converted 2130 entries (files -> sqlite); world 207 atoms, 0 sets; preserved libs 0; config memory 0; counter 23336  (3.4 s)

# ns.sh, run as: sudo unshare -m sh ns.sh $S $B {orig|view}
mount --make-rprivate /
mkdir -p $S/edb.$MODE; cp -a /var/cache/edb/. $S/edb.$MODE/; mount --bind $S/edb.$MODE /var/cache/edb
if [ "$MODE" = view ]; then
  $B vdb mount --allow-other sqlite:$S/vdb.sqlite $S/mnt/vdb
  mount --bind $S/mnt/vdb /var/db/pkg
fi
sh tools.sh $S/out.$MODE
```

`findmnt /var/db/pkg` inside the view run:
`/var/db/pkg sqlite:…/vdb.sqlite fuse ro,nosuid,nodev,noatime,user_id=0,group_id=0,allow_other`.

## Outside tools, original vs view

| command | lines | result |
|---|---|---|
| `qlist -IvS \| sort` | 2130 | identical |
| `qlist -e app-shells/bash` | 28 | identical |
| `qlist -IvF '%{CATEGORY}/%{PN}:%{SLOT} %{REPO} %{BUILD_TIME}'` | 2130 | identical |
| `qdepends -Q sys-libs/zlib` | 2 | identical |
| `qfile /bin/bash /usr/lib64/libz.so.1 /etc/portage` | 6 | identical |
| `qcheck sys-apps/coreutils app-shells/bash sys-libs/glibc` (CONTENTS md5/mtime vs live fs) | 6 | identical |
| `qsize` / `qsize -s` | 2130 / 2131 | 2 lines differ (readdir order, below) |
| `eix -I -c --nocolor` | 2086 | identical |
| `equery list '*'` | 2130 | identical |
| `equery belongs /bin/bash` | 1 | identical |
| `equery depends sys-libs/zlib` | 2 | identical |
| `emerge -p @world` (rc 0) | 418 | identical but `Dependency resolution took 8.23 s` vs `9.45 s` |
| `emerge -puDN @world` (rc 0) | 28 | identical but `17.93 s` vs `19.25 s` |

Wall time of the whole tool list: 55 s original, 62 s through the view.

**`qsize` difference.** `kde-plasma/plasma-login-manager` and
`x11-misc/lightdm` both list
`/usr/share/dbus-1/system.d/org.freedesktop.DisplayManager.conf`;
`qsize` counts a shared inode once, against whichever package it visits
first, and marks the other `(N unique)`. It visits in readdir order: the
host filesystem returns categories in hash order (`x11-misc` 16th,
`kde-plasma` 54th), the view returns them sorted (`kde-plasma` 50th,
`x11-misc` 102nd). POSIX gives readdir no order; nothing reads the data
differently.

## The view itself

- `diff -r /var/db/pkg <view>` is empty for the sqlite view, the redb
  view, and the `files:/` passthrough view.
- `find -printf '%p %y %m %s %T@'` over both trees: every file matches in
  mode, size and nanosecond mtime, and every entry directory matches in
  mode and mtime (sqlite view and `files:/` passthrough). Directory
  sizes differ (filesystem-specific). Category and root directory mtimes
  differ by design (S7.3: a category's mtime is the latest change inside
  it; the host's also moves on entry removals the database no longer
  records).
- Writes: `touch <view>/x`, `mkdir <view>/sys-apps/zz` and an append to
  an entry's `SLOT` all fail with `Read-only file system`.

## redb while mounted

The mount opens redb read-only (`RedbDb::open_readonly`, a shared lock),
so readers coexist with it and writers are refused:

| while `vdb mount redb:…` is up | result |
|---|---|
| `mrg --pretend --vdb-backend=redb --vdb-path=… @world` | runs, rc 0 |
| `portuale vdb status redb:…` | runs, rc 0 |
| `portuale vdb sweep --remove zz-none/x-1 redb:…` | `database is already open (redb allows only one process …)`, rc 2 |
| `portuale vdb convert --force --from files:/ --to redb:…` | same Busy message, rc 2 |
| `vdb sweep` after `fusermount3 -u` | opens (`zz-none/x-1 is not pending`), rc 2 |

The first version of this note expected `Busy` for `mrg --pretend`; that
was wrong (a pretend run opens read-only). The `vdb --help` text listed
a FUSE mount among the holders that make any open Busy; it now says
that only a write is refused and readers share the file.

## Not covered here

- Changes made to the database while it is mounted (residue R17's
  limits: a same-size + same-mtime rewrite under a big-file handle; a
  `files` in-place `replace_file` until a directory mtime moves). No
  outside tool writes the VDB through the view, so the S7.4 commands
  cannot reach them.
