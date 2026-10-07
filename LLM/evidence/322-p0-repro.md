# #322 P0 — repro of the missing-build-tree panic

Captured 2026-10-07 on branch `backlog/322` (portuale `c49aa942`, release
binary built from that commit: `rust/target/release/portuale`, 21211160 bytes,
mtime 2026-10-07 15:22:36 UTC). Image `localhost/test-portuale:latest`
(`5616d41d00cd`), always `--entrypoint /bin/bash` (the image's own entrypoint is
an `init` that exits). The "bare" runs mount **only** the binary; the "control"
runs also mount the checkout read-only at its build path.

## 0. The original report (owner, server container, ssh to a client container)

`portuale mrg -v --getbinpkgonly --remote-hostname=client --remote-user=root
--remote-key-file=/root/id --remote-vdb=client:/var/db/pkg app-portage/eix`:
preflight ok, plan printed, then

```
thread 'main' (6143) panicked at portuale/src/ebuild_phases.rs:656:10:
repo root resolves (portuale is always built from within the checkout): Os { code: 2, kind: NotFound, ... }
  ... 14: portuale::remote::run_binpkg_flow
  ... 15: portuale::remote::run_remote_plan
Aborted (core dumped)
```

`[profile.release] panic = "abort"` is why it dumps core.

## 1. Same crash without ssh or a client (local transport)

```
podman run --rm --entrypoint /bin/bash \
  -v $PWD/portuale:/usr/local/bin/portuale:ro \
  -v <pmtest>/fixtures/pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar:/fx/p.gpkg.tar:ro \
  localhost/test-portuale:latest -c 'mkdir -p /far/var/db/pkg;
  /usr/local/bin/portuale mrg --getbinpkgonly --remote-hostname=localhost \
    --remote-transport=local --remote-root=/far --remote-binpkg=/fx/p.gpkg.tar'
```

Bare binary, output and `rc=134`:

```
>>> Remote preflight localhost: ok

thread 'main' (3) panicked at portuale/src/ebuild_phases.rs:656:10:
repo root resolves (portuale is always built from within the checkout): Os { code: 2, kind: NotFound, message: "No such file or directory" }
```

The panic comes after the preflight and before the first unit ships, as in §0.

## 2. `portage_checkout()` panics the same way (not in the original entry)

Bare binary: `portuale emerge --list-sets`:

```
thread 'main' (2) panicked at portuale/src/ebuild_phases.rs:656:10:
repo root resolves (portuale is always built from within the checkout): ...
rc=134
```

## 3. Control: checkout mounted at the build path

Same command as §1 plus `-v <repo>:<repo>:ro`: `rc=0`, `STATUS=merged`,
`>>> Remote merged dev-libs/gpkgreadpkg-1.0`, vdb entry written. Same as
§2 plus the mount: `rc=0`, the set list prints. So the build-tree path is the
only missing input.

## 4. Side observation (separate from #322)

A first control run piped into `head -12` ended `rc=134` *after* a complete
merge. That was SIGPIPE: the Rust runtime ignores it, so a `println!` to the
closed pipe panics and `panic = "abort"` turns it into a core dump. Re-run
with the output in a file, the same merge is `rc=0`. Any `portuale … | head`
therefore aborts. Not part of #322; mentioned because it is another crash that
tells the user nothing.

## 5. Baseline

Test names and verdicts before any change, to compare **by name** after S1/S2:

- `cargo test --release --no-fail-fast` (whole workspace): 2296 passed, 0
  failed (`322-p0-baseline-cargo.txt`, the 2285 named `test … ok` lines,
  sorted; the other 11 are doc/harness counts without a name line).
- pmtest contract suite (`python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/… -rA`): 2248 passed, 37 skipped, 2 xfailed
  (`322-p0-baseline-pytest.txt`, the PASSED lines; the skips are the opt-in
  benchmark and musl smoke, and 35 metamorphic cases without IUSE).

Flake seen while taking it: the first `cargo test --release` (without
`--no-fail-fast`) reported `846 passed; 1 failed` in the portuale lib and so
skipped the later crates. The failing test name was lost (the filter I used
dropped the `FAILED` line); the same suite re-run was `847 passed; 0 failed`,
twice. It ran while a container control was starting, so it is one of the known
load-sensitive tests (#308, #321 class). If a post-change run shows a single
failure, re-run that test alone before suspecting the change.
