"""musl static-build smoke test, wrapped as a CI gate (see docs/agent-context.md: "Rust
CI also gates on a musl static build smoke-tested inside a minimal
(scratch/busybox-level) container"). Opt-in via PORTUALE_RUN_MUSL_SMOKE=1,
same pattern as test_benchmark_gate.py -- it needs podman or docker and
takes tens of seconds, so it stays out of the default fast contract-suite
run; CI should set the env var to actually enforce the gate.
"""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

SMOKE_SCRIPT = Path(__file__).resolve().parents[1] / "musl" / "smoke_test.sh"

pytestmark = pytest.mark.skipif(
    os.environ.get("PORTUALE_RUN_MUSL_SMOKE") != "1",
    reason="opt-in: set PORTUALE_RUN_MUSL_SMOKE=1 to run the musl container smoke test",
)


def test_musl_static_binaries_run_in_scratch_container():
    if shutil.which("podman") is None and shutil.which("docker") is None:
        pytest.skip("neither podman nor docker available")
    result = subprocess.run(
        ["bash", str(SMOKE_SCRIPT)], capture_output=True, text=True, check=False
    )
    print(result.stdout)
    assert result.returncode == 0, result.stderr
