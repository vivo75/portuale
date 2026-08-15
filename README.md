# Porting pilot

This is the "Suggested first execution step" pilot from [`PROMPT.md`](PROMPT.md):
a small, complete run of the whole pipeline (Rust port, Python harness,
shared black-box contract suite, multicall dispatch skeleton) on the
smallest meaningful slice, before committing to the depgraph/config/full
`emerge` surface.

## Layout

```
PORTING/
  PROMPT.md                    planning prompt this pilot implements
  rust/                        Rust workspace
    versions-harness/          port of lib/portage/versions.py (vercmp, ververify)
    multicall/                 emerge/ebuild dispatch skeleton (dry-run stub only)
  python/
    versions_harness.py        thin CLI wrapper around the real portage.versions
  bench/                       benchmark-mode timing comparison (the CI perf gate)
    dataset.py                  synthetic version-pair generator (stand-in for a
                                 real vendored Gentoo tree snapshot, see below)
    run_benchmark.py            times both harnesses' `batch` mode, reports
                                 speedup, checks/updates baseline.json
    baseline.json                recorded speedup from the last --update-baseline run
  musl/                         musl static-build smoke test (the minimal-Linux CI gate)
    Containerfile                 alpine/musl builder stage -> FROM scratch runtime stage
    smoke_test.sh                 builds the image, runs it, checked as a CI gate
  tests/                       shared, black-box pytest contract suite
    conftest.py                 builds the Rust binaries, exposes both harnesses
    test_versions_contract.py   asserts identical output, Python vs. Rust
    test_multicall.py           tests the compiled dispatch binary via symlinks
    test_benchmark_gate.py      opt-in wrapper around run_benchmark.py for CI
    test_musl_smoke.py          opt-in wrapper around musl/smoke_test.sh for CI
```

## What this proves

- **`versions-harness`**: a faithful Rust port of `vercmp`/`ververify`,
  checked against the real Python implementation through a neutral CLI
  contract (not a product CLI, not FFI/PyO3 bindings) -- see `PROMPT.md`
  hard goal 4 and the "black-box via CLI/API" decision.
- **`multicall`**: proves the `argv[0]`-based dispatch mechanism for
  shipping `emerge`+`ebuild` as one static binary (`PROMPT.md`,
  "emerge/ebuild binary shape"). Behavior is a dry-run stub only, per the
  first-port scope decision -- no dependency resolution or phase execution
  yet.
- **`PORTING/tests`**: an example of the jointly-owned contract suite
  described in `PROMPT.md` under "Ownership" -- it imports nothing from
  either implementation, driving both purely as subprocesses, so it stays
  valid regardless of how either side's internals evolve.
- **`PORTING/bench`**: the performance-regression gate from `PROMPT.md`
  hard goal 2 ("Rust must be measurably faster... tracked over time in CI
  as a regression gate"). `run_benchmark.py` feeds an identical batch of
  operations to both harnesses' `batch` subcommand (many ops per process,
  so process-spawn overhead doesn't drown out the comparison), takes the
  best of several timed repetitions per side, and refuses to report numbers
  at all if the two implementations' outputs disagree. It exits nonzero if
  Rust isn't at least `--min-speedup` times faster than Python, and
  (`--check-baseline`) if speedup regresses more than 10% below the
  recorded `baseline.json`. As of the last `--update-baseline` run, Rust is
  **~6x faster** than Python on this synthetic dataset.
- **`PORTING/musl`**: the minimal-Linux gate from `PROMPT.md` hard goal 3
  and "Test/benchmark harness architecture" ("Rust CI also gates on a musl
  static build smoke-tested inside a minimal (scratch/busybox-level)
  container"). `Containerfile` cross-builds both binaries against musl
  (Alpine's own `rust`/`cargo` packages target musl natively, so no
  rustup/target-add is needed) with `relocation-model=static` forced via
  `rust/.cargo/config.toml` -- the resulting binaries have no dynamic
  section at all, not even a reference to musl's own dynamic loader
  (verified with `ldd`/`readelf`). The runtime stage is `FROM scratch`:
  no libc, no shell, no busybox, nothing but the two binaries. `smoke_test.sh`
  builds that image and exercises `versions-harness`, `emerge`-dispatch,
  `ebuild`-dispatch, and batch mode inside it, exiting nonzero on any
  failure.

Known simplification: `versions-harness` compares version components as
`i128` rather than Python's arbitrary-precision integers. See the comment
at the top of `rust/versions-harness/src/versions.rs`.

Also a known simplification: `bench/dataset.py` generates synthetic,
seeded-random version pairs, not the "real, vendored Gentoo tree snapshot"
`PROMPT.md` calls for -- no such snapshot is vendored into this repo yet.
Swapping the generator for a real tree walk is a drop-in follow-up (same
`generate_batch_lines`-shaped interface).

## Running it

Build both Rust binaries:

```sh
cd PORTING/rust && cargo build --release
```

Try the harnesses directly:

```sh
# Python
python3 PORTING/python/versions_harness.py vercmp 1.0-r1 1.0

# Rust
PORTING/rust/target/release/versions-harness vercmp 1.0-r1 1.0

# batch mode (benchmark-oriented: many ops, one process)
printf 'vercmp 1.0 1.0\nververify 1.0_pre2\n' | PORTING/rust/target/release/versions-harness batch
```

Try the multicall skeleton:

```sh
ln -s PORTING/rust/target/release/multicall /tmp/emerge
/tmp/emerge --pretend sys-apps/foo
```

Run the contract suite (builds the Rust binaries itself; requires `cargo`
on `PATH`):

```sh
python3 -m pytest PORTING/tests -v
```

Run the benchmark / regression gate:

```sh
# report speedup, no gating
python3 PORTING/bench/run_benchmark.py --ops 200000

# CI-style: fail if speedup regressed vs. the recorded baseline
python3 PORTING/bench/run_benchmark.py --check-baseline

# record a new baseline after an intentional, reviewed perf change
python3 PORTING/bench/run_benchmark.py --update-baseline

# same gate, wrapped as a pytest for CI (skipped by default -- see
# PORTING/tests/test_benchmark_gate.py)
PORTING_RUN_BENCHMARK=1 python3 -m pytest PORTING/tests/test_benchmark_gate.py -v
```

Run the musl static-build smoke test (requires `podman` or `docker`; builds
a container image, so it needs network access for the Alpine base layer
and `apk add rust cargo` the first time):

```sh
bash PORTING/musl/smoke_test.sh

# same gate, wrapped as a pytest for CI (skipped by default -- see
# PORTING/tests/test_musl_smoke.py)
PORTING_RUN_MUSL_SMOKE=1 python3 -m pytest PORTING/tests/test_musl_smoke.py -v -s
```
