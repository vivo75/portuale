# #317 S0 — real Portage's VDB syscalls, and FUSE in a bed container

Run 2026-10-05 on the execution host. Plan: [`../02.317-rw-fuse.opus.md`](../02.317-rw-fuse.opus.md) Task 0.
Spec: [`../superpowers/specs/2026-10-05-rw-fuse-vdb-design.md`](../superpowers/specs/2026-10-05-rw-fuse-vdb-design.md).

## 1. FUSE inside a bed container

`podman_run_pm` (pmtest `run/lib.sh:72`) plus extra flags, image
`localhost/test-portuale:latest`, portuale `9fb94e43` build, as container root:

| Extra flags | Result |
|---|---|
| `--device /dev/fuse` | `vdb mount` fails: the image has no `fusermount3` (fuser's mount helper for non-root) and the direct mount is not permitted |
| `--device /dev/fuse -v /usr/bin/fusermount3:…` | `fusermount3: mount failed: Operation not permitted` |
| **`--device /dev/fuse --cap-add SYS_ADMIN`** | **works**: fuser mounts directly as root (no `fusermount3` needed); `diff -r /var/db/pkg <mount>` empty (320 entries); `touch` → `Read-only file system`; `umount <mount>` rc 0 and the daemon exits |

S6 uses `--device /dev/fuse --cap-add SYS_ADMIN` on the real-Portage
consume container only, and unmounts with `umount` (no `fusermount3` there).

## 2. Capture

`317-s0-capture.sh` (run as root from the portuale root): real Portage
3.0.82.2 `ebuild --skip-manifest` on pmtest's fixture
`dev-libs/binpkgrmpkg` (1.0 and 2.0, `SLOT=0`, one payload file, all
`pkg_*` hooks), `PORTAGE_CONFIGROOT=../pmtest/fixtures`, a scratch ROOT
with an empty files VDB, under `strace -f -y`. Steps: 1 new merge, 2 same-pf
replace, 3 other-pf replace, 4 unmerge; then 5–7 = 1, 3, 4 again with
`FEATURES=parallel-install`, the only case where real takes the in-tree slot
lock (`_slot_locked`, `vartree.py:2088-2105`, wrapping `dblink.unmerge`
`:2481` and `dblink.merge` `:6123` — the path `emerge`'s MergeProcess uses
too). Real `emerge` itself could not run here (the fixture profile is set up
for portuale: "Your current profile is invalid"); S6 runs it in the bed.

Kept per step in `317-s0-strace/`: `<step>.vdb` (every `strace` line that
names `var/db/pkg`, ROOT stripped), `<step>.ops.txt` (reduced by
`317-s0-reduce.py` to the calls that change state or lock), `<step>.out`
(command output). The raw logs (~25 MB each) are not kept; the script
regenerates them.

## 3. What real does, mapped to the spec

Every state-changing or locking VDB call in the seven steps, by shape
(`X` = `binpkgrmpkg-<v>`, `<tmp>` = an 8-character `mkstemp` suffix):

| Calls (in order within a merge) | Spec rule |
|---|---|
| `mkdir var/db/pkg` (EEXIST), `mkdir <cat>` (first merge) | §3.1 category `mkdir` (volatile until an entry lands) |
| `utimensat <cat>`, `utimensat var/db/pkg` (`_bump_mtime`, before and after each merge/unmerge; ENOENT on a missing category) | §3.4 accepted, no effect |
| `open <cat>/.<pn>:<slot>.portage_lockfile O_RDWR\|O_CREAT 0660`, `chown(-1, portage_gid)`, `fcntl F_SETLK F_WRLCK`, `unlink ..<pn>:<slot>.portage_lockfile.hardlock-<host>-<pid>` (ENOENT), `linkat lockfile -> hardlock`, `unlink hardlock`, `fcntl F_UNLCK`, `unlink lockfile` (parallel-install only) | §3.1 volatile names; **`chown` must succeed on volatile files** (real prints three "Cannot chown a lockfile" lines otherwise, `locks.py:275-297`); `fcntl` locks kernel-local |
| `mkdir <cat>/-MERGING-X 0777` | §3.2 staged |
| into `-MERGING-X`: `open <field> O_WRONLY\|O_CREAT\|O_TRUNC 0666` + `copy_file_range`/`write` + `close` (≈20 info files, `COUNTER`) | §3.2 staged files (`copy_file_range` from outside the mount: the adapter must answer so the kernel falls back to plain writes) |
| `open CONTENTS<tmp> O_RDWR\|O_CREAT\|O_EXCL\|O_NOFOLLOW 0600`, write, `open CONTENTS<tmp>_umask_test O_CREAT` + `unlink` it, `chmod CONTENTS<tmp> 0644`, `rename CONTENTS<tmp> -> CONTENTS`, `unlink CONTENTS<tmp>` (ENOENT) — same for `metadata<tmp>` | §3.2 (`write_atomic` inside the staged dir) |
| `open metadata O_WRONLY\|O_CREAT\|O_APPEND`, write 31 bytes (the `#dir_mtime=` stamp) | §3.2; no `utime` of the staged dir anywhere, so the stamp is the dir mtime after the last rename inside it — the staged-dir mtime rule makes it Valid at publish |
| replace: `unlinkat <cat>/<old>/<file>` for every file (shutil.rmtree, fd-relative), `rmdir <cat>/<old>`, `rmdir <cat>` → ENOTEMPTY | §3.3.3 hide + `delete_entry`; category `rmdir` refused while a staged dir exists |
| `rename <cat>/-MERGING-X -> <cat>/X` | §3.3.1 publish |
| after publish, from the `pkg_postinst` bash: `open <cat>/X/environment.bz2 O_WRONLY\|O_CREAT\|O_TRUNC 0666`, write, close (`PORTAGE_UPDATE_ENV`) | §3.3.2 live rewrite on close |
| unmerge: `unlinkat` every file, `rmdir <cat>/X`, `rmdir <cat>` (succeeds when empty) | §3.3.3, then §3.1 category `rmdir` |

Not seen in these steps (preserved libs, blockers): the W4
`removeFromContents` rewrite of another entry's `CONTENTS`. It is
`write_atomic` (`vartree.py:3823`, `util/__init__.py` `atomic_ofstream`),
the same `<tmp>` + `chmod` + `rename` shape as above but inside a **live**
entry: §3.3.2 (temp kept in scratch until the rename onto `CONTENTS`).

**No call falls outside the spec's rules.** Two refinements for Tasks 1–3
(not spec changes): volatile files accept `chown` (owner kept in memory),
and the adapter must handle `copy_file_range` into a staged file (reply
`ENOSYS`/`EOPNOTSUPP` so the kernel or Python falls back to `write`).
