"""CI performance-regression gate (see docs/agent-context.md: "benchmark suite must show
Rust ahead of Python and must not regress over time"). Opt-in via
PORTUALE_RUN_BENCHMARK=1 so the default `pytest tests` run (the
correctness contract suite) stays fast; CI should set the env var to
actually enforce the gate.
"""

import os
import subprocess
import sys
from pathlib import Path

import pytest

BENCH_SCRIPT = Path(__file__).resolve().parents[1] / "bench" / "run_benchmark.py"

pytestmark = pytest.mark.skipif(
    os.environ.get("PORTUALE_RUN_BENCHMARK") != "1",
    reason="opt-in: set PORTUALE_RUN_BENCHMARK=1 to run the timing regression gate",
)


def test_rust_versions_harness_is_faster_than_python():
    result = subprocess.run(
        [
            sys.executable,
            str(BENCH_SCRIPT),
            "--ops",
            "20000",
            "--repeat",
            "3",
            "--min-speedup",
            "1.0",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    print(result.stdout)
    assert result.returncode == 0, result.stderr
