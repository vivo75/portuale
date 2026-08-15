import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
RUST_DIR = REPO_ROOT / "PORTING" / "rust"
PYTHON_HARNESS = REPO_ROOT / "PORTING" / "python" / "versions_harness.py"


def _cargo_build(package: str) -> Path:
    subprocess.run(
        ["cargo", "build", "--release", "--package", package],
        cwd=RUST_DIR,
        check=True,
    )
    return RUST_DIR / "target" / "release" / package


@pytest.fixture(scope="session")
def versions_harness_rust() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")
    return _cargo_build("versions-harness")


@pytest.fixture(scope="session")
def versions_harness_python() -> list[str]:
    return [sys.executable, str(PYTHON_HARNESS)]


@pytest.fixture(scope="session")
def multicall_binary() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")
    return _cargo_build("multicall")
