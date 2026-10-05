# #317 S5 — real `ebuild` merge and unmerge through a `--rw` mount

Run 2026-10-05 on the execution host, portuale `198b32e0` build, real
Portage 3.0.82.2. Plan: [`../02.317-rw-fuse.opus.md`](../02.317-rw-fuse.opus.md) Task 5.

`317-s5-host.sh` runs four real commands on pmtest's fixture
`dev-libs/binpkgrmpkg` (1.0 new merge, 1.0 again = same-pf replace, 2.0 =
other-pf replace, 2.0 unmerge), each twice: on a plain files ROOT, and on a
ROOT whose `var/db/pkg` is `portuale vdb mount --rw --root <ROOT>` over a
sqlite (then redb) database, mounted for the command and unmounted after
it (so the store import runs every step). After each step the database is
converted back to files and compared with the plain run by
`317-s5-compare.py`: same entries, file names, modes, directory mode,
metadata-stamp state (`#dir_mtime=` equal to the entry directory mtime),
and bytes after normalising what differs between any two runs
(`BUILD_TIME`, the stamp value, the mtime column of `CONTENTS`, and
`environment.bz2`, whose saved environment carries the ROOT path; compared
by presence). The counter is compared too.

## Result: equal at every step on both backends

```text
== sqlite
  step 1: ebuild binpkgrmpkg-1.0.ebuild merge
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    compare: equal (1 entries)
    counter: files 0, sqlite 0
  step 2: ebuild binpkgrmpkg-1.0.ebuild merge
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    compare: equal (1 entries)
    counter: files 1, sqlite 1
  step 3: ebuild binpkgrmpkg-2.0.ebuild merge
    binpkgrmpkg-2.0.ebuild merge: rc 0, 0 error lines
    binpkgrmpkg-2.0.ebuild merge: rc 0, 0 error lines
    compare: equal (1 entries)
    counter: files 2, sqlite 2
  step 4: ebuild binpkgrmpkg-2.0.ebuild unmerge
    binpkgrmpkg-2.0.ebuild unmerge: rc 0, 0 error lines
    binpkgrmpkg-2.0.ebuild unmerge: rc 0, 0 error lines
    compare: equal (0 entries)
    counter: files 2, sqlite 2
== redb
  step 1: ebuild binpkgrmpkg-1.0.ebuild merge
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    compare: equal (1 entries)
    counter: files 0, redb 0
  step 2: ebuild binpkgrmpkg-1.0.ebuild merge
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    binpkgrmpkg-1.0.ebuild merge: rc 0, 0 error lines
    compare: equal (1 entries)
    counter: files 1, redb 1
  step 3: ebuild binpkgrmpkg-2.0.ebuild merge
    binpkgrmpkg-2.0.ebuild merge: rc 0, 0 error lines
    binpkgrmpkg-2.0.ebuild merge: rc 0, 0 error lines
    compare: equal (1 entries)
    counter: files 2, redb 2
  step 4: ebuild binpkgrmpkg-2.0.ebuild unmerge
    binpkgrmpkg-2.0.ebuild unmerge: rc 0, 0 error lines
    binpkgrmpkg-2.0.ebuild unmerge: rc 0, 0 error lines
    compare: equal (0 entries)
    counter: files 2, redb 2
```

Every real command exited 0 with no `ERROR`/`FAILED`/`Traceback`/"not
permitted" line, through the mount as on disk. The pkg_* hook chain
(`var/lib/binpkgrmpkg.log`) is the same as real's own run on files.

Found while getting here (fixed in S2/S3, each with a test): the kernel
kept dentries for staged files across the publish rename (fixed: publish
keeps every inode), and the in-place `> environment.bz2` of
`pkg_postinst` arrived as `setattr(size=0)` before `open` (fixed: the
mount asks for `FUSE_ATOMIC_O_TRUNC`).
