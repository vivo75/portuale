import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
RUST_DIR = REPO_ROOT / "PORTING" / "rust"
VERSIONS_PYTHON_HARNESS = REPO_ROOT / "PORTING" / "python" / "versions_harness.py"
ATOM_PYTHON_HARNESS = REPO_ROOT / "PORTING" / "python" / "atom_harness.py"
USE_REDUCE_PYTHON_HARNESS = REPO_ROOT / "PORTING" / "python" / "use_reduce_harness.py"
REQUIRED_USE_PYTHON_HARNESS = REPO_ROOT / "PORTING" / "python" / "required_use_harness.py"
EMERGE_PRETEND_PYTHON_REFERENCE = (
    REPO_ROOT / "PORTING" / "python" / "emerge_pretend_reference.py"
)
FIXTURES_ROOT = REPO_ROOT / "PORTING" / "fixtures"


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
    return [sys.executable, str(VERSIONS_PYTHON_HARNESS)]


@pytest.fixture(scope="session")
def atom_harness_rust() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")
    return _cargo_build("atom-harness")


@pytest.fixture(scope="session")
def atom_harness_python() -> list[str]:
    return [sys.executable, str(ATOM_PYTHON_HARNESS)]


@pytest.fixture(scope="session")
def use_reduce_harness_rust() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")
    return _cargo_build("use-reduce-harness")


@pytest.fixture(scope="session")
def use_reduce_harness_python() -> list[str]:
    return [sys.executable, str(USE_REDUCE_PYTHON_HARNESS)]


@pytest.fixture(scope="session")
def required_use_harness_rust() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")
    return _cargo_build("required-use-harness")


@pytest.fixture(scope="session")
def required_use_harness_python() -> list[str]:
    return [sys.executable, str(REQUIRED_USE_PYTHON_HARNESS)]


@pytest.fixture(scope="session")
def portuale_binary() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo not available")
    return _cargo_build("portuale")


@pytest.fixture(scope="session")
def emerge_binary(portuale_binary: Path, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A real `emerge` symlink to the portuale binary, so tests exercise
    the same argv[0]-dispatch path a real installation would use."""
    link_dir = tmp_path_factory.mktemp("emerge-symlink")
    link = link_dir / "emerge"
    link.symlink_to(portuale_binary)
    return link


@pytest.fixture(scope="session")
def ebuild_binary(portuale_binary: Path, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A real `ebuild` symlink to the portuale binary, so tests exercise
    the same argv[0]-dispatch path a real installation would use."""
    link_dir = tmp_path_factory.mktemp("ebuild-symlink")
    link = link_dir / "ebuild"
    link.symlink_to(portuale_binary)
    return link


@pytest.fixture(scope="session")
def emerge_pretend_python() -> list[str]:
    return [sys.executable, str(EMERGE_PRETEND_PYTHON_REFERENCE)]


@pytest.fixture
def fixtures_root() -> Path:
    """PORTING/fixtures/, for tests that copy a committed fixture file
    (e.g. a real `.tbz2`/`.gpkg.tar`) into an ad-hoc tree."""
    return FIXTURES_ROOT


@pytest.fixture
def fixture_env() -> dict[str, str]:
    """PORTAGE_CONFIGROOT/ROOT pointed at PORTING/fixtures, the synthetic
    repo+vdb tree the emerge --pretend pilot slice is tested against.
    DISTDIR points at the committed PORTING/fixtures/distfiles/ so the
    `f`/`F` fetch-restrict bracket column has a deterministic on-disk
    state to check against."""
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = str(FIXTURES_ROOT)
    env["ROOT"] = str(FIXTURES_ROOT)
    env["DISTDIR"] = str(FIXTURES_ROOT / "distfiles")
    return env
