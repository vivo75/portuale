"""Black-box contract suite for the atom-matching pilot (see
PORTING/PROMPT.md and PORTING/rust/atom-harness/src/atom.rs). Drives the
Python harness (a thin wrapper around the real portage.dep.Atom /
match_from_list) and the Rust harness identically via subprocess, and
asserts their outputs are byte-for-byte the same. Neither harness's
internals are imported directly.
"""

import subprocess

import pytest

PARSE_VALID_ATOMS = [
    "dev-libs/foo",
    "sys-apps/portage",
    "=dev-libs/foo-1.2.3",
    "=dev-libs/foo-1.2.3-r1",
    ">=dev-libs/foo-1.2.3-r1",
    ">dev-libs/foo-1.0",
    "<=dev-libs/foo-2.0",
    "<dev-libs/foo-2.0",
    "~dev-libs/foo-1.2.3",
    "!dev-libs/foo",
    "!!dev-libs/foo",
    "!>=dev-libs/foo-1.0",
    "dev-libs/foo:2",
    "dev-libs/foo:2/2.1",
    ">=dev-libs/foo-1.0:0",
    "dev-libs/foo-bar",  # package name itself containing a hyphen
    "dev-libs/foo-bar:1.0-2",  # hyphenated package name plus a dotted slot
    "=dev-libs/foo-1.0b",  # letter suffix
    "=dev-libs/foo-1.0_pre2",  # underscore suffix
    "=dev-libs/foo-1.0_alpha3-r4",
]

# Not valid at all, or valid PMS atoms that use a feature outside the v1
# grammar this pilot ports (see atom.rs's module doc comment).
PARSE_INVALID_ATOMS = [
    "not-an-atom",
    "dev-libs/",
    "/foo",
    "dev-libs/foo-1.2.3",  # bare version with no operator: not a valid atom
    "dev-libs/foo[bar]",  # USE dep: out of v1 scope
    "dev-libs/foo[-bar]",
    "dev-libs/foo::gentoo",  # repo constraint: out of v1 scope
    "=dev-libs/foo-1.2.3*",  # glob operator: out of v1 scope
    "*/foo-1",  # extended/wildcard syntax: out of v1 scope
    "dev-libs/foo:0=",  # slot operator: out of v1 scope
    "dev-libs/foo-1.0@2",  # build id: out of v1 scope
    "dev-libs/foo-bar-2",  # bare atom whose package name would end in
    "dev-libs/foo-2",  # something version-like: ambiguous under PMS, rejected
]


def _run(cmd: list[str], *args: str) -> str:
    result = subprocess.run(
        [*cmd, *args], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


@pytest.mark.parametrize("atom", PARSE_VALID_ATOMS)
def test_parse_valid_atom_matches_between_implementations(
    atom, atom_harness_python, atom_harness_rust
):
    python_result = _run(atom_harness_python, "parse", atom)
    rust_result = _run([str(atom_harness_rust)], "parse", atom)
    assert rust_result == python_result
    assert python_result != "INVALID"


@pytest.mark.parametrize("atom", PARSE_INVALID_ATOMS)
def test_parse_invalid_atom_matches_between_implementations(
    atom, atom_harness_python, atom_harness_rust
):
    python_result = _run(atom_harness_python, "parse", atom)
    rust_result = _run([str(atom_harness_rust)], "parse", atom)
    assert python_result == "INVALID"
    assert rust_result == python_result


CANDIDATES = [
    "dev-libs/foo-1.0",
    "dev-libs/foo-1.2.3",
    "dev-libs/foo-1.2.3-r1",
    "dev-libs/foo-1.2.3-r1:2/2.1",
    "dev-libs/foo-2.0:3",
    "dev-libs/foo-2.0:3/1.0",
    "dev-libs/foo-0.9:2",
    "dev-libs/bar-1.0",
    "dev-libs/foobar-1.0",  # same prefix, different package: must not match
]

MATCH_CASES = [
    # (atom, candidates)
    ("dev-libs/foo", CANDIDATES),
    ("=dev-libs/foo-1.2.3", CANDIDATES),
    ("=dev-libs/foo-1.2.3-r1", CANDIDATES),
    ("=dev-libs/foo-1.2.3-r0", CANDIDATES),  # r0 must equal the un-revisioned "1.2.3"
    ("~dev-libs/foo-1.2.3", CANDIDATES),
    (">=dev-libs/foo-1.2.3", CANDIDATES),
    (">dev-libs/foo-1.2.3", CANDIDATES),
    ("<=dev-libs/foo-1.2.3", CANDIDATES),
    ("<dev-libs/foo-1.2.3", CANDIDATES),
    ("dev-libs/foo:2", CANDIDATES),
    ("dev-libs/foo:2/2.1", CANDIDATES),
    ("dev-libs/foo:3/1.0", CANDIDATES),
    ("dev-libs/foo:9", CANDIDATES),  # slot nothing matches
    ("!dev-libs/foo", CANDIDATES),  # blocker: matches same as non-blocker
    (">=dev-libs/nonexistent-1.0", CANDIDATES),  # no matches at all
    # a candidate with no slot info must still pass a slotted atom, per the
    # real match_from_list's "unknown slot isn't filtered" behavior.
    ("dev-libs/foo:2", ["dev-libs/foo-1.0", "dev-libs/foo-1.0:2"]),
]


@pytest.mark.parametrize("atom,candidates", MATCH_CASES)
def test_match_matches_between_implementations(
    atom, candidates, atom_harness_python, atom_harness_rust
):
    python_result = _run(atom_harness_python, "match", atom, *candidates)
    rust_result = _run([str(atom_harness_rust)], "match", atom, *candidates)
    assert rust_result == python_result


def test_match_of_invalid_atom_is_invalid_in_both(
    atom_harness_python, atom_harness_rust
):
    python_result = _run(
        atom_harness_python, "match", "dev-libs/foo[bar]", *CANDIDATES
    )
    rust_result = _run(
        [str(atom_harness_rust)], "match", "dev-libs/foo[bar]", *CANDIDATES
    )
    assert python_result == "INVALID"
    assert rust_result == python_result


def test_batch_mode_output_matches(atom_harness_python, atom_harness_rust):
    """Exercises benchmark-shaped batch mode: many operations fed to a
    single process invocation via stdin."""
    lines = [f"parse {a}" for a in PARSE_VALID_ATOMS + PARSE_INVALID_ATOMS]
    lines += [f"match {atom} {' '.join(candidates)}" for atom, candidates in MATCH_CASES]
    stdin_data = "\n".join(lines) + "\n"

    python_out = subprocess.run(
        [*atom_harness_python, "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    rust_out = subprocess.run(
        [str(atom_harness_rust), "batch"],
        input=stdin_data,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    assert rust_out == python_out
    assert len(rust_out.splitlines()) == len(lines)
