"""Black-box test for the emerge/ebuild multicall skeleton (see
PORTING/PROMPT.md, "emerge/ebuild binary shape"). Tests the real compiled
CLI via symlinks in a PATH, exactly as it would be invoked in practice --
not by importing anything from the binary.
"""

import os
import subprocess


def test_dispatch_via_symlink_emerge(multicall_binary, tmp_path):
    emerge_link = tmp_path / "emerge"
    emerge_link.symlink_to(multicall_binary)
    result = subprocess.run(
        [str(emerge_link), "--pretend", "sys-apps/foo"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "emerge (pilot stub)" in result.stdout
    assert "--pretend" in result.stdout


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
    env = dict(os.environ)
    env["PATH"] = f"{tmp_path}{os.pathsep}{env.get('PATH', '')}"

    result = subprocess.run(
        ["emerge", "--pretend", "sys-apps/foo"],
        capture_output=True,
        text=True,
        check=True,
        env=env,
    )
    assert "emerge (pilot stub)" in result.stdout


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
