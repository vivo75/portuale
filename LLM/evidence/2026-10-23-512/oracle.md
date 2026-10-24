# #512 oracle: scheduler items (Portage 3.0.83-r1, host `emerge`)

Probe: `probe.sh` (hermetic `ROOT=/var/tmp/ev512-tree`, repo `mw512`,
profile `packages`: `*sys-libs/glibc`, `*app-misc/sysextra`; fast
`src_compile` sleeps 2 s, slow 20 s, cycle 8 s, pretend 5 s), `sigint.sh`,
`cgroup.sh`. Host `lib/_emerge/Scheduler.py` and
`lib/_emerge/_find_deep_system_runtime_deps.py` are byte-identical to the
`portage-3.0.83` tag (`3rdparty/portage`); the `-r1` patch touches only
`bintree.py`. Upstream commits: `3b1fc4238` (scope), `1c8b75d73` +
`5157db0d2` (parallel pretend), `3d677b7b2` (cycle post-merge, bug
452172), `8ac550470` + `bfa615f06` (SIGINT, bug 961044), `55beac58c` +
`a6fca880a` (cgroup text).

Upstream white-box tests (statement of intent; `testMergeWaitScope*`
both pass against the tag checkout with the tests' own `.gnupg` keys):
`lib/portage/tests/resolver/test_merge_wait_scope.py` (deep
`{glibc, sysextra, runtime-dep, deeper-runtime-dep}`, system
`{glibc, sysextra}`, toolchain `{glibc}`, none `{}`, default `deep`,
bogus raises `ValueError`; option parsing: four values accepted, unset by
default, bare flag and missing value rejected),
`lib/portage/tests/emerge/test_pkg_pretend.py` (timestamped markers:
phases overlap, each package's three markers consecutive = mergelist
replay), `lib/portage/tests/emerge/test_sigint_merge_wait.py` (rc 130,
no `Installing (` after SIGINT).

## Item (1): barrier scope in `-j` merge order

`FEATURES="-merge-wait"` everywhere except shape 05 (the help text says
the scope only matters for packages not held back by `FEATURES="merge-wait"`
anyway). `>>> Installing` timestamps (`-j2`):

- 01 deep: slow `:11`, deeper `:11`, runtime `:14`, glibc `:16`;
  `>>> Jobs:` shows `1 merge wait`. Everything `@system`-deep waits.
- 02 system: deeper `:47`, runtime `:50` (parallel, no barrier); slow
  `:05`, glibc `:05` with `1 merge wait`.
- 03 toolchain: same shape as 02 (glibc is toolchain, the rest is not).
- 04 none: deeper `:02`, runtime `:05`, glibc `:08` — all parallel.
- 05 default `FEATURES` (merge-wait on): deeper waits for slow (`:33`),
  then everything serialises (`2 merge wait` seen); scope moot.
- 06 deep closure: sysextra + deeper wait (`:30` with slow); runtime
  `:33`, glibc `:36`.
- 07 system closure: deeper `:07`, runtime `:10` parallel; slow `:27`,
  sysextra + glibc `:28`.
- 08 toolchain/sysextra: sysextra `:00` parallel with slow (`:18`).
- 11 bogus: rc 2, stderr `emerge: error: argument --merge-wait-scope:
  invalid choice: 'bogus' (choose from 'deep', 'system', 'toolchain',
  'none')` (usage block on stdout).
- 12 missing value: rc 2, stderr `emerge: error: argument
  --merge-wait-scope: expected one argument` (usage block on stdout).

## Item (2): parallel `pkg_pretend` with defined output ordering

Shape 10 (`-j2`, both pretends sleep 5 s): ppA BEGIN/BODY/END block then
ppB block (mergelist replay), all before any `>>> Emerging`; embedded
`date +%s.%N` stamps overlap (ppA `...968.188`, ppB `...968.278`, both
END `...973.19/28`). Output on stdout (`einfo` ` * ` markers keep their
colour codes even under `--color=n`).

## Item (3): post-merge dependencies in a cycle are not waited on

Shape 09 (`cycA` RDEPEND `cycB`, `cycB` PDEPEND `cycA`, `-j2`): cycB
Emerging `:46`, Installing `:54`; cycA Emerging `:55`, Installing `:03`
— B first, rc 0. The builds do not overlap here (cycA hard-waits cycB's
merge); the observable contract is the B-first order plus success.

## Item (4): SIGINT during merge-wait leaves a clean state

`sigint.sh` (shape 13): fast sysextra + slow, `-j2`, default `FEATURES`,
SIGINT the moment the first `merge wait` `>>> Jobs:` line appears
(`13-sigint.<pm>.killts` records the second; `.ts` is a timestamped side
copy since the primary stays raw so `$!` is emerge itself).
Portage (`13-sigint.portage.*`): the first `merge wait` line prints at
idle-drain time (`:36`, both queued), SIGINT the same second: rc 130,
the in-flight `Installing (1 of 2) sysextra` runs to `Completed`, and
slow never starts (no `Installing (2 of 2)`, no VDB entry) — the queue
does not drain after terminate (commit `8ac550470`, bug 961044). The
upstream white-box test (`test_sigint_merge_wait.py`, same bug) pins the
earlier variant: SIGINT strictly mid-wait gives rc 130 with no
`Installing (` line at all.
portuale (`13-sigint.portuale.*`, post-fix binary): SIGINT strictly
mid-wait (`1 running, 1 merge wait` queued, slow compiling): rc 130,
zero `Installing` lines, empty VDB. The killed `sleep 20` surfaces as a
transient `1 failed` in the last `>>> Jobs:` lines before the
`Exiting on signal 2` exit — same rc and state as Portage.

## Item (5): cgroup log text with `--verbose`

`cgroup.sh` (shape 14): `FEATURES=cgroup`, `-j2 --verbose`, one fast
package. As vivo the shape degrades silently: `/sys/fs/cgroup` is not
writable (`Permission denied`), no `CgroupManager` leaf is created, rc 0
with no `Resource usage` anywhere (`14-cgroup-degraded.portage.*`). As
root the real text appears on stdout (the background `--verbose` echo,
commit `a6fca880a`), rc 0 (`14-cgroup.portage.*`, plus `.cast`):

```text
=== Resource usage for build of app-misc/sysextra-1.0: CPU: 0.68s (0.23x), MaxMem: 22.66 MiB, MaxSwap: 0.00 B, I/O R: 68.00 KiB, I/O W: 0.00 B
=== Resource usage for install of app-misc/sysextra-1.0: CPU: 0.25s, MaxMem: 16.45 MiB, MaxSwap: 0.00 B
```

i.e. `=== Resource usage for {build,install} of {cpv}: {rendered}`
(commit `55beac58c`; previously `=== Resource usage for {cpv}
[{rendered}]`). portuale has no cgroup accounting at all (no
`CgroupManager`, no `FEATURES=cgroup` handling), so there is no
black-box shape in which it could emit true counters: residue.

## Post-fix portuale behaviour (same `captures/`, release binary with #512)

`compare.sh` over shapes 01-12: rc equal on every row; `>>> Emerging` /
`>>> Installing` order equal on every row (modulo timestamps). Spot
timestamps: 01 deep — deeper queued at `:00` (`1 running, 1 merge wait`),
slow Installing `:18`, deeper Installing `:18`, runtime `:21`, glibc
after; 02/03 — deeper/runtime parallel, glibc under `1 merge wait` with
slow's; 04 none — glibc before slow; 05 default `FEATURES` — everything
queues (`2 merge wait` seen); 06/07/08 — the closure splits as Portage
shows them; 09 — cycB `:20/:28`, cycA `:28/:36`; 10 — ppA block then ppB
block before any `>>> Emerging`, embedded stamps overlapping by ~5 s;
11/12 — rc 2, same last stderr line. Shape 13 (`sigint.sh`, post-fix
binary): SIGINT strictly mid-wait gives rc 130, zero `Installing`
lines, empty VDB. Shape 14 (`cgroup.sh`, post-fix binary): rc 0, no
`Resource usage` anywhere (residue #543).

## Task 8 shapes (15-17, backlog #544-#546; same `captures/`)

- 15-pretend-j1: serial `-j1` replays ppA block then ppB block (~5 s
  apart, unbuffered in order), both before any `>>> Emerging`, on both
  managers (rc 0). Pre-Task-8 portuale interleaved pretend per build.
- 16-pretend-buildpkgonly: `--buildpkgonly -j2` replays both blocks
  (overlapping ~5 s) before any `>>> Emerging`; both binpkgs land in
  `$PKGDIR`, on both managers (rc 0). Pre-Task-8 portuale interleaved
  pretend per entry build.
- 17-mixed-pretend: `-k --jobs=2` with ppA prebuilt (binary) and ppB
  source: the binary block (from the binpkg metadata) then the source
  block, overlapping, before `>>> Emerging binary`, on both managers
  (rc 0). Pre-Task-8 portuale ran source pretend up front but binary
  pretend inline at merge time.
- All three shapes also carry Portage's `>>> Running pre-merge checks
  for <cpv>` head line per block on both managers (Task-8 addition to
  the pass output, same text both sides).

## Pre-fix portuale behaviour (`captures/prefix/`, release binary before #512)

- Shapes 01-08: no barrier — every `Installing` lands while the slow build
  still runs (e.g. 01: deeper `:11`, runtime `:13`, glibc `:15`, slow
  `:29`), and no `merge wait` ever appears in `>>> Jobs:`.
- Shape 09: rc 0 with no `>>> Emerging`/`>>> Installing` line at all — a
  merge cycle dispatches nothing.
- Shape 10: no `PRETEND-*` line on stdout — `pkg_pretend` runs inside each
  build's captured phase chain.
- Shapes 11/12: already match (rc 2, same last stderr line; S4).
