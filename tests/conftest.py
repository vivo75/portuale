import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
RUST_DIR = REPO_ROOT / "rust"
VERSIONS_PYTHON_HARNESS = REPO_ROOT / "python" / "versions_harness.py"
ATOM_PYTHON_HARNESS = REPO_ROOT / "python" / "atom_harness.py"
USE_REDUCE_PYTHON_HARNESS = REPO_ROOT / "python" / "use_reduce_harness.py"
REQUIRED_USE_PYTHON_HARNESS = REPO_ROOT / "python" / "required_use_harness.py"
EMERGE_PRETEND_PYTHON_REFERENCE = REPO_ROOT / "python" / "emerge_pretend_reference.py"
FIXTURES_ROOT = REPO_ROOT / "fixtures"

# Config variables portuale now honours from the process environment
# (real `config.regenerate()`'s `env` USE_ORDER layer -- see
# portage-profile's `ENV_INCREMENTAL_VARS`/`ENV_SCALAR_VARS`). A test
# runner's own environment must not leak into the fixture config, so
# these are stripped process-wide for the whole test session; a test
# that specifically exercises an env override sets the var explicitly.
_ENV_CONFIG_VARS = (
    "USE", "ACCEPT_KEYWORDS", "USE_EXPAND", "USE_EXPAND_UNPREFIXED",
    "USE_EXPAND_IMPLICIT", "USE_EXPAND_HIDDEN", "IUSE_IMPLICIT",
    "ACCEPT_LICENSE", "ACCEPT_PROPERTIES", "ACCEPT_RESTRICT",
    "PKGDIR", "PORTAGE_LOGDIR", "PORTAGE_BINHOST", "PORTAGE_NICENESS",
    "PORTAGE_IONICE_COMMAND", "PORTAGE_SCHEDULING_POLICY",
    "PORTAGE_SCHEDULING_PRIORITY", "PORTAGE_ELOG_SYSTEM", "PORTAGE_ELOG_CLASSES",
    "PORTAGE_ELOG_MAILURI", "FEATURES", "CHOST", "CBUILD", "CTARGET",
    "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "FFLAGS", "FCFLAGS",
    "MAKEOPTS", "EMERGE_DEFAULT_OPTS", "PORTAGE_RSYNC_EXTRA_OPTS",
    "GENTOO_MIRRORS", "VIDEO_CARDS", "PYTHON_TARGETS", "PYTHON_SINGLE_TARGET",
    "LINGUAS", "L10N", "CPU_FLAGS_X86", "ELIBC", "KERNEL", "USERLAND", "ABI_X86",
)


@pytest.fixture(scope="session", autouse=True)
def _isolate_config_env():
    """Strip any inherited make.conf-style config vars for the session."""
    for name in _ENV_CONFIG_VARS:
        os.environ.pop(name, None)
    yield


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


@pytest.fixture(autouse=True)
def _no_clean_delay(monkeypatch: pytest.MonkeyPatch) -> None:
    """Real `emerge -C`/`--depclean`/`--prune` run a `CLEAN_DELAY`-second
    countdown (default 5) before removing anything. Pin it to 0 for the
    whole suite so removal tests don't each sleep -- real portage's own
    test infra does the same."""
    monkeypatch.setenv("CLEAN_DELAY", "0")


@pytest.fixture
def fixtures_root() -> Path:
    """fixtures/, for tests that copy a committed fixture file
    (e.g. a real `.tbz2`/`.gpkg.tar`) into an ad-hoc tree."""
    return FIXTURES_ROOT


@pytest.fixture
def fixture_env() -> dict[str, str]:
    """PORTAGE_CONFIGROOT/ROOT pointed at fixtures, the synthetic
    repo+vdb tree the emerge --pretend pilot slice is tested against.
    DISTDIR points at the committed fixtures/distfiles/ so the
    `f`/`F` fetch-restrict bracket column has a deterministic on-disk
    state to check against.

    PORTAGE_RUNNING_ROOT is pinned to the same fixture ROOT: the pilot
    now routes BDEPEND/IDEPEND against the running root whenever it
    differs from the target ROOT (real EAPI-7+ portage does this
    unconditionally -- see running_root_from_env's doc comment), so
    without this pin every fixture-ROOT test would consult the real
    host's /var/db/pkg and lose determinism. A test that specifically
    exercises a cross-root build overrides PORTAGE_RUNNING_ROOT itself."""
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = str(FIXTURES_ROOT)
    env["ROOT"] = str(FIXTURES_ROOT)
    env["PORTAGE_RUNNING_ROOT"] = str(FIXTURES_ROOT)
    env["DISTDIR"] = str(FIXTURES_ROOT / "distfiles")
    return env
