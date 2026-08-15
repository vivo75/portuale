"""Black-box contract suite for the versions-comparison pilot (see
PORTING/PROMPT.md). Drives the Python harness (a thin wrapper around the
real `portage.versions`) and the Rust harness identically via subprocess,
and asserts their outputs are byte-for-byte the same. Neither harness's
internals are imported directly -- this is deliberately implementation-
agnostic, per the "black-box via CLI/API" decision in PROMPT.md.

Test vectors are mirrored from lib/portage/tests/versions/test_vercmp.py so
the pilot is graded against the same cases the Python implementation is
already known to satisfy.
"""

import subprocess

import pytest

# Mirrors testVerCmpGreater in lib/portage/tests/versions/test_vercmp.py:
# ver1 > ver2 for every pair below.
VERCMP_GREATER_CASES = [
    ("6.0", "5.0"),
    ("5.0", "5"),
    ("1.0-r1", "1.0-r0"),
    ("1.0-r1", "1.0"),
    ("999999999999999999999999999999", "999999999999999999999999999998"),
    ("1.0.0", "1.0"),
    ("1.0.0", "1.0b"),
    ("1b", "1"),
    ("1b_p1", "1_p1"),
    ("1.1b", "1.1"),
    ("12.2.5", "12.2b"),
]

# Mirrors testVerCmpEqual.
VERCMP_EQUAL_CASES = [
    ("4.0", "4.0"),
    ("1.0", "1.0"),
    ("1.0-r0", "1.0"),
    ("1.0", "1.0-r0"),
    ("1.0-r0", "1.0-r0"),
    ("1.0-r1", "1.0-r1"),
]

VERVERIFY_VALID_CASES = [
    "1.0",
    "1.0-r1",
    "1.0_pre2",
    "1.0_alpha1",
    "1",
    "1.2.3.4.5",
]

VERVERIFY_INVALID_CASES = [
    "abc",
    "1.0-r",
    "1.0_bogus",
    "-1.0",
]


def _run(cmd: list[str], *args: str) -> str:
    result = subprocess.run(
        [*cmd, *args], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


def _all_vercmp_pairs():
    for v1, v2 in VERCMP_GREATER_CASES:
        yield v1, v2
        yield v2, v1  # reversed direction must give the negated result
    for v1, v2 in VERCMP_EQUAL_CASES:
        yield v1, v2


@pytest.mark.parametrize("ver1,ver2", list(_all_vercmp_pairs()))
def test_vercmp_matches_between_implementations(
    ver1, ver2, versions_harness_python, versions_harness_rust
):
    python_result = _run(versions_harness_python, "vercmp", ver1, ver2)
    rust_result = _run([str(versions_harness_rust)], "vercmp", ver1, ver2)
    assert rust_result == python_result


@pytest.mark.parametrize("ver", VERVERIFY_VALID_CASES)
def test_ververify_valid_matches_between_implementations(
    ver, versions_harness_python, versions_harness_rust
):
    python_result = _run(versions_harness_python, "ververify", ver)
    rust_result = _run([str(versions_harness_rust)], "ververify", ver)
    assert python_result == "True"
    assert rust_result == python_result


@pytest.mark.parametrize("ver", VERVERIFY_INVALID_CASES)
def test_ververify_invalid_matches_between_implementations(
    ver, versions_harness_python, versions_harness_rust
):
    python_result = _run(versions_harness_python, "ververify", ver)
    rust_result = _run([str(versions_harness_rust)], "ververify", ver)
    assert python_result == "False"
    assert rust_result == python_result


@pytest.mark.parametrize("bad_ver", VERVERIFY_INVALID_CASES)
def test_vercmp_of_invalid_version_is_none_in_both(
    bad_ver, versions_harness_python, versions_harness_rust
):
    python_result = _run(versions_harness_python, "vercmp", bad_ver, "1.0")
    rust_result = _run([str(versions_harness_rust)], "vercmp", bad_ver, "1.0")
    assert python_result == "None"
    assert rust_result == python_result


def test_batch_mode_output_matches(versions_harness_python, versions_harness_rust):
    """Exercises benchmark mode: many operations fed to a single process
    invocation via stdin, to avoid fork/exec overhead dominating a
    performance comparison (see PROMPT.md, harness architecture)."""
    lines = [f"vercmp {v1} {v2}" for v1, v2 in _all_vercmp_pairs()]
    lines += [f"ververify {ver}" for ver in VERVERIFY_VALID_CASES]
    lines += [f"ververify {ver}" for ver in VERVERIFY_INVALID_CASES]
    stdin_data = "\n".join(lines) + "\n"

    python_out = subprocess.run(
        [*versions_harness_python, "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    rust_out = subprocess.run(
        [str(versions_harness_rust), "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    assert rust_out == python_out
    assert len(rust_out.splitlines()) == len(lines)
