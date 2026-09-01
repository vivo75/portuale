#!/usr/bin/env python3
"""Benchmark-mode timing comparison for the versions-comparison pilot.

Drives each harness's `batch` subcommand -- a single process handling many
operations read from stdin -- specifically to avoid fork/exec overhead
dominating the measurement (see PORTING/PROMPT.md, "Test/benchmark harness
architecture"). By default the workload is drawn from a real, vendored
Gentoo tree snapshot (dataset.py / gentoo_snapshot.json, produced by
extract_snapshot.py), per PROMPT.md's benchmark-data decision; pass
`--dataset synthetic` to fall back to seeded-random version strings. This
is the "regression gate" tool: it exits nonzero if Rust isn't measurably
faster than Python (PROMPT.md hard goal 2), or if given --check-baseline,
if speedup has regressed vs. a recorded baseline.

Example:
    python3 PORTING/bench/run_benchmark.py --ops 200000
    python3 PORTING/bench/run_benchmark.py --update-baseline
    python3 PORTING/bench/run_benchmark.py --check-baseline
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
RUST_DIR = REPO_ROOT / "rust"
RUST_BIN = RUST_DIR / "target" / "release" / "versions-harness"
PYTHON_HARNESS = [
    sys.executable,
    str(REPO_ROOT / "python" / "versions_harness.py"),
]
BASELINE_PATH = Path(__file__).resolve().parent / "baseline.json"

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dataset import (
    DEFAULT_SNAPSHOT_PATH,
    generate_snapshot_lines,
    generate_synthetic_lines,
)


def build_rust_binary() -> None:
    subprocess.run(
        ["cargo", "build", "--release", "--package", "versions-harness"],
        cwd=RUST_DIR,
        check=True,
    )


def time_batch(cmd: list[str], stdin_data: str, repeat: int) -> tuple[float, str]:
    """Runs `cmd batch` over `stdin_data` `repeat` times and returns the
    best (minimum) wall-clock time along with the captured output, so the
    caller can sanity-check that both implementations agree before
    reporting numbers for either of them."""
    best = None
    output = None
    for _ in range(repeat):
        start = time.perf_counter()
        result = subprocess.run(
            [*cmd, "batch"], input=stdin_data, capture_output=True, text=True, check=True
        )
        elapsed = time.perf_counter() - start
        if best is None or elapsed < best:
            best = elapsed
            output = result.stdout
    return best, output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--ops", type=int, default=200_000, help="number of operations in the batch"
    )
    parser.add_argument(
        "--repeat",
        type=int,
        default=5,
        help="timed repetitions per implementation; the minimum is reported",
    )
    parser.add_argument("--seed", type=int, default=0, help="dataset seed")
    parser.add_argument(
        "--dataset",
        choices=["snapshot", "synthetic"],
        default="snapshot",
        help="'snapshot' (default) draws from the vendored real Gentoo tree "
        "snapshot (gentoo_snapshot.json); 'synthetic' uses seeded-random "
        "version strings instead (fallback if the snapshot is unavailable)",
    )
    parser.add_argument("--json", type=Path, help="write results as JSON to this path")
    parser.add_argument(
        "--min-speedup",
        type=float,
        default=1.0,
        help="fail if rust isn't at least this many times faster than python "
        "(PROMPT.md hard goal 2 requires >1.0)",
    )
    parser.add_argument(
        "--check-baseline",
        action="store_true",
        help="fail if speedup regresses more than 10%% below baseline.json",
    )
    parser.add_argument(
        "--update-baseline",
        action="store_true",
        help="record this run's results as the new baseline.json",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="assume the release rust binary is already built",
    )
    args = parser.parse_args()

    if not args.skip_build:
        build_rust_binary()

    if not RUST_BIN.exists():
        print(f"error: rust binary not found at {RUST_BIN}", file=sys.stderr)
        return 1

    if args.dataset == "snapshot":
        if not DEFAULT_SNAPSHOT_PATH.exists():
            print(
                f"error: {DEFAULT_SNAPSHOT_PATH} not found; re-vendor it with "
                "extract_snapshot.py, or pass --dataset synthetic",
                file=sys.stderr,
            )
            return 1
        lines = generate_snapshot_lines(args.ops, seed=args.seed)
    else:
        lines = generate_synthetic_lines(args.ops, seed=args.seed)
    stdin_data = "\n".join(lines) + "\n"

    python_time, python_output = time_batch(PYTHON_HARNESS, stdin_data, args.repeat)
    rust_time, rust_output = time_batch([str(RUST_BIN)], stdin_data, args.repeat)

    if python_output != rust_output:
        print(
            "error: harness outputs differ -- refusing to report a benchmark for "
            "implementations that disagree; run the contract suite (PORTING/tests) "
            "to find the mismatch first",
            file=sys.stderr,
        )
        return 1

    speedup = python_time / rust_time
    result = {
        "dataset": args.dataset,
        "ops": args.ops,
        "seed": args.seed,
        "repeat": args.repeat,
        "python_seconds": python_time,
        "rust_seconds": rust_time,
        "python_ops_per_sec": args.ops / python_time,
        "rust_ops_per_sec": args.ops / rust_time,
        "speedup": speedup,
    }

    print(f"{'implementation':<12} {'time (s)':>10} {'ops/sec':>14}")
    print(f"{'python':<12} {python_time:>10.4f} {args.ops / python_time:>14,.0f}")
    print(f"{'rust':<12} {rust_time:>10.4f} {args.ops / rust_time:>14,.0f}")
    print(
        f"\nrust is {speedup:.2f}x faster than python "
        f"(best of {args.repeat} runs, {args.ops} ops, seed {args.seed})"
    )

    if args.json:
        args.json.write_text(json.dumps(result, indent=2) + "\n")

    exit_code = 0

    if speedup < args.min_speedup:
        print(
            f"FAIL: speedup {speedup:.2f}x is below --min-speedup {args.min_speedup:.2f}x",
            file=sys.stderr,
        )
        exit_code = 1

    if args.check_baseline:
        if not BASELINE_PATH.exists():
            print(
                f"error: --check-baseline given but {BASELINE_PATH} does not exist "
                "(run with --update-baseline first)",
                file=sys.stderr,
            )
            exit_code = 1
        else:
            baseline = json.loads(BASELINE_PATH.read_text())
            baseline_speedup = baseline["speedup"]
            if baseline.get("dataset", "snapshot") != args.dataset:
                print(
                    f"warning: baseline was recorded with --dataset "
                    f"{baseline.get('dataset', 'snapshot')!r}, comparing against "
                    f"--dataset {args.dataset!r} now -- numbers may not be comparable",
                    file=sys.stderr,
                )
            regression_floor = baseline_speedup * 0.9
            if speedup < regression_floor:
                print(
                    f"FAIL: speedup {speedup:.2f}x regressed more than 10% below "
                    f"baseline {baseline_speedup:.2f}x (floor {regression_floor:.2f}x)",
                    file=sys.stderr,
                )
                exit_code = 1

    if args.update_baseline:
        BASELINE_PATH.write_text(json.dumps(result, indent=2) + "\n")
        print(f"baseline updated: {BASELINE_PATH}")

    return exit_code


if __name__ == "__main__":
    sys.exit(main())
