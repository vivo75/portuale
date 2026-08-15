"""Black-box test for the emerge/ebuild multicall skeleton (see
PORTING/PROMPT.md, "emerge/ebuild binary shape"). Tests the real compiled
CLI via symlinks in a PATH, exactly as it would be invoked in practice --
not by importing anything from the binary.
"""

import os
import subprocess
from pathlib import Path

FIXTURES_ROOT = str(Path(__file__).resolve().parents[1] / "fixtures")


def _fixture_env():
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = FIXTURES_ROOT
    env["ROOT"] = FIXTURES_ROOT
    return env


def test_dispatch_via_symlink_emerge(multicall_binary, tmp_path):
    """Exercises real `emerge --pretend` resolution (see
    test_emerge_pretend_contract.py for full coverage of the outcomes);
    here the point is that the symlink-dispatched binary reaches it at
    all."""
    emerge_link = tmp_path / "emerge"
    emerge_link.symlink_to(multicall_binary)
    result = subprocess.run(
        [str(emerge_link), "--pretend", "dev-libs/newpkg"],
        capture_output=True,
        text=True,
        check=True,
        env=_fixture_env(),
    )
    assert result.stdout.strip() == "[ebuild  N] dev-libs/newpkg-1.0"


def test_dispatch_via_symlink_ebuild(multicall_binary, tmp_path):
    ebuild_link = tmp_path / "ebuild"
    ebuild_link.symlink_to(multicall_binary)
    result = subprocess.run(
        [str(ebuild_link), "foo-1.0.ebuild", "merge"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild (pilot stub)" in result.stdout


def test_dispatch_via_path_lookup_by_bare_name(multicall_binary, tmp_path):
    """The real-world usage pattern: PATH contains a directory of applet
    symlinks, and the applet is invoked by bare name. Proves the binary is
    a drop-in for tooling that calls `emerge`/`ebuild` directly."""
    (tmp_path / "emerge").symlink_to(multicall_binary)
    (tmp_path / "ebuild").symlink_to(multicall_binary)
    env = _fixture_env()
    env["PATH"] = f"{tmp_path}{os.pathsep}{env.get('PATH', '')}"

    result = subprocess.run(
        ["emerge", "--pretend", "dev-libs/newpkg"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert result.stdout.strip() == "[ebuild  N] dev-libs/newpkg-1.0"


def test_explicit_arg_fallback_dispatch(multicall_binary):
    """Invoked under its own name (no symlink), the binary still dispatches
    via an explicit first argument, busybox-style, so it's testable and
    usable without setting up symlinks."""
    result = subprocess.run(
        [str(multicall_binary), "ebuild", "foo-1.0.ebuild", "merge"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild (pilot stub)" in result.stdout


def test_unrecognized_applet_fails_clearly(multicall_binary):
    result = subprocess.run(
        [str(multicall_binary)], capture_output=True, text=True, check=False
    )
    assert result.returncode != 0
    assert "unrecognized applet" in result.stderr
