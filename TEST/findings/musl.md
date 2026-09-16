# musl smoke findings — the #61 builder/source walls

Bed: `musl/Containerfile` + `musl/smoke_test.sh` (opt-in gate,
`PORTUALE_RUN_MUSL_SMOKE=1`, `tests/test_musl_smoke.py`).
Environment: podman 6.1.0, 2026-09-16, tree `main` @ `580f72f`.
Plan: [`docs/07.61-musl-smoke-toolchain-drift.opus.md`](../docs/07.61-musl-smoke-toolchain-drift.opus.md).
Discovery: `l2.md` "## #58 S4" §"musl smoke — blocked, pre-existing".

## S0 — reproduction, three walls in order

### Wall 1 — resolved MSRV abort (base image too old)

`alpine:3.22.1` ships rustc 1.87.0 (`apk add rust cargo`). The
Containerfile's five-package build includes `portuale`, whose graph
reaches `portage-atom*` / `gentoo-interner`, all at
`rust-version = 1.95`. Cargo aborts before compiling:

```
$ podman run --rm -v <tree>:/work:Z docker.io/library/alpine:3.22.1 \
    sh -c 'apk add --no-cache rust cargo; cd /work/rust; \
    cargo build --release --target x86_64-alpine-linux-musl --package portuale'
error: rustc 1.87.0 is not supported by the following packages:
  brush-builtins@0.2.0 requires rustc 1.88.0
  ...
  gentoo-interner@0.4.1 requires rustc 1.95
  portage-atom@0.11.1 requires rustc 1.95
  portage-atom-pubgrub@0.8.0 requires rustc 1.95
  portage-atom-resolvo@0.8.1 requires rustc 1.95
  portage-solver@0.3.0 requires rustc 1.95
  pubgrub@0.4.0 requires rustc 1.92
  smol_str@0.3.6 requires rustc 1.89
  uucore@0.10.0 requires rustc 1.88.0
Either upgrade rustc or select compatible dependency versions ...
```

Per-build-graph, not per-workspace: on the same base,
`--package versions-harness` builds fine (its graph has none of those
crates). The Containerfile's set always includes `portuale`.

### Wall 2 — static link abort (newer toolchain)

`alpine:edge` (rustc 1.98.1) resolves everything; the
`x86_64-alpine-linux-musl` link fails identically on `alpine:3.24.1`
(rustc 1.96.1) without the fix — so it is *not* the version bump:

```
= note: .../x86_64-alpine-linux-musl/bin/ld: attempted static link of
  dynamic object `.../lib/libc.so'
collect2: error: ld returned 1 exit status
error: could not compile `hello` (bin "hello") due to 1 previous error
```

Root cause, from `-C link-arg=-Wl,-v` (hello-world, same rustflags as
`rust/.cargo/config.toml`), the effective link line has rustc's wrapper

```
... -lgcc_eh -lc <sysroot>/.../liblibc-*.rlib -lc <rlibs>
    -Wl,-Bdynamic -Wl,--eh-frame-hdr ... -static -no-pie ...
```

and gcc appends its own libraries **after** that `-Wl,-Bdynamic`:

```
-lssp_nonshared --start-group -lgcc -lgcc_eh -lc --end-group .../crtend.o .../crtn.o
```

So the final `-lc` is searched dynamically (finds `/usr/lib/libc.so`)
while the global `-static` is in force, and GNU ld refuses. The target
spec explains why only this target is affected: `x86_64-alpine-linux-musl`
has `"crt-static-respected": true` but no `"crt-static-default"` (Alpine
is a dynamic-musl distro); `x86_64-unknown-linux-musl` defaults static
but its std is not shipped by the Alpine `rust` apk (sysroot ships only
`x86_64-alpine-linux-musl`).

**Fix:** append `"-C", "link-arg=-Wl,-Bstatic"` to the
`x86_64-alpine-linux-musl` rustflags. Rustc places link-args last among
its own arguments, i.e. after its `-Wl,-Bdynamic`, so gcc's appended
`-lc` (and `-lssp_nonshared`/`-lgcc`/`-lgcc_eh`) resolve statically.
Result on both `alpine:3.24.1` and `alpine:edge` (hello-world):
`readelf -l` has no `INTERP`, `readelf -d` prints "There is no dynamic
section in this file", and the binary runs.

### Wall 3 — `libc::sched_param` literal is glibc-shaped

With the builder unblocked, `portuale` fails to compile for musl:

```
error[E0063]: missing fields `sched_ss_init_budget`, `sched_ss_low_priority`,
              `sched_ss_max_repl` and 1 other field in initializer of `sched_param`
    --> portuale/src/pretend.rs:3774:17
     |
3774 |     let param = libc::sched_param {
     |                 ^^^^^^^^^^^^^^^^^ missing ...
```

glibc's `struct sched_param` is a single `sched_priority`; musl's
carries the four `SCHED_DEADLINE` members. Pre-existing source bug,
hidden all along by wall 1. Real Portage calls
`os.sched_setscheduler(pid, policy, os.sched_param(priority))`
(`3rdparty/portage/lib/_emerge/actions.py:3379`), and CPython converts
that to `struct sched_param` as a C11 compound literal with only
`sched_priority` set — every other field zero. Ported as
`unsafe { std::mem::zeroed() }` + `param.sched_priority = priority`,
which is correct on both libcs.

## S0 fix validation (replica tree, `main` @ `580f72f` + the two fixes)

```
$ cargo build --release --target x86_64-alpine-linux-musl \
    --package versions-harness --package atom-harness \
    --package use-reduce-harness --package required-use-harness --package portuale
    Finished `release` profile [optimized] target(s) in 2m 10s
RC=0
versions-harness       INTERP=0 NEEDED=0
atom-harness           INTERP=0 NEEDED=0
use-reduce-harness     INTERP=0 NEEDED=0
required-use-harness   INTERP=0 NEEDED=0
portuale               INTERP=0 NEEDED=0
```

(`INTERP` = `readelf -l | grep -c INTERP`, `NEEDED` = `readelf -d |
grep -c NEEDED`; zero on every shipped binary.)

One warning remains on musl only: `portuale/src/elog.rs:595` casts to
`libc::time_t`, deprecated in the musl libc definitions. Warning-only
(no `-D warnings` in the Containerfile), left as-is.

## Base-image / toolchain matrix (2026-09-16)

| base | rustc | cargo resolve (1.95 floor) | static link (no flag) | static link (+ flag) |
|---|---|---|---|---|
| `alpine:3.22.1` (pinned before) | 1.87.0 | abort (wall 1) | n/a | n/a |
| `alpine:3.23` | 1.91.1 | abort (1.91 < 1.95) | n/a | n/a |
| `alpine:3.24.1` (chosen) | 1.96.1 | ok | abort (wall 2) | **ok, fully static** |
| `alpine:edge` | 1.98.1 | ok | abort (wall 2) | ok, fully static |

`3.23` was checked at version level only (1.91.1 < the 1.95 floor);
`3.24.1` and `edge` were each built end-to-end for hello-world and are
the two candidates for the flag fix. `3.24.1` chosen: newest **stable**
release clearing the floor, so the base stays a stable pin.

## S1 — builder fix (alpine:3.24.1 + trailing `-Wl,-Bstatic`)

On a replica of the S1 tree in `alpine:3.24.1` (rustc 1.96.1), the four
harnesses build and link fully static; `portuale` stops at the known
wall 3 (`E0063 sched_param`, fixed in S2):

```
$ cargo build --release --target x86_64-alpine-linux-musl \
    --package versions-harness --package atom-harness \
    --package use-reduce-harness --package required-use-harness
    Finished `release` profile [optimized] target(s) in 20.22s
harness RC=0
versions-harness       INTERP=0 NEEDED=0
atom-harness           INTERP=0 NEEDED=0
use-reduce-harness     INTERP=0 NEEDED=0
required-use-harness   INTERP=0 NEEDED=0
$ cargo build ... --package portuale
error[E0063]: missing fields `sched_ss_init_budget`, `sched_ss_low_priority`,
              `sched_ss_max_repl` and 1 other field in initializer of `sched_param`
```

## S2 — `sched_param` portability

Same command with S2 applied: full five-package build, rc 0, all static:

```
    Finished `release` profile [optimized] target(s) in 3m 15s
RC=0
versions-harness       INTERP=0 NEEDED=0
atom-harness           INTERP=0 NEEDED=0
use-reduce-harness     INTERP=0 NEEDED=0
required-use-harness   INTERP=0 NEEDED=0
portuale               INTERP=0 NEEDED=0
```

`std::mem::zeroed()` + `param.sched_priority = priority` is byte-for-byte
CPython's own conversion: real passes `os.sched_param(priority)` to
`os.sched_setscheduler` (`actions.py:3379`) and CPython builds the C
struct as a compound literal with only `sched_priority` set. The
existing host test `test_emerge_applies_portage_scheduling_policy`
covers the runtime path unchanged.

## S3 — MSRV guard

With `rust-version = "1.95"` workspace-wide, the minimal
`versions-harness` build on `alpine:3.22.1` (rustc 1.87) now fails at
the member level, before any dependency resolution or compile:

```
error: rustc 1.87.0 is not supported by the following packages:
  harness-common@0.1.0 requires rustc 1.95
  portage-versions@0.1.0 requires rustc 1.95
  versions-harness@0.1.0 requires rustc 1.95
```

## S4/S5 — full smoke gate green, warning-free

```
$ bash musl/smoke_test.sh
...
musl smoke test: PASS (image localhost/portage-rust-musl-smoke:pilot)
```

23/23 checks, zero `FAIL` lines; the 3m16s builder log has zero
`warning:` lines (S5 removed the musl-only `libc::time_t` deprecation).
The static claim on the shipped artifact, from the same image:

```
$ cid=$(podman create localhost/portage-rust-musl-smoke:pilot)
$ podman cp "$cid:/bin/portuale" /tmp/portuale.musl && podman rm "$cid"
$ readelf -l /tmp/portuale.musl | grep -c INTERP   # 0
$ readelf -d /tmp/portuale.musl | grep -c NEEDED   # 0
```

### The stale assertions S4 replaced

`smoke_test.sh` had not run end to end since the builder broke; its
exact-match pins were pilot-era. The table is what S4 rewrote from
(structural: rc + exact cpv order + markers):

| check | script expected (pilot) | current binary |
|---|---|---|
| merge list | `[ebuild  N] pkg-1.0` | `[ebuild  N     ] pkg-1.0 ` (padded, trailing space) |
| order | target first | dependencies first, target last |
| USE | no column | `USE="foo -missingflag"` / `USE="-foo"` |
| masked+unmasked | `[ebuild  N]` | `[ebuild  N    #]` |
| blocks | `[blocks] a hard blocks b ("!!b")` | `[blocks B      ] b ("b" is hard blocking a)`, rc 1 + real's blocked-packages error |
| slot conflict | a `[slot conflict]` block | settles on `slotconflicttarget-1.0` (contract-pinned) |
| virtual | 3 entries incl. newpkg | `virtual/texteditor-0`, `virtualconsumerpkg` |
| REQUIRED_USE | old single-line grep | real's `!!! The ebuild selected … has unmet requirements` block |
| `--jobs` | "real option, unimplemented" | implemented; now checks `--nobindeps`' refusal instead |
| ebuild `merge` | "pilot stub" text | `ebuild --help` usage + the bad-phase refusal |

`set -e` aborted the old script at the first expected-nonzero rc
(`blockerpkg`); every command now goes through `capture()`, so later
checks always run. Byte-exact output stays
`tests/test_emerge_pretend_contract.py`'s job.
