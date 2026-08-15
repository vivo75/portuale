"""Black-box contract suite for the `emerge --pretend` pilot slice (see
PORTING/PROMPT.md and PORTING/rust/portage-repo/src/lib.rs for the full
scope writeup, including the dependency-recursion follow-up in
resolve_pretend_graph and the profile/make.conf -> real USE/ACCEPT_KEYWORDS
follow-up in portage-profile). Drives the real compiled `emerge` binary
(multicall, dispatched via a real symlink -- not a neutral harness, since
emerge is an actual product surface per PROMPT.md's testing decision) and
the Python reference implementation identically, against the synthetic
fixture tree at PORTING/fixtures (whose repos.conf/make.profile/make.conf
now drive real config resolution, not hardcoded values), and asserts their
stdout, stderr, and exit codes all match exactly.
"""

import subprocess

import pytest

# (description, args, expected_exit_code) -- exit codes: 0 success,
# 1 resolution/parse error, 2 CLI-usage error (mirrors both sides' shared
# convention, not real emerge's own exit codes).
CASES = [
    ("new install", ["--pretend", "dev-libs/newpkg"], 0),
    ("already installed", ["--pretend", "dev-libs/samepkg"], 0),
    ("upgrade available", ["--pretend", "dev-libs/upgradepkg"], 0),
    ("only ~keyword, not visible", ["--pretend", "dev-libs/maskedpkg"], 1),
    ("package does not exist", ["--pretend", "dev-libs/does-not-exist"], 1),
    ("sibling-prefix package: new", ["--pretend", "dev-libs/foo"], 0),
    ("sibling-prefix package: installed", ["--pretend", "dev-libs/foo-bar"], 0),
    ("versioned atom: out of v1 scope", ["--pretend", ">=dev-libs/foo-1.0"], 2),
    ("slotted atom: out of v1 scope", ["--pretend", "dev-libs/foo:0"], 2),
    ("syntactically invalid atom", ["--pretend", "not an atom!"], 1),
    ("no atom given", ["--pretend"], 2),
    ("more than one atom", ["--pretend", "dev-libs/foo", "dev-libs/bar"], 2),
    ("missing --pretend", ["dev-libs/newpkg"], 2),
    ("unrecognized option", ["--deep", "dev-libs/newpkg"], 2),
    ("recursion: basic dependency chain", ["--pretend", "dev-libs/withdeps"], 0),
    ("recursion: diamond dependency dedup", ["--pretend", "dev-libs/diamond"], 0),
    ("recursion: dependency cycle terminates", ["--pretend", "dev-libs/cycle-a"], 0),
    ("recursion: any-of group resolves every alternative", ["--pretend", "dev-libs/anyof"], 0),
    ("recursion: unresolvable dep doesn't fail the graph", ["--pretend", "dev-libs/missingdep"], 0),
    ("recursion: dedup across DEPEND and RDEPEND", ["--pretend", "dev-libs/dualdep"], 0),
    ("profile config: real USE flag gates a dependency", ["--pretend", "dev-libs/useflagpkg"], 0),
]


def _run(cmd: list[str], args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        [*cmd, *args], capture_output=True, text=True, env=env, check=False
    )


@pytest.mark.parametrize("description,args,expected_exit", CASES)
def test_pretend_matches_between_implementations(
    description, args, expected_exit, emerge_binary, emerge_pretend_python, fixture_env
):
    rust_result = _run([str(emerge_binary)], args, fixture_env)
    python_result = _run(emerge_pretend_python, args, fixture_env)

    assert rust_result.returncode == expected_exit, (
        f"{description}: rust exit {rust_result.returncode} != expected {expected_exit}\n"
        f"stdout={rust_result.stdout!r} stderr={rust_result.stderr!r}"
    )
    assert python_result.returncode == expected_exit, (
        f"{description}: python exit {python_result.returncode} != expected {expected_exit}\n"
        f"stdout={python_result.stdout!r} stderr={python_result.stderr!r}"
    )
    assert rust_result.stdout == python_result.stdout, description
    assert rust_result.stderr == python_result.stderr, description


def test_missing_repos_conf_matches_between_implementations(
    emerge_binary, emerge_pretend_python
):
    """A config root with no repos.conf at all is a distinct error path
    from "package not found" -- exercised separately since it doesn't use
    the shared fixture_env."""
    env = {"PORTAGE_CONFIGROOT": "/nonexistent-config-root-for-this-test", "ROOT": "/"}
    args = ["--pretend", "dev-libs/newpkg"]

    rust_result = _run([str(emerge_binary)], args, env)
    python_result = _run(emerge_pretend_python, args, env)

    assert rust_result.returncode == 1
    assert python_result.returncode == 1
    assert rust_result.stdout == python_result.stdout
    assert rust_result.stderr == python_result.stderr


def test_diamond_dependency_is_deduped_and_ordered(emerge_binary, fixture_env):
    """Pins the exact recursion output for the diamond fixture (diamond ->
    shared-a, shared-b -> common), not just parity with Python: "common"
    must appear exactly once despite being reachable two ways, in
    discovery order."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/diamond"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/diamond-1.0",
        "[ebuild  N] dev-libs/shared-a-1.0",
        "[ebuild  N] dev-libs/shared-b-1.0",
        "[ebuild  N] dev-libs/common-1.0",
    ]


def test_any_of_group_resolves_every_alternative(emerge_binary, fixture_env):
    """Pins the documented v1 any-of simplification: both alternatives of
    `|| ( dev-libs/newpkg dev-libs/samepkg )` are considered, but only the
    one that would newly merge (newpkg) is printed -- samepkg is already
    installed and stays silent, same as any other already-satisfied dep."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/anyof"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/anyof-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_unresolvable_dependency_is_reported_not_silently_dropped(
    emerge_binary, fixture_env
):
    """The top-level package still resolves and the graph doesn't fail,
    but the unresolvable dependency is reported on stderr, not silently
    omitted."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/missingdep"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/missingdep-1.0"]
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/doesnotexist-anywhere"'
    )


def test_real_use_flags_from_profile_gate_a_dependency(emerge_binary, fixture_env):
    """Pins the profile/make.conf -> real USE follow-up end to end: the
    fixture's profile chain + make.conf (see PORTING/fixtures/repo/profiles
    and portage-profile's own contract test) resolves "foo" enabled and
    "missingflag" disabled, so useflagpkg's `foo? ( dev-libs/newpkg )`
    dependency must be pulled in and its
    `missingflag? ( dev-libs/hiddendep )` must not be -- proving real
    profile-derived USE, not a hardcoded empty set, reaches use_reduce."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/useflagpkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]
    assert "hiddendep" not in result.stdout
