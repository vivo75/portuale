# #317 S6 — L1 gate with real Portage merging through the mount

Run 2026-10-05, portuale `f0363391` build (registry), pmtest `65567bc`
(`L1_PORTAGE_VDB_MOUNT`), cached glibc/bash binpkgs from the #305 Z.2
build run `l1-20261005T110956Z`. Plan: [`../02.317-rw-fuse.opus.md`](../02.317-rw-fuse.opus.md) Task 6.

The **real Portage 3.0.82.2** consume container (`--device /dev/fuse
--cap-add SYS_ADMIN`) converts its `/var/db/pkg` (320 entries) into the
database, mounts `portuale vdb mount --rw --root /` over `/var/db/pkg`,
and real `emerge -k --getbinpkg --oneshot --reinstall-atoms` re-merges
`sys-libs/glibc-2.43-r2` and `app-shells/bash-5.3_p15` into it; then
unmount (store import), convert back, snapshot. The portuale side merges
on files as usual.

```sh
# from ../pmtest
for k in sqlite redb; do
  L1_PORTAGE_VDB_MOUNT=$k L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 \
    differential-test-bed/run/l1-merge-from-binpkg.sh differential-test-bed/atomlists/l1-merge-gate.txt
done
```

| Run | Backend | Result |
|---|---|---|
| `l1-20261005T114439Z` (#305 Z.2) | files, no mount | merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only (baseline) |
| `l1-20261005T170210Z` | **sqlite mount** | merged 2/2, merge_rc 0, 0 hard / 0 unexplained / 0 payload, 1415 mtime-only; database after unmount: 320 installed, counter 463, nothing pending |
| `l1-20261005T170354Z` | **redb mount** | same, 1415 mtime-only; 320 installed, counter 463, nothing pending |

The 1415 mtime-only paths are **the same set** as the baseline's (both
runs, compared path by path from `portage.mtimes.tsv` / `portuale.mtimes.tsv`).

A first pair of runs (`l1-20261005T165803Z`, `…165949Z`) had one extra
mtime-only path, `/var/lib/portage/world`: the bed's export step
(`vdb convert --force … --to files:/`) rewrote the world file that real
`emerge --oneshot` had left alone. Fixed in the bed (pmtest `65567bc`
keeps that file across the export); not a mount difference.

This is the merge-path proof the spec asked for (§1 success test): glibc
and bash, whose files every process maps, merge through the read-write
view with real Portage exactly as on disk.
