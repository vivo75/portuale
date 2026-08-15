"""Black-box test for the emerge/ebuild multicall skeleton (see
PORTING/PROMPT.md, "emerge/ebuild binary shape"). Tests the real compiled
CLI via symlinks in a PATH, exactly as it would be invoked in practice --
not by importing anything from the binary.

Also covers `ebuild`'s CLI-surface-recognition follow-up (see
PORTING/rust/multicall/src/ebuild.rs/ebuild_options.rs): real ebuild
options (bin/ebuild's own argparse setup) and real ebuild commands
(doebuild()'s own validcommands list) are recognized and accepted as a
still-a-no-op dry-run stub, while genuinely invalid input (an
unrecognized option, a bad filename, an unrecognized command, or
missing required args) is now rejected with a specific message and a
real exit code -- unlike `emerge --pretend`, `ebuild` has no Python
reference implementation to contract-test against, since it has no
real behavior to keep in sync between two implementations; this file
is the only test surface for it.
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


def test_ebuild_accepts_multiple_real_commands(ebuild_binary):
    """Real ebuild invocations commonly chain several phases in one call
    (e.g. "clean compile install") -- all still just recognized, still a
    no-op stub."""
    result = subprocess.run(
        [str(ebuild_binary), "foo-1.0.ebuild", "clean", "compile", "install"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild (pilot stub)" in result.stdout


def test_ebuild_accepts_a_real_value_option_without_misreading_its_value(ebuild_binary):
    """--color is a real ebuild option that takes a value (see
    bin/ebuild's own argparse setup) -- its value ("y") must not be
    misinterpreted as the ebuild file or an extra command."""
    result = subprocess.run(
        [str(ebuild_binary), "--color", "y", "foo-1.0.ebuild", "merge"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild (pilot stub)" in result.stdout
    assert 'ebuild file: "foo-1.0.ebuild"' in result.stdout
    assert 'commands: ["merge"]' in result.stdout


def test_ebuild_accepts_the_inline_equals_form_of_a_value_option(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "--color=y", "foo-1.0.ebuild", "merge"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "ebuild (pilot stub)" in result.stdout


def test_ebuild_rejects_an_unrecognized_option(ebuild_binary):
    """Distinct from a real-but-unimplemented option: a token that isn't
    in bin/ebuild's own option surface at all is rejected immediately
    and specifically, unlike real bin/ebuild's own argparse (which uses
    parse_known_args and would silently swallow it into the positional
    args instead -- see ebuild.rs's doc comment for why this pilot
    deviates)."""
    result = subprocess.run(
        [str(ebuild_binary), "--not-a-real-option", "foo-1.0.ebuild", "merge"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert result.stderr.strip() == 'ebuild: unrecognized option "--not-a-real-option"'


def test_ebuild_rejects_a_filename_not_ending_in_dot_ebuild(ebuild_binary):
    result = subprocess.run(
        [str(ebuild_binary), "foo-1.0.tar.gz", "merge"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert result.stderr.strip() == 'ebuild: "foo-1.0.tar.gz": does not end with ".ebuild"'


def test_ebuild_rejects_an_unrecognized_command(ebuild_binary):
    """"not-a-real-phase" isn't in doebuild()'s own validcommands list,
    so it must be rejected the same way real doebuild() itself would
    (exit 1), not silently accepted as if it were a real, merely
    unimplemented phase."""
    result = subprocess.run(
        [str(ebuild_binary), "foo-1.0.ebuild", "not-a-real-phase"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert (
        result.stderr.strip()
        == 'ebuild: "not-a-real-phase" is not one of the valid ebuild commands'
    )


def test_ebuild_rejects_missing_required_args(ebuild_binary):
    """Mirrors real bin/ebuild's own argparse parser.error() exit code
    (2) for "missing required args", distinct from the exit-1 "invalid
    input" cases above."""
    no_args = subprocess.run(
        [str(ebuild_binary)], capture_output=True, text=True, check=False
    )
    assert no_args.returncode == 2
    assert no_args.stderr.strip() == "ebuild: missing required args"

    no_command = subprocess.run(
        [str(ebuild_binary), "foo-1.0.ebuild"], capture_output=True, text=True, check=False
    )
    assert no_command.returncode == 2
    assert no_command.stderr.strip() == "ebuild: missing required args"
