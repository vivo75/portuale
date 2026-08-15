"""Black-box contract suite for the `emerge --pretend` pilot slice (see
PORTING/PROMPT.md and PORTING/rust/portage-repo/src/lib.rs for the full
scope writeup, including the dependency-recursion follow-up in
resolve_pretend_graph, the profile/make.conf -> real USE/ACCEPT_KEYWORDS
follow-up in portage-profile, the package.mask/.unmask/.accept_keywords/
.use follow-up on top of that, the blocker-reporting follow-up on top of
that, and the overlays (multiple repos.conf repos) follow-up on top of
that). Drives the real compiled `emerge` binary (multicall, dispatched
via a real symlink -- not a neutral harness, since emerge is an actual
product surface per PROMPT.md's testing decision) and the Python
reference implementation identically, against the synthetic fixture tree
at PORTING/fixtures (whose repos.conf/make.profile/make.conf/
package.mask/package.unmask/package.accept_keywords/package.use now drive
real config resolution, not hardcoded values, and whose repos.conf now
defines a second, higher-priority overlay repo alongside the main one),
and asserts their stdout, stderr, and exit codes all match exactly.
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
    ("package.mask: hidden, no unmask", ["--pretend", "dev-libs/hardmaskedpkg"], 1),
    ("package.mask + package.unmask: masked then unmasked", ["--pretend", "dev-libs/maskedandunmaskedpkg"], 0),
    ("package.mask: -atom removal leaves candidate unaffected", ["--pretend", "dev-libs/samepkg"], 0),
    ("package.accept_keywords: wildcard extends visibility", ["--pretend", "dev-libs/wildcardkeywordpkg"], 0),
    ("package.accept_keywords: ** accepts no-keywords package", ["--pretend", "dev-libs/livekeywordpkg"], 0),
    ("package.use: wildcard entry enables a flag not on globally", ["--pretend", "dev-libs/packageuseenablepkg"], 0),
    ("package.use: entry disables a flag that is on globally", ["--pretend", "dev-libs/packageusedisablepkg"], 0),
    ("blocker: strong (!!) blocker matches an installed package", ["--pretend", "dev-libs/blockerpkg"], 0),
    ("blocker: weak (!) blocker matches another new package in the graph", ["--pretend", "dev-libs/graphblockerparent"], 0),
    ("overlay: package exists only in the overlay repo", ["--pretend", "dev-libs/overlayonlypkg"], 0),
    ("overlay: best version wins across repos", ["--pretend", "dev-libs/overlaynewerpkg"], 0),
    ("overlay: same-version tie broken toward higher priority", ["--pretend", "dev-libs/overlaytiepkg"], 0),
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


def test_package_mask_hides_with_no_matching_unmask(emerge_binary, fixture_env):
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/hardmaskedpkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for "dev-libs/hardmaskedpkg"'
    )


def test_package_unmask_cancels_a_matching_package_mask(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/maskedandunmaskedpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/maskedandunmaskedpkg-1.0"


def test_package_mask_minus_atom_removal_leaves_candidate_unaffected(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/etc/portage/package.mask masks dev-libs/samepkg and
    then immediately un-masks it again via "-dev-libs/samepkg" within the
    same file -- it must resolve completely normally (already installed),
    proving -atom removal actually took effect rather than the mask
    lingering."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/samepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "dev-libs/samepkg-1.0 is already installed; nothing to do"


def test_package_accept_keywords_wildcard_extends_visibility(emerge_binary, fixture_env):
    """dev-libs/wildcardkeywordpkg is only ~amd64 (not globally accepted),
    but PORTING/fixtures/etc/portage/package.accept_keywords has a
    "*/wildcardkeywordpkg ~amd64" entry that makes it visible."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/wildcardkeywordpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/wildcardkeywordpkg-1.0"


def test_package_accept_keywords_double_star_accepts_no_keywords_package(
    emerge_binary, fixture_env
):
    """dev-libs/livekeywordpkg has no KEYWORDS at all (like a live/9999
    ebuild), but a "**" package.accept_keywords entry accepts it
    unconditionally."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/livekeywordpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/livekeywordpkg-9999"


def test_unrelated_masked_by_keywords_package_is_still_hidden(emerge_binary, fixture_env):
    """Regression guard: the "*/wildcardkeywordpkg" package.accept_keywords
    entry is scoped to that package name only (not "dev-libs/*"), so it
    must not accidentally make dev-libs/maskedpkg visible too."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/maskedpkg"], fixture_env)
    assert result.returncode == 1


def test_package_use_wildcard_entry_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/etc/portage/package.use has a
    "*/packageuseenablepkg pkguseflag" entry: "pkguseflag" isn't enabled by
    the profile chain or make.conf, so this proves package.use (not just
    the global USE set) reaches use_reduce."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/packageuseenablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/packageuseenablepkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_package_use_entry_disables_a_globally_enabled_flag_for_one_package(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/etc/portage/package.use has a
    "dev-libs/packageusedisablepkg -foo" entry: "foo" IS enabled globally
    by the fixture profile chain (dev-libs/useflagpkg's own foo?-gated
    dependency IS pulled in, per test_real_use_flags_from_profile_gate_a_dependency),
    but package.use disables it for this one package only, so its own
    foo?-gated dependency must not be pulled in -- proving package.use is
    applied per package, not globally."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/packageusedisablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/packageusedisablepkg-1.0"]


def test_strong_blocker_matches_an_installed_package(emerge_binary, fixture_env):
    """dev-libs/blockerpkg's RDEPEND is "!!dev-libs/samepkg", and
    dev-libs/samepkg-1.0 is already installed per the fixture vdb -- a
    strong blocker match is reported (not enforced: exit code stays 0)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/blockerpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/blockerpkg-1.0",
        '[blocks] dev-libs/blockerpkg-1.0 hard blocks dev-libs/samepkg-1.0 ("!!dev-libs/samepkg")',
    ]


def test_weak_blocker_matches_another_new_package_in_the_same_graph(emerge_binary, fixture_env):
    """dev-libs/graphblockerparent pulls in both dev-libs/blockerpartnerpkg
    and dev-libs/weakblockerpkg (whose RDEPEND is
    "!dev-libs/blockerpartnerpkg") as New in the same run, so the weak
    blocker is matched against blockerpartnerpkg's graph-resolved version,
    not just the (empty, for this package) vdb -- printed right after its
    owner's own [ebuild ...] line."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/graphblockerparent"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/graphblockerparent-1.0",
        "[ebuild  N] dev-libs/blockerpartnerpkg-1.0",
        "[ebuild  N] dev-libs/weakblockerpkg-1.0",
        '[blocks] dev-libs/weakblockerpkg-1.0 soft blocks dev-libs/blockerpartnerpkg-1.0 ("!dev-libs/blockerpartnerpkg")',
    ]


def test_unrelated_package_reports_no_blockers(emerge_binary, fixture_env):
    """Regression guard: the diamond fixture has no blockers at all, so
    resolving it must not gain a spurious [blocks] line."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/diamond"], fixture_env)
    assert result.returncode == 0
    assert "[blocks]" not in result.stdout


def test_overlay_only_package_is_found(emerge_binary, fixture_env):
    """dev-libs/overlayonlypkg exists only in the fixture's overlay repo
    (see PORTING/fixtures/etc/portage/repos.conf), not the main repo --
    proving the overlay is actually searched, not just present in
    repos.conf."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlayonlypkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/overlayonlypkg-1.0"


def test_best_version_wins_regardless_of_which_repo_has_it(emerge_binary, fixture_env):
    """dev-libs/overlaynewerpkg-1.0 is in the main repo, -2.0 is in the
    overlay -- the higher version wins even though it isn't in the main
    (lower-priority) repo."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlaynewerpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/overlaynewerpkg-2.0"


def test_same_version_tie_across_repos_is_broken_toward_higher_priority(
    emerge_binary, fixture_env
):
    """dev-libs/overlaytiepkg-1.0 exists identically-versioned in both the
    main repo (priority -1000, no deps) and the overlay (priority 10,
    RDEPENDs on dev-libs/newpkg): resolving it must pull in newpkg,
    proving the overlay's own copy -- not the main repo's -- is the one
    whose metadata got read."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlaytiepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/overlaytiepkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]
