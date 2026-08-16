"""Black-box contract suite for the `emerge --pretend` pilot slice (see
PORTING/PROMPT.md and PORTING/rust/portage-repo/src/lib.rs for the full
scope writeup, including the dependency-recursion follow-up in
resolve_pretend_graph, the profile/make.conf -> real USE/ACCEPT_KEYWORDS
follow-up in portage-profile, the package.mask/.unmask/.accept_keywords/
.use follow-up on top of that, the blocker-reporting follow-up on top of
that, the overlays (multiple repos.conf repos) follow-up on top of that,
and the slot-conflict-reporting follow-up on top of that -- plus a
virtuals check confirming, against a fixture shaped exactly like the
real virtual/pager, that virtual/* atoms need no dedicated code at all:
they're ordinary packages with an any-of RDEPEND, already covered by
existing machinery -- and the CLI-surface-recognition follow-up, which
enumerates every real emerge option/action from lib/_emerge/main.py so
each one gets a specific "recognized, not implemented" message instead
of a generic "unsupported option" one). Drives the real compiled
`emerge` binary (multicall, dispatched via a real symlink
-- not a neutral harness, since emerge is an actual product surface per
PROMPT.md's testing decision) and the Python reference implementation
identically, against the synthetic fixture tree at PORTING/fixtures
(whose repos.conf/make.profile/make.conf/package.mask/package.unmask/
package.accept_keywords/package.use now drive real config resolution,
not hardcoded values, and whose repos.conf now defines a second,
higher-priority overlay repo alongside the main one), and asserts their
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
    ("versioned top-level atom: resolves New", ["--pretend", ">=dev-libs/foo-1.0"], 0),
    ("slotted top-level atom: resolves New", ["--pretend", "dev-libs/foo:0"], 0),
    ("versioned top-level atom: no version satisfies it", ["--pretend", ">=dev-libs/newpkg-9.0"], 1),
    ("slotted top-level atom: no such slot", ["--pretend", "dev-libs/newpkg:5"], 1),
    ("exact-version top-level atom: already installed", ["--pretend", "=dev-libs/samepkg-1.0"], 0),
    ("blocker top-level atom: not a valid target", ["--pretend", "!!dev-libs/newpkg"], 2),
    ("weak-blocker top-level atom: not a valid target", ["--pretend", "!dev-libs/newpkg"], 2),
    ("USE-dep top-level atom: now in v1 grammar, resolves New", ["--pretend", "dev-libs/foo[bar]"], 0),
    ("repo-constrained top-level atom: outside v1 grammar entirely", ["--pretend", "dev-libs/foo::gentoo"], 1),
    ("slot-operator top-level atom: now in v1 grammar, resolves New", ["--pretend", "dev-libs/foo:0="], 0),
    ("slot-operator top-level atom, no explicit slot: resolves New", ["--pretend", "dev-libs/foo:="], 0),
    ("bare trailing colon top-level atom: still invalid", ["--pretend", "dev-libs/foo:"], 1),
    ("explicit slot + \"*\" top-level atom: still invalid", ["--pretend", "dev-libs/foo:0*"], 1),
    ("syntactically invalid atom", ["--pretend", "not an atom!"], 1),
    ("no atom given", ["--pretend"], 2),
    ("missing --pretend", ["dev-libs/newpkg"], 2),
    ("real emerge option, value-taking, not implemented", ["--deep", "dev-libs/newpkg"], 2),
    ("real emerge option, boolean, not implemented", ["--debug", "--pretend", "dev-libs/newpkg"], 2),
    ("real emerge option, inline =value form, not implemented", ["--jobs=4", "--pretend", "dev-libs/newpkg"], 2),
    ("real emerge action, not implemented", ["--depclean"], 2),
    ("real emerge action, short alias, not implemented", ["-c"], 2),
    ("genuinely unrecognized option", ["--totally-fake-option", "dev-libs/newpkg"], 2),
    ("recursion: basic dependency chain", ["--pretend", "dev-libs/withdeps"], 0),
    ("recursion: diamond dependency dedup", ["--pretend", "dev-libs/diamond"], 0),
    ("recursion: dependency cycle terminates", ["--pretend", "dev-libs/cycle-a"], 0),
    ("recursion: any-of group resolves every alternative", ["--pretend", "dev-libs/anyof"], 0),
    ("recursion: unresolvable dep doesn't fail the graph", ["--pretend", "dev-libs/missingdep"], 0),
    ("recursion: dedup across DEPEND and RDEPEND", ["--pretend", "dev-libs/dualdep"], 0),
    ("recursion: BDEPEND is walked", ["--pretend", "dev-libs/bdependpkg"], 0),
    ("recursion: PDEPEND is walked", ["--pretend", "dev-libs/pdependpkg"], 0),
    ("recursion: IDEPEND is walked", ["--pretend", "dev-libs/idependpkg"], 0),
    ("recursion: slot-operator dependency atoms are resolved, not dropped", ["--pretend", "dev-libs/slotoperatorpkg"], 0),
    ("recursion: USE-dep dependency atoms are resolved, not dropped", ["--pretend", "dev-libs/usedeppkg"], 0),
    ("profile config: real USE flag gates a dependency", ["--pretend", "dev-libs/useflagpkg"], 0),
    ("package.mask: hidden, no unmask", ["--pretend", "dev-libs/hardmaskedpkg"], 1),
    ("package.mask + package.unmask: masked then unmasked", ["--pretend", "dev-libs/maskedandunmaskedpkg"], 0),
    ("package.mask: -atom removal leaves candidate unaffected", ["--pretend", "dev-libs/samepkg"], 0),
    ("package.mask: repo-level profiles/package.mask hides a package", ["--pretend", "dev-libs/repomaskedpkg"], 1),
    ("package.mask: profile-level package.mask hides a package", ["--pretend", "dev-libs/profilemaskedpkg"], 1),
    ("package.mask: profile-level package.unmask cancels a repo-level mask", ["--pretend", "dev-libs/repomaskedthenprofileunmaskedpkg"], 0),
    ("package.mask: user-level -atom removes a repo-level mask entry", ["--pretend", "dev-libs/repomaskedthenuserremovedpkg"], 0),
    ("package.accept_keywords: wildcard extends visibility", ["--pretend", "dev-libs/wildcardkeywordpkg"], 0),
    ("package.accept_keywords: profile-level entry extends visibility", ["--pretend", "dev-libs/profileacceptkeywordspkg"], 0),
    ("package.accept_keywords: ** accepts no-keywords package", ["--pretend", "dev-libs/livekeywordpkg"], 0),
    ("package.use: wildcard entry enables a flag not on globally", ["--pretend", "dev-libs/packageuseenablepkg"], 0),
    ("package.use: entry disables a flag that is on globally", ["--pretend", "dev-libs/packageusedisablepkg"], 0),
    ("blocker: strong (!!) blocker matches an installed package", ["--pretend", "dev-libs/blockerpkg"], 0),
    ("blocker: weak (!) blocker matches another new package in the graph", ["--pretend", "dev-libs/graphblockerparent"], 0),
    ("overlay: package exists only in the overlay repo", ["--pretend", "dev-libs/overlayonlypkg"], 0),
    ("overlay: best version wins across repos", ["--pretend", "dev-libs/overlaynewerpkg"], 0),
    ("overlay: same-version tie broken toward higher priority", ["--pretend", "dev-libs/overlaytiepkg"], 0),
    ("slot conflict: two incompatible version constraints on one slot", ["--pretend", "dev-libs/slotconflictparent"], 0),
    ("slot conflict: different slots of the same package coexist", ["--pretend", "dev-libs/multislotparent"], 0),
    ("virtual: resolved directly", ["--pretend", "virtual/texteditor"], 0),
    ("virtual: resolved as a dependency", ["--pretend", "dev-libs/virtualconsumerpkg"], 0),
    ("multi-atom: two independent new packages", ["--pretend", "dev-libs/newpkg", "dev-libs/withdeps"], 0),
    ("multi-atom: literal duplicate atom dedupes silently", ["--pretend", "dev-libs/newpkg", "dev-libs/newpkg"], 0),
    ("multi-atom: dependency shared between two targets dedupes", ["--pretend", "dev-libs/shared-a", "dev-libs/shared-b"], 0),
    ("multi-atom: slot conflict between two targets (not just two deps)", ["--pretend", "dev-libs/slotconflictnewconsumer", "dev-libs/slotconflictoldconsumer"], 0),
    ("multi-atom: all requested atoms already installed", ["--pretend", "dev-libs/samepkg", "dev-libs/samepkg"], 0),
    ("multi-atom: a nonexistent atom aborts the whole run, first-bad-wins", ["--pretend", "dev-libs/does-not-exist", "dev-libs/newpkg"], 1),
    ("multi-atom: a later nonexistent atom still aborts the whole run", ["--pretend", "dev-libs/newpkg", "dev-libs/does-not-exist"], 1),
    ("--verbose is now implemented, not rejected", ["--pretend", "--verbose", "dev-libs/newpkg"], 0),
    ("-v short alias is now implemented, not rejected", ["--pretend", "-v", "dev-libs/newpkg"], 0),
    ("without --verbose, USE= is never shown even for a package with IUSE", ["--pretend", "dev-libs/useflagpkg"], 0),
    ("-v on a package with no IUSE at all: no USE= line", ["--pretend", "-v", "dev-libs/newpkg"], 0),
    ("-v combined with a real-but-unimplemented option: still rejected", ["--pretend", "-v", "--deep", "dev-libs/newpkg"], 2),
    ("-v explicit disable via a following \"n\" token", ["--pretend", "-v", "n", "dev-libs/useflagpkg"], 0),
    ("-v explicit enable via a following \"y\" token", ["--pretend", "-v", "y", "dev-libs/useflagpkg"], 0),
    ("--verbose=n inline form disables", ["--pretend", "--verbose=n", "dev-libs/useflagpkg"], 0),
    ("--verbose=y inline form enables", ["--pretend", "--verbose=y", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -pv: both implemented flags", ["-pv", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -vp: order doesn't matter", ["-vp", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -pd: pretend + unimplemented option", ["-pd", "dev-libs/useflagpkg"], 2),
    ("short-flag bundle -pz: pretend + genuinely unrecognized", ["-pz", "dev-libs/useflagpkg"], 2),
    ("bundled -v never consumes a following token as its value", ["-pv", "n"], 1),
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


def test_bdepend_pdepend_idepend_are_walked_same_as_depend_rdepend(
    emerge_binary, fixture_env
):
    """Prior to this slice, resolve_pretend_graph only concatenated
    DEPEND+RDEPEND before flattening -- a package whose only dependency
    was declared via BDEPEND (build-time, EAPI 7+), PDEPEND (post-merge),
    or IDEPEND (install-time, EAPI 8+, rare) would silently resolve with
    no dependencies at all. v1 makes no distinction between any of the
    five real dependency-string keys (this pilot has no real merge
    ordering for the distinction to matter to), so each of these three
    single-key fixtures must still pull in dev-libs/newpkg exactly like
    dev-libs/withdeps's own DEPEND/RDEPEND-based fixture does."""
    for pkg in ("bdependpkg", "pdependpkg", "idependpkg"):
        result = _run([str(emerge_binary)], ["--pretend", f"dev-libs/{pkg}"], fixture_env)
        assert result.returncode == 0, pkg
        assert result.stdout.splitlines() == [
            f"[ebuild  N] dev-libs/{pkg}-1.0",
            "[ebuild  N] dev-libs/newpkg-1.0",
        ], pkg


def test_slot_operator_dependency_atoms_resolve_both_forms(emerge_binary, fixture_env):
    """dev-libs/slotoperatorpkg's own RDEPEND is
    "dev-libs/newpkg:= dev-libs/multislotpkg:1=" -- both real slot-operator
    forms (PMS 8.3.3). Prior to this slice, portage-dep's v1 grammar
    didn't parse slot operators AT ALL, and resolve_pretend_graph's BFS
    loop treats a parse failure as "not a dependency, skip it"
    (`let Some(atom) = parse_atom(..) else { continue }`), not as an
    unresolvable one -- so both tokens would have been silently dropped
    from the graph entirely: no entry, no error, nothing. This was a
    genuine Rust-only parity bug, not just a documented gap: the Python
    reference side's own dependency-atom parsing always used the real,
    unrestricted portage.dep.Atom (only the CLI's top-level-atom
    validation narrowed the grammar), so it already resolved this
    fixture correctly before this slice touched anything on the Python
    side at all. ":=" (no explicit slot) must pull in newpkg regardless
    of its slot; ":1=" (an explicit slot) must resolve multislotpkg's
    SLOT=1 version specifically (2.0), not its SLOT=0 version (1.0)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/slotoperatorpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/slotoperatorpkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/multislotpkg-2.0",
    ]


def test_use_dep_dependency_atoms_are_resolved_not_dropped(emerge_binary, fixture_env):
    """dev-libs/usedeppkg's own RDEPEND is
    "dev-libs/newpkg[bar] dev-libs/multislotpkg:1[baz?]" -- same class of
    bug slot operators had (see
    test_slot_operator_dependency_atoms_resolve_both_forms): before this
    slice, portage-dep's v1 grammar didn't parse USE deps at all, so both
    tokens would have been silently dropped from the graph. The USE-dep
    constraint itself is deliberately never enforced by matching (see
    portage-dep's module doc comment -- verified against real portage,
    which already behaves identically for plain-string candidates), so
    both dependencies resolve exactly as if the "[...]" suffix weren't
    there at all -- newpkg (any version) and multislotpkg's SLOT=1
    version (2.0, combined with a plain slot restriction here rather than
    a slot operator)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/usedeppkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/usedeppkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/multislotpkg-2.0",
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
        == 'emerge: there are no ebuilds to satisfy "dev-libs/hardmaskedpkg".'
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


def test_repo_level_package_mask_hides_a_package(emerge_binary, fixture_env):
    """PORTING/fixtures/repo/profiles/package.mask (the main repo's own
    repo-level mask -- real portage's most common real-world masking
    source, e.g. security/arch masks) hides dev-libs/repomaskedpkg with
    no matching unmask anywhere, same "no ebuilds to satisfy" outcome a
    user-level mask produces."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/repomaskedpkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/repomaskedpkg".'
    )


def test_profile_level_package_mask_hides_a_package(emerge_binary, fixture_env):
    """PORTING/fixtures/repo/profiles/base/package.mask (a package.mask
    file at one level of the profile inheritance chain, not the repo
    root or /etc/portage) hides dev-libs/profilemaskedpkg -- proving
    per-profile-level package.mask is actually read, not just the
    repo-level and user-level sources."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/profilemaskedpkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/profilemaskedpkg".'
    )


def test_profile_level_package_unmask_cancels_a_repo_level_mask(emerge_binary, fixture_env):
    """dev-libs/repomaskedthenprofileunmaskedpkg is masked by the
    repo-level profiles/package.mask, then unmasked by
    PORTING/fixtures/repo/profiles/default/package.unmask -- a
    profile-level package.unmask entry cancelling a mask from an
    earlier-stacked source (the repo level), proving the three sources
    are genuinely stacked together, not checked independently."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/repomaskedthenprofileunmaskedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert (
        result.stdout.strip() == "[ebuild  N] dev-libs/repomaskedthenprofileunmaskedpkg-1.0"
    )


def test_user_level_minus_atom_removes_a_repo_level_mask_entry(emerge_binary, fixture_env):
    """dev-libs/repomaskedthenuserremovedpkg is masked by the repo-level
    profiles/package.mask; PORTING/fixtures/etc/portage/package.mask's
    own "-dev-libs/repomaskedthenuserremovedpkg" line removes that entry
    even though it didn't add it -- proving -atom removal now applies
    across the whole combined [repo, profile chain, user] stack (real
    MaskManager.py's stack_lists(incremental=1) semantics), not just
    within the single file that contains the "-atom" line, which is all
    the pilot supported before this slice."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/repomaskedthenuserremovedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/repomaskedthenuserremovedpkg-1.0"


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


def test_package_accept_keywords_profile_level_entry_extends_visibility(
    emerge_binary, fixture_env
):
    """dev-libs/profileacceptkeywordspkg is only ~amd64, made visible not
    by the user-level package.accept_keywords fixture (which has no entry
    for it) but by PORTING/fixtures/repo/profiles/arch/amd64's own
    package.accept_keywords -- proving package.accept_keywords is now
    stacked from the profile chain too, not just user-level, mirroring
    real KeywordsManager.getPKeywords (which has no repo-level source for
    this file at all, unlike package.mask)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/profileacceptkeywordspkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/profileacceptkeywordspkg-1.0"


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


def test_slot_conflict_is_reported_between_two_incompatible_version_constraints(
    emerge_binary, fixture_env
):
    """dev-libs/slotconflictparent pulls in slotconflictnewconsumer (bare
    RDEPEND on slotconflicttarget, resolves the best version, 2.0, first)
    and slotconflictoldconsumer (RDEPEND "<dev-libs/slotconflicttarget-2.0",
    which 2.0 itself does NOT satisfy) -- both want slot 0 of the same
    package at versions that can't both be right, so this must surface as
    a [slot conflict] line, not a second, silently-overwriting entry, and
    must not change the exit code (purely informational, like blockers)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/slotconflictparent"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/slotconflictparent-1.0",
        "[ebuild  N] dev-libs/slotconflictnewconsumer-1.0",
        "[ebuild  N] dev-libs/slotconflictoldconsumer-1.0",
        "[ebuild  N] dev-libs/slotconflicttarget-2.0",
        '[slot conflict] dev-libs/slotconflicttarget:0 resolved to dev-libs/slotconflicttarget-2.0, which does not satisfy "<dev-libs/slotconflicttarget-2.0"',
    ]


def test_different_slots_of_the_same_package_coexist_without_conflict(emerge_binary, fixture_env):
    """dev-libs/multislotparent RDEPENDs on both dev-libs/multislotpkg:0
    and dev-libs/multislotpkg:1 -- real, different slots of the same
    package are normal coexistence (like dev-lang/python:3.11 and
    :3.12), not a conflict: both must appear as independent [ebuild N]
    lines, with no [slot conflict] line at all."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/multislotparent"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/multislotparent-1.0",
        "[ebuild  N] dev-libs/multislotpkg-1.0",
        "[ebuild  N] dev-libs/multislotpkg-2.0",
    ]
    assert "[slot conflict]" not in result.stdout


def test_multiple_top_level_atoms_share_dedup_and_slot_conflict_machinery(
    emerge_binary, fixture_env
):
    """Two top-level atoms passed in one invocation seed the same BFS as a
    single atom's own dependencies would: dev-libs/shared-a and
    dev-libs/shared-b (both used by the diamond-dependency fixture) share
    dev-libs/common, which must appear exactly once, not twice -- proving
    the multi-atom slice reuses the existing visited-atoms dedup rather
    than resolving each requested atom in isolation."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/shared-a", "dev-libs/shared-b"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/shared-a-1.0",
        "[ebuild  N] dev-libs/shared-b-1.0",
        "[ebuild  N] dev-libs/common-1.0",
    ]


def test_multiple_top_level_atoms_detect_a_slot_conflict_between_targets(
    emerge_binary, fixture_env
):
    """Same slot-conflict fixture pair as
    test_slot_conflict_is_reported_between_two_incompatible_version_constraints,
    but requested directly as two top-level atoms instead of reached
    through a shared parent -- proving a slot conflict between two
    *targets* (not just between two dependencies of one target) is
    detected too, since resolve_pretend_graph now seeds all top-level
    atoms into the same BFS/resolved_slots bookkeeping."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/slotconflictnewconsumer", "dev-libs/slotconflictoldconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/slotconflictnewconsumer-1.0",
        "[ebuild  N] dev-libs/slotconflictoldconsumer-1.0",
        "[ebuild  N] dev-libs/slotconflicttarget-2.0",
        '[slot conflict] dev-libs/slotconflicttarget:0 resolved to dev-libs/slotconflicttarget-2.0, which does not satisfy "<dev-libs/slotconflicttarget-2.0"',
    ]


def test_multiple_top_level_atoms_dedupe_a_literal_duplicate(emerge_binary, fixture_env):
    """emerge --pretend foo foo: the second occurrence dedupes silently
    via the existing visited-atoms set, same as a dependency cycle does
    -- exactly one [ebuild N] line, not two."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/newpkg", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/newpkg-1.0"]


def test_multiple_top_level_atoms_all_already_installed(emerge_binary, fixture_env):
    """Generalizes the old single-atom "already installed; nothing to do"
    shortcut: every requested top-level atom that resolves
    AlreadyInstalled gets its own such line (there's no longer a
    len(entries) == 1 special case)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/samepkg", "dev-libs/samepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["dev-libs/samepkg-1.0 is already installed; nothing to do"]


def test_bad_top_level_atom_aborts_the_whole_run_in_argv_order(emerge_binary, fixture_env):
    """A top-level atom with no visible candidate is fatal to the whole
    call (matching real portage's own depgraph.py "there are no ebuilds
    to satisfy" behavior), not reported-and-continued the way a
    dependency's own NoVisibleCandidate is -- confirmed with the user
    before implementing. Since top-level atoms are always dequeued in
    argv order before any dependency, the *first* bad one wins: a good
    atom placed after a bad one is never even attempted, and a good atom
    placed before a bad one doesn't get its own output printed either,
    since the whole call aborts before any printing happens at the CLI
    layer."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/newpkg", "dev-libs/does-not-exist"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == 'emerge: there are no ebuilds to satisfy "dev-libs/does-not-exist".'


def test_versioned_and_slotted_top_level_atoms_resolve_like_bare_ones(
    emerge_binary, fixture_env
):
    """>=dev-libs/newpkg-1.0 and dev-libs/newpkg:0 are both real,
    common invocation shapes (`emerge '>=foo-1.2'`, `emerge foo:0`) that
    the CLI used to reject outright ("only a bare category/package atom
    is supported") even though resolve_pretend's own atom-vs-candidate
    matching already handled operators/slots correctly for every
    dependency atom -- lifting the CLI-level restriction was the entire
    slice, no resolution-logic changes needed."""
    for atom in (">=dev-libs/newpkg-1.0", "dev-libs/newpkg:0"):
        result = _run([str(emerge_binary)], ["--pretend", atom], fixture_env)
        assert result.returncode == 0, atom
        assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/newpkg-1.0"], atom


def test_blocker_top_level_atom_is_rejected_not_silently_dropped(emerge_binary, fixture_env):
    """Prior to this slice, the CLI's bare-atom check never tested for a
    blocker at all: `emerge --pretend '!!foo'` was accepted (operator/
    slot/version were all unset) and then silently dropped by
    resolve_pretend_graph's own `atom.blocker != Blocker::None` BFS skip
    -- exit 0, no output, no error. A blocker isn't a valid emerge target
    in real portage either, so this must now be rejected explicitly."""
    result = _run([str(emerge_binary)], ["--pretend", "!!dev-libs/newpkg"], fixture_env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge (pilot v1): "!!dev-libs/newpkg" is a blocker, not a valid emerge target'
    )


def test_verbose_shows_use_flags_gated_by_profile_and_make_conf(emerge_binary, fixture_env):
    """dev-libs/useflagpkg declares IUSE="foo missingflag"; the fixture
    profile chain resolves "foo" enabled and "missingflag" disabled (see
    portage-profile's own fixture test) -- -v must show both, enabled
    plain and disabled "-"-prefixed, alphabetically ordered. Without -v,
    no USE= appears at all, even though the same data was computed."""
    verbose = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/useflagpkg"], fixture_env
    )
    assert verbose.returncode == 0
    assert verbose.stdout.splitlines()[0] == '[ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"'

    quiet = _run([str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg"], fixture_env)
    assert quiet.returncode == 0
    assert quiet.stdout.splitlines()[0] == "[ebuild  N] dev-libs/useflagpkg-1.0"
    assert "USE=" not in quiet.stdout


def test_verbose_use_flags_reflect_package_use_overrides(emerge_binary, fixture_env):
    """Reuses the package.use fixtures from the package.use slice:
    packageuseenablepkg's own IUSE="pkguseflag" is enabled only via a
    */packageuseenablepkg wildcard entry (off globally), and
    packageusedisablepkg's own IUSE="foo" is disabled only via a
    dev-libs/packageusedisablepkg entry (on globally) -- -v's USE=
    display must reflect the same per-package effective_use_flags result
    dependency recursion itself already uses, not the global set."""
    enable = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/packageuseenablepkg"], fixture_env
    )
    assert enable.stdout.splitlines()[0] == (
        '[ebuild  N] dev-libs/packageuseenablepkg-1.0  USE="pkguseflag"'
    )

    disable = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/packageusedisablepkg"], fixture_env
    )
    assert disable.stdout.splitlines() == [
        '[ebuild  N] dev-libs/packageusedisablepkg-1.0  USE="-foo"'
    ]


def test_verbose_on_a_package_with_no_iuse_shows_no_use_line(emerge_binary, fixture_env):
    """dev-libs/newpkg declares no IUSE at all -- -v must not print an
    empty USE="" line, matching real portage's own "nothing to show"
    behavior (_create_use_string returns "" when there's nothing to
    join)."""
    result = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/newpkg-1.0"]


def test_verbose_consumes_an_explicit_y_or_n_value(emerge_binary, fixture_env):
    """-v/--verbose is not a plain boolean in real emerge -- it's
    registered with choices=("True", "y", "n"), and insert_optional_args
    inserts "True" only when no explicit value follows. A standalone -v
    (not bundled) must consume an immediately-following "n" as an
    explicit disable, or "y" as an explicit enable -- verified by tracing
    real insert_optional_args by hand, not guessed."""
    disabled = _run(
        [str(emerge_binary)], ["--pretend", "-v", "n", "dev-libs/useflagpkg"], fixture_env
    )
    assert disabled.returncode == 0
    assert disabled.stdout.splitlines()[0] == "[ebuild  N] dev-libs/useflagpkg-1.0"
    assert "USE=" not in disabled.stdout

    enabled = _run(
        [str(emerge_binary)], ["--pretend", "-v", "y", "dev-libs/useflagpkg"], fixture_env
    )
    assert enabled.returncode == 0
    assert enabled.stdout.splitlines()[0] == '[ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"'


def test_verbose_inline_equals_form_consumes_y_or_n(emerge_binary, fixture_env):
    """--verbose=y / --verbose=n (argparse's own native "=" syntax, a
    separate mechanism from insert_optional_args's next-token lookahead)
    must be honored the same way."""
    disabled = _run(
        [str(emerge_binary)], ["--pretend", "--verbose=n", "dev-libs/useflagpkg"], fixture_env
    )
    assert disabled.returncode == 0
    assert "USE=" not in disabled.stdout

    enabled = _run(
        [str(emerge_binary)], ["--pretend", "--verbose=y", "dev-libs/useflagpkg"], fixture_env
    )
    assert enabled.returncode == 0
    assert 'USE="foo -missingflag"' in enabled.stdout


def test_short_flag_bundle_pv_enables_both_pretend_and_verbose(emerge_binary, fixture_env):
    """-pv (and -vp, order shouldn't matter) decomposes into -p + -v,
    both real, implemented flags -- real argparse's own native bundling
    for plain boolean short options, verified directly against real
    argparse before relying on it. Prior to this slice, a bundled token
    like this matched no table entry at all and was reported as
    "unrecognized option", a worse outcome than even a "recognized, not
    implemented" report would have been, since -p and -v genuinely are
    both implemented."""
    for bundle in ("-pv", "-vp"):
        result = _run([str(emerge_binary)], [bundle, "dev-libs/useflagpkg"], fixture_env)
        assert result.returncode == 0, bundle
        assert (
            result.stdout.splitlines()[0]
            == '[ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"'
        ), bundle


def test_short_flag_bundle_reports_the_first_out_of_scope_character(
    emerge_binary, fixture_env
):
    """-pd (pretend + real-but-unimplemented -d/--debug) and -pz
    (pretend + a genuinely unrecognized "-z") each decompose left to
    right, processing "-p" silently and then reporting on the next
    character exactly as a standalone occurrence of it would -- same
    messages, same exit code."""
    unimplemented = _run(
        [str(emerge_binary)], ["-pd", "dev-libs/useflagpkg"], fixture_env
    )
    assert unimplemented.returncode == 2
    assert (
        unimplemented.stderr.strip()
        == 'emerge (pilot v1): option "--debug" is a real emerge option, but is not '
        "implemented in this pilot (only --pretend/-p and --verbose/-v are implemented "
        "so far; see PROMPT.md)"
    )

    unrecognized = _run(
        [str(emerge_binary)], ["-pz", "dev-libs/useflagpkg"], fixture_env
    )
    assert unrecognized.returncode == 2
    assert unrecognized.stderr.strip() == 'emerge: unrecognized option "-z"'


def test_bundled_verbose_never_consumes_the_next_token_as_its_value(
    emerge_binary, fixture_env
):
    """Real emerge's own insert_optional_args never lets a *bundled*
    short option (one sharing a token with another flag) pick up an
    inline or next-token value -- only a standalone, unbundled -v does
    (see test_verbose_consumes_an_explicit_y_or_n_value). "-pv n" must
    treat "n" as a positional atom, not as -v's value -- proven here by
    it failing as an invalid atom, not by "n" silently disabling
    verbose."""
    result = _run([str(emerge_binary)], ["-pv", "n"], fixture_env)
    assert result.returncode == 1
    assert result.stderr.strip() == 'emerge: invalid atom "n"'


def test_virtual_is_resolved_directly(emerge_binary, fixture_env):
    """virtual/texteditor is shaped exactly like a real virtual (e.g.
    virtual/pager in the real Gentoo tree, confirmed by inspection): an
    ordinary ebuild whose RDEPEND is a "|| ( ... )" any-of group of real
    providers, no PROVIDE mechanism or special resolution involved. It
    must resolve through the exact same category + any-of-group
    machinery as any other package -- v1's documented any-of behavior
    (resolve every alternative, only show the one that would newly
    merge) picks dev-libs/newpkg (New) over dev-libs/samepkg (already
    installed, stays silent)."""
    result = _run([str(emerge_binary)], ["--pretend", "virtual/texteditor"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] virtual/texteditor-0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_virtual_is_resolved_as_a_dependency(emerge_binary, fixture_env):
    """dev-libs/virtualconsumerpkg RDEPENDs on virtual/texteditor --
    proving a virtual/ atom extracted from another package's own
    DEPEND/RDEPEND resolves identically to the top-level case above,
    with no virtual-specific code path anywhere in this pilot."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/virtualconsumerpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/virtualconsumerpkg-1.0",
        "[ebuild  N] virtual/texteditor-0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_real_option_not_implemented_message_names_the_option(emerge_binary, fixture_env):
    """--deep is a real emerge option (see lib/_emerge/main.py's
    argument_options) this pilot doesn't implement -- the error must
    name it specifically and say "option", distinct from both a
    genuinely unrecognized flag and an unimplemented action."""
    result = _run([str(emerge_binary)], ["--deep", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge (pilot v1): option "--deep" is a real emerge option, but is not '
        "implemented in this pilot (only --pretend/-p and --verbose/-v are implemented so far; see "
        "PROMPT.md)"
    )


def test_real_option_inline_equals_form_is_still_recognized(emerge_binary, fixture_env):
    """--jobs=4 (the "--opt=value" form argparse also accepts) must
    resolve to the same canonical "--jobs" option as "--jobs 4" would,
    not be treated as one unrecognized token."""
    result = _run([str(emerge_binary)], ["--jobs=4", "--pretend", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 2
    assert (
        result.stderr.strip()
        == 'emerge (pilot v1): option "--jobs" is a real emerge option, but is not '
        "implemented in this pilot (only --pretend/-p and --verbose/-v are implemented so far; see "
        "PROMPT.md)"
    )


def test_real_action_not_implemented_message_says_action_not_option(emerge_binary, fixture_env):
    """--depclean is a real emerge action (see main.py's actions
    frozenset), not an option -- the error must say "action", and its
    short alias -c (see shortmapping) must report the same canonical
    "--depclean" name."""
    result = _run([str(emerge_binary)], ["--depclean"], fixture_env)
    assert result.returncode == 2
    expected = (
        'emerge (pilot v1): action "--depclean" is a real emerge action, but is not '
        "implemented in this pilot (only --pretend/-p and --verbose/-v are implemented so far; see "
        "PROMPT.md)"
    )
    assert result.stderr.strip() == expected

    short_result = _run([str(emerge_binary)], ["-c"], fixture_env)
    assert short_result.returncode == 2
    assert short_result.stderr.strip() == expected


def test_genuinely_unrecognized_option_gets_a_distinct_message(emerge_binary, fixture_env):
    """A flag that isn't in real emerge's own option surface at all must
    be reported differently from a real-but-unimplemented one, so users
    can tell a typo apart from a pilot scope gap."""
    result = _run(
        [str(emerge_binary)], ["--totally-fake-option", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 2
    assert result.stderr.strip() == 'emerge: unrecognized option "--totally-fake-option"'
