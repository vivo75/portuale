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
    (
        "without --update, a newer visible version is not offered",
        ["--pretend", "dev-libs/upgradepkg"],
        0,
    ),
    ("--update: upgrade available", ["--pretend", "--update", "dev-libs/upgradepkg"], 0),
    ("-u short alias for --update", ["--pretend", "-u", "dev-libs/upgradepkg"], 0),
    ("without --deep, an already-installed package's own deps stay unwalked", ["--pretend", "dev-libs/deeppkg"], 0),
    ("--deep: walks the whole already-installed chain", ["--pretend", "--deep", "dev-libs/deeppkg"], 0),
    ("-D short alias for --deep", ["--pretend", "-D", "dev-libs/deeppkg"], 0),
    ("--deep=N inline form", ["--pretend", "--deep=2", "dev-libs/deeppkg"], 0),
    ("--deep=0 matches not passing --deep at all", ["--pretend", "--deep=0", "dev-libs/deeppkg"], 0),
    ("--deep=-1 is a real, immediate parse error", ["--pretend", "--deep=-1", "dev-libs/deeppkg"], 2),
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
    (
        "USE-dep top-level atom: now in v1 grammar, resolves New",
        ["--pretend", "dev-libs/foo[bar(+)]"],
        0,
    ),
    ("repo-constrained top-level atom: resolves New from the named repo", ["--pretend", "dev-libs/foo::testrepo"], 0),
    ("repo-constrained top-level atom: wrong repo, no ebuilds satisfy it", ["--pretend", "dev-libs/foo::overlay"], 1),
    (
        "package.use.mask/package.use.force with atom-specificity ordering",
        ["--pretend", "-v", "dev-libs/pkgusemaskforcepkg"],
        0,
    ),
    ("slot-operator top-level atom: now in v1 grammar, resolves New", ["--pretend", "dev-libs/foo:0="], 0),
    ("slot-operator top-level atom, no explicit slot: resolves New", ["--pretend", "dev-libs/foo:="], 0),
    ("bare trailing colon top-level atom: still invalid", ["--pretend", "dev-libs/foo:"], 1),
    ("explicit slot + \"*\" top-level atom: still invalid", ["--pretend", "dev-libs/foo:0*"], 1),
    ("syntactically invalid atom", ["--pretend", "not an atom!"], 1),
    ("no atom given", ["--pretend"], 2),
    ("missing --pretend", ["dev-libs/newpkg"], 2),
    ("real emerge option, value-taking, not implemented", ["--jobs", "dev-libs/newpkg"], 2),
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
    (
        "recursion: a genuinely unsatisfied USE-dep dependency atom is rejected",
        ["--pretend", "dev-libs/usedeprejectedpkg"],
        0,
    ),
    (
        "USE-dep enforcement: plain flag declared and enabled matches",
        ["--pretend", "dev-libs/useflagpkg[foo]"],
        0,
    ),
    (
        "USE-dep enforcement: negated flag declared but enabled does not match",
        ["--pretend", "dev-libs/useflagpkg[-foo]"],
        1,
    ),
    (
        "USE-dep enforcement: plain flag declared but disabled does not match",
        ["--pretend", "dev-libs/useflagpkg[missingflag]"],
        1,
    ),
    (
        "USE-dep enforcement: negated flag declared and disabled matches",
        ["--pretend", "dev-libs/useflagpkg[-missingflag]"],
        0,
    ),
    (
        "USE-dep enforcement: flag not declared in IUSE at all, no default, never matches",
        ["--pretend", "dev-libs/useflagpkg[nonexistentflag]"],
        1,
    ),
    (
        "USE-dep enforcement: (+) default rescues a flag missing from IUSE",
        ["--pretend", "dev-libs/useflagpkg[nonexistentflag(+)]"],
        0,
    ),
    (
        "REQUIRED_USE: genuinely satisfied, resolves normally",
        ["--pretend", "dev-libs/requireduseokpkg"],
        0,
    ),
    (
        "REQUIRED_USE: genuinely violated, top-level atom, aborts the whole run",
        ["--pretend", "dev-libs/requiredusebadpkg"],
        1,
    ),
    (
        "REQUIRED_USE: violated on a dependency, still aborts the whole run",
        ["--pretend", "dev-libs/requiredusebadparentpkg"],
        1,
    ),
    ("profile config: real USE flag gates a dependency", ["--pretend", "dev-libs/useflagpkg"], 0),
    (
        "USE_EXPAND: VIDEO_CARDS=nvidia expands to video_cards_nvidia, gates a dependency",
        ["--pretend", "-v", "dev-libs/useexpandpkg"],
        0,
    ),
    (
        "package.use USE_EXPAND-prefix shorthand: PYTHON_TARGETS: python3_12 gates a dependency",
        ["--pretend", "-v", "dev-libs/packageuseexpandpkg"],
        0,
    ),
    (
        "use.stable.force/package.use.stable.mask apply to a genuinely stable candidate",
        ["--pretend", "-v", "dev-libs/stableusepkg"],
        0,
    ),
    (
        "use.stable.force/package.use.stable.mask do not apply to an unstable candidate",
        ["--pretend", "-v", "dev-libs/unstableusepkg"],
        0,
    ),
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
    ("package.use: repo-level entry enables a flag not on globally", ["--pretend", "dev-libs/repouseenablepkg"], 0),
    ("package.use: profile-level entry enables a flag not on globally", ["--pretend", "dev-libs/profileuseenablepkg"], 0),
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
    ("-v combined with a real-but-unimplemented option: still rejected", ["--pretend", "-v", "--jobs", "dev-libs/newpkg"], 2),
    ("-v explicit disable via a following \"n\" token", ["--pretend", "-v", "n", "dev-libs/useflagpkg"], 0),
    ("-v explicit enable via a following \"y\" token", ["--pretend", "-v", "y", "dev-libs/useflagpkg"], 0),
    ("--verbose=n inline form disables", ["--pretend", "--verbose=n", "dev-libs/useflagpkg"], 0),
    ("--verbose=y inline form enables", ["--pretend", "--verbose=y", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -pv: both implemented flags", ["-pv", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -vp: order doesn't matter", ["-vp", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -pd: pretend + unimplemented option", ["-pd", "dev-libs/useflagpkg"], 2),
    ("short-flag bundle -pz: pretend + genuinely unrecognized", ["-pz", "dev-libs/useflagpkg"], 2),
    ("bundled -v never consumes a following token as its value", ["-pv", "n"], 1),
    ("--help is now implemented, not rejected", ["--help"], 0),
    ("-h short alias is now implemented, not rejected", ["-h"], 0),
    ("--help wins over any other flag present, valid or not", ["--jobs", "--help"], 0),
    ("-h bundled with other short flags still wins", ["-ph"], 0),
    ("--help wins even without --pretend at all", ["--help", "dev-libs/newpkg"], 0),
    ("@world expands to the fixture world file's own atoms", ["--pretend", "@world"], 0),
    ("@world combined with an explicit atom too", ["--pretend", "dev-libs/samepkg", "@world"], 0),
    ("@system expands to the fixture profile chain's own packages files", ["--pretend", "@system"], 0),
    ("@system combined with an explicit atom too", ["--pretend", "dev-libs/samepkg", "@system"], 0),
    ("@some-other-set as a top-level atom: not implemented, clear invalid-atom error", ["--pretend", "@some-other-set"], 1),
    (
        "--newuse reinstalls a package whose USE changed since it was installed",
        ["--pretend", "--newuse", "dev-libs/reinstallpkg"],
        0,
    ),
    (
        "--newuse short alias -N, bundled with -p",
        ["-pN", "dev-libs/reinstallpkg"],
        0,
    ),
    (
        "without --newuse, a USE-changed package stays already-installed",
        ["--pretend", "dev-libs/reinstallpkg"],
        0,
    ),
    (
        "--newuse is a no-op when USE hasn't changed",
        ["--pretend", "--newuse", "dev-libs/samepkg"],
        0,
    ),
    (
        "--newuse forced_flags suppresses a spurious reinstall (use.mask)",
        ["--pretend", "--newuse", "dev-libs/usemaskreinstallpkg"],
        0,
    ),
    (
        "--newuse reinstalls for a newly-added IUSE flag alone",
        ["--pretend", "--newuse", "dev-libs/changedusepkg"],
        0,
    ),
    (
        "--changed-use ignores that same newly-added IUSE flag",
        ["--pretend", "--changed-use", "dev-libs/changedusepkg"],
        0,
    ),
    (
        "--changed-use short alias -U, bundled with -p",
        ["-pU", "dev-libs/changedusepkg"],
        0,
    ),
    (
        "--changed-use still catches an enablement change on a shared IUSE flag",
        ["--pretend", "--changed-use", "dev-libs/reinstallpkg"],
        0,
    ),
    (
        "--nodeps disables recursion into DEPEND/RDEPEND entirely",
        ["--pretend", "--nodeps", "dev-libs/withdeps"],
        0,
    ),
    (
        "--nodeps short alias -O, bundled with -p",
        ["-pO", "dev-libs/withdeps"],
        0,
    ),
    (
        "--nodeps still shows the top-level atom's own USE display with -v",
        ["--pretend", "-O", "-v", "dev-libs/useflagpkg"],
        0,
    ),
    (
        "--onlydeps suppresses the top-level atom, shows its dependencies",
        ["--pretend", "--onlydeps", "dev-libs/withdeps"],
        0,
    ),
    (
        "--onlydeps short alias -o, bundled with -p",
        ["-po", "dev-libs/withdeps"],
        0,
    ),
    (
        "--onlydeps on an already-installed top-level atom: no output at all",
        ["--pretend", "--onlydeps", "dev-libs/samepkg"],
        0,
    ),
    (
        "--update threads through dependency recursion, not just top-level",
        ["--pretend", "--update", "dev-libs/withdeps"],
        0,
    ),
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
    "dev-libs/newpkg[bar(+)] dev-libs/multislotpkg:1[baz(+)?]" -- same
    class of bug slot operators had (see
    test_slot_operator_dependency_atoms_resolve_both_forms): before this
    slice, portage-dep's v1 grammar didn't parse USE deps at all, so both
    tokens would have been silently dropped from the graph. Both use-dep
    flags are `(+)`-defaulted and missing from their own target's IUSE
    (see use_deps_satisfied's own doc comment, portage-dep, for why that
    trivially satisfies them regardless of profile USE state) -- proving
    USE-dep atoms are genuinely resolved AND enforced now, not just
    grammar-recognized-but-ignored (see the dedicated USE-dep enforcement
    tests below for the rejection side of that)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/usedeppkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/usedeppkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/multislotpkg-2.0",
    ]


def test_use_dep_rejected_dependency_atom_reports_no_visible_ebuild(
    emerge_binary, fixture_env
):
    """dev-libs/usedeprejectedpkg's own RDEPEND is
    "dev-libs/useflagpkg[-foo]" -- useflagpkg's own "foo" is enabled
    globally by the fixture profile chain (see the plain --verbose
    contract test), so "-foo" is genuinely never satisfied. Same "report,
    don't fail the whole graph" spirit as an unresolvable dependency
    (test_unresolvable_dependency_is_reported_not_silently_dropped
    above): the parent still resolves, the rejected dependency is
    reported on stderr, not silently dropped or silently accepted."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/usedeprejectedpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/usedeprejectedpkg-1.0"]
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/useflagpkg"'
    )


def test_use_dep_enforcement_plain_flag_declared_and_enabled_matches(
    emerge_binary, fixture_env
):
    """dev-libs/useflagpkg's own IUSE="foo missingflag", with "foo"
    enabled globally by the fixture profile chain -- a top-level atom's
    own "[foo]" use-dep (declared, enabled) is genuinely satisfied, so
    this resolves exactly like the plain "dev-libs/useflagpkg" atom
    would (still recursing into its own foo?-gated RDEPEND)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg[foo]"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/useflagpkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_use_dep_enforcement_negated_flag_declared_but_enabled_does_not_match(
    emerge_binary, fixture_env
):
    """Same fixture as above, but "[-foo]": "foo" IS declared, but it's
    enabled, not disabled -- genuinely unsatisfied, so there's no visible
    candidate for this atom at all."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg[-foo]"], fixture_env
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".'
    )


def test_use_dep_enforcement_plain_flag_declared_but_disabled_does_not_match(
    emerge_binary, fixture_env
):
    """"missingflag" is declared in useflagpkg's own IUSE but never
    enabled anywhere in the fixture profile chain -- "[missingflag]"
    (must be enabled) is genuinely unsatisfied."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg[missingflag]"], fixture_env
    )
    assert result.returncode == 1
    assert result.stdout == ""


def test_use_dep_enforcement_negated_flag_declared_and_disabled_matches(
    emerge_binary, fixture_env
):
    """Same fixture, "[-missingflag]": declared and disabled -- the
    negated form's own requirement is genuinely satisfied."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/useflagpkg[-missingflag]"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/useflagpkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_use_dep_enforcement_undeclared_flag_with_no_default_never_matches(
    emerge_binary, fixture_env
):
    """Real _use_dep.required (see use_deps_satisfied's own doc comment):
    a use-dep flag with no (+)/(-) default marker must be a real,
    declared IUSE flag on the candidate, or the atom simply doesn't match
    at all -- "nonexistentflag" isn't in useflagpkg's own IUSE anywhere,
    and has no default here, so this is unsatisfiable regardless of
    enabled/disabled state."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/useflagpkg[nonexistentflag]"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""


def test_use_dep_enforcement_plus_default_rescues_an_undeclared_flag(
    emerge_binary, fixture_env
):
    """Same undeclared "nonexistentflag" as above, but with a "(+)"
    default this time -- missing from IUSE no longer disqualifies it,
    the default stands in for "as if enabled", satisfying the plain
    (must-be-enabled) form."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/useflagpkg[nonexistentflag(+)]"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/useflagpkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_required_use_satisfied_resolves_normally(emerge_binary, fixture_env):
    """dev-libs/requireduseokpkg's own REQUIRED_USE is "foo? ( bar )" --
    "foo" is enabled globally by the fixture profile chain, and "bar" is
    forced on by this package's own package.use entry, so the
    use-conditional group is genuinely satisfied (its own real
    check_required_use, PMS 7.3.4/8.2) -- resolves exactly like any
    other New package, no different treatment at all."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/requireduseokpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N] dev-libs/requireduseokpkg-1.0\n"


def test_required_use_violated_top_level_aborts_the_whole_run(emerge_binary, fixture_env):
    """dev-libs/requiredusebadpkg's own REQUIRED_USE is "foo? ( bar )" --
    "foo" is enabled globally, but "bar" is never forced on for this
    package, so the use-conditional group is genuinely violated. Real
    depgraph.py's own REQUIRED_USE check happens right after package
    selection and, on failure, aborts the whole run (NOTE comment in
    depgraph.py: "REQUIRED_USE checks are delayed until after package
    selection") -- a materially different severity than a merely
    unresolvable dependency (report, don't fail): here nothing is
    printed to stdout at all, and the run exits nonzero."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/requiredusebadpkg"], fixture_env
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: '
        '"foo? ( bar )"'
    )


def test_required_use_violated_dependency_still_aborts_the_whole_run(
    emerge_binary, fixture_env
):
    """dev-libs/requiredusebadparentpkg RDEPENDs on dev-libs/requiredusebadpkg
    (see the top-level REQUIRED_USE violation test above) -- proving the
    same fatal-abort severity applies regardless of whether the
    violating package was reached as a top-level atom or a dependency
    deep in the graph, unlike a dependency's own NoVisibleCandidate."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/requiredusebadparentpkg"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: '
        '"foo? ( bar )"'
    )


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


def test_use_expand_variable_drives_a_dependency(emerge_binary, fixture_env):
    """PORTING/fixtures/repo/profiles/base/make.defaults declares
    USE_EXPAND="VIDEO_CARDS" and VIDEO_CARDS="nvidia" -- real config.py's
    own USE_EXPAND mechanism (PMS 7.3.4) expands that into the pseudo-USE
    flag "video_cards_nvidia", added to the global USE set exactly like
    an ordinary profile-declared flag would be. dev-libs/useexpandpkg's
    own `video_cards_nvidia? ( dev-libs/newpkg )` proves the expanded
    flag genuinely drives dependency recursion, not just USE display;
    `video_cards_amdgpu` (never set by anything) stays off, so its own
    `? ( dev-libs/hiddendep )` clause is never pulled in."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/useexpandpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N] dev-libs/useexpandpkg-1.0  USE="-video_cards_amdgpu video_cards_nvidia"',
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]
    assert "hiddendep" not in result.stdout


def test_package_use_expand_prefix_shorthand_drives_a_dependency(emerge_binary, fixture_env):
    """PORTING/fixtures/etc/portage/package.use has "dev-libs/
    packageuseexpandpkg PYTHON_TARGETS: python3_12" -- real
    UseManager._parse_user_files_to_extatomdict's own shorthand syntax
    (PMS-adjacent, user-level package.use only -- see
    parse_package_use_lines's own doc comment, portage-profile) expands
    that into "python_targets_python3_12" exactly as if it had been
    written out in full, gating dev-libs/packageuseexpandpkg's own
    RDEPEND."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "dev-libs/packageuseexpandpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N] dev-libs/packageuseexpandpkg-1.0  USE="python_targets_python3_12"',
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_use_stable_force_and_package_use_stable_mask_apply_when_stable(
    emerge_binary, fixture_env
):
    """dev-libs/stableusepkg's own KEYWORDS="amd64" (no "~") is genuinely
    stable under real KeywordsManager.isStable (PMS-adjacent; see
    portage-repo's own is_stable doc comment): converting it to "~amd64"
    would fall outside the fixture's own ACCEPT_KEYWORDS="amd64", so
    portage-repo's own is_stable check is True. use.stable.force (global,
    profiles/base/use.stable.force) forces "stableforceflag" on, pulling
    in its own RDEPEND; package.use.stable.mask (repo-level) masks
    "maskflag" back off even though PORTING/fixtures/etc/portage/
    package.use enables it first -- proving package.use.stable.mask wins
    over package.use, but only for a genuinely stable candidate."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/stableusepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N] dev-libs/stableusepkg-1.0  USE="-maskflag stableforceflag"',
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_use_stable_force_and_package_use_stable_mask_skip_an_unstable_candidate(
    emerge_binary, fixture_env
):
    """dev-libs/unstableusepkg's own KEYWORDS="~amd64" is genuinely NOT
    stable (its own already-"~"-prefixed keyword is unchanged by
    is_stable's own re-unstabilization, so it stays visible either way)
    -- same use.stable.force/package.use are in play as
    dev-libs/stableusepkg's own test above, but neither
    use.stable.force nor package.use.stable.mask apply here at all:
    "stableforceflag" stays off (no dependency pulled in), and
    "maskflag" -- enabled by the same package.use entry stableusepkg's
    own test uses -- stays on, unmasked."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/unstableusepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == (
        '[ebuild  N] dev-libs/unstableusepkg-1.0  USE="maskflag -stableforceflag"\n'
    )


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


def test_repo_level_package_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/repo/profiles/package.use (the main repo's own
    repo-level package.use) has a "dev-libs/repouseenablepkg
    repouseflag" entry -- "repouseflag" is off everywhere else, so its
    own repouseflag?-gated dependency is pulled in only because this
    repo-level source is now stacked in, proving package.use is no
    longer user-level only."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/repouseenablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/repouseenablepkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_profile_level_package_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/repo/profiles/default/package.use (the leaf
    profile's own package.use) has a "dev-libs/profileuseenablepkg
    profileuseflag" entry -- same proof as the repo-level case above, for
    the profile-chain source instead."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/profileuseenablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/profileuseenablepkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_package_use_mask_and_force_with_atom_specificity_ordering(emerge_binary, fixture_env):
    """dev-libs/pkgusemaskforcepkg's own IUSE="forceflag maskflag specflag":
    repo-level package.use.force force-enables "forceflag" via a bare
    wildcard entry; the base profile level's own package.use.mask masks
    both "maskflag" and "specflag" via a bare atom; the leaf profile's
    own package.use.mask has a MORE SPECIFIC exact-version atom
    ("=dev-libs/pkgusemaskforcepkg-1.0 -specflag") that un-masks
    "specflag" again -- proving atom-specificity ordering (not just
    profile-chain order) decides which entry wins, and that a
    less-specific entry from an EARLIER profile level can still be
    overridden by a more-specific one from a LATER level. Final USE:
    forceflag on (forced), maskflag off (masked, nothing un-masks it),
    specflag off (un-masked but not enabled by anything else, so stays
    at its off-by-default)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/pkgusemaskforcepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == (
        '[ebuild  N] dev-libs/pkgusemaskforcepkg-1.0  USE="forceflag -maskflag -specflag"\n'
    )


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
        "implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, "
        "--onlydeps/-o, --update/-u, --deep/-D, and --help/-h are implemented so far; see PROMPT.md)"
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


def test_help_prints_a_pilot_specific_summary_not_real_emerges_own(
    emerge_binary, fixture_env
):
    """--help/-h is real and implemented, but the text is a short,
    honest, pilot-specific summary -- not a port of real emerge's own
    _emerge/help.py (157 lines of colorized usage syntax for its full
    ~130-flag surface, most of which this pilot doesn't implement).
    Pinned in full since it's this pilot's own content, not derived from
    real emerge's own output."""
    result = _run([str(emerge_binary)], ["--help"], fixture_env)
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout == (
        "emerge (pilot v1): command-line interface to the Rust porting pilot\n"
        "\n"
        "Usage:\n"
        "   emerge --pretend [--verbose] <atom> [<atom> ...]\n"
        "   emerge --help\n"
        "\n"
        "Options:\n"
        "   -p, --pretend   required: the only real merge calculation this pilot implements\n"
        '   -v, --verbose   show USE="..." on each [ebuild ...] line (optionally: -v y|n)\n'
        "   -N, --newuse    reinstall an already-installed package if its USE has changed\n"
        "   -U, --changed-use  like -N, but ignores newly added/removed IUSE flags entirely\n"
        "   -O, --nodeps    do not resolve or show any dependency, only the given atoms\n"
        "   -o, --onlydeps  show only the given atoms' dependencies, not the atoms themselves\n"
        "   -u, --update    upgrade to a newer visible version even if the installed one satisfies the atom\n"
        "   -D, --deep[=N]  also recurse into an already-installed package's own dependencies (optionally, only N levels deep)\n"
        "   -h, --help      show this message and exit\n"
        "\n"
        "Every other real emerge option/action is recognized by name (see "
        "lib/_emerge/main.py) but not implemented -- using one reports which "
        "option or action it is, instead of a generic error.\n"
        "See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.\n"
    )


def test_help_wins_unconditionally_regardless_of_other_flags_or_position(
    emerge_binary, fixture_env
):
    """Matches real emerge's own behavior: main.py's parse_opts maps
    -h/--help to the "help" action, and main() checks
    "if myaction == 'help'" once, after the whole line has already
    parsed successfully -- so help wins no matter where it appears or
    what else (valid but unimplemented) accompanies it. "-h" bundled
    with another short flag must win the same way a standalone "-h"
    does, and --help must win even with no --pretend/atom at all."""
    for args in (
        ["--jobs", "--help"],
        ["-ph"],
        ["--help", "dev-libs/newpkg"],
    ):
        result = _run([str(emerge_binary)], args, fixture_env)
        assert result.returncode == 0, args
        assert result.stdout.startswith(
            "emerge (pilot v1): command-line interface to the Rust porting pilot"
        ), args


def test_world_expands_to_the_fixture_world_files_own_atoms(emerge_binary, fixture_env):
    """PORTING/fixtures/var/lib/portage/world (real portage's own
    WORLD_FILE, <ROOT>/var/lib/portage/world) lists dev-libs/newpkg and
    dev-libs/withdeps (which itself recurses into newpkg again -- deduped
    -- and upgradepkg), plus a "@some-nested-set-reference" line that
    must be silently skipped, not mishandled -- proving @world expansion
    feeds the exact same multi-atom/recursion machinery every other
    invocation already uses, not a separate code path. --update is added
    purely so upgradepkg's own dependency-level entry actually upgrades
    (see the --update contract tests) rather than staying silently
    AlreadyInstalled -- unrelated to what this test itself is about."""
    result = _run([str(emerge_binary)], ["--pretend", "--update", "@world"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/withdeps-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_world_combines_with_an_explicit_atom(emerge_binary, fixture_env):
    """@world can appear alongside an explicit atom in the same
    invocation, expanding in place at whatever position it's given --
    real portage's own most common combined usage shape. --update is
    added for the same reason as the plain @world test above."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "dev-libs/samepkg", "@world"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "dev-libs/samepkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/withdeps-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_world_missing_file_expands_to_nothing_not_an_error(
    emerge_binary, fixture_env, tmp_path
):
    """A missing WORLD_FILE (e.g. a fresh ROOT that's never had anything
    merged into it) is a real, valid state, not a mistake -- @world
    expands to an empty list, which then hits the same "nothing to
    resolve" error an empty target list from any other source would,
    not a crash or a silent no-op. PORTAGE_CONFIGROOT stays pointed at
    the real fixtures (for a valid repos.conf/profile); only ROOT is
    redirected to an empty tmp_path with no var/lib/portage/world at
    all."""
    env = dict(fixture_env)
    env["ROOT"] = str(tmp_path)
    result = _run([str(emerge_binary)], ["--pretend", "@world"], env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == "emerge (pilot v1): no package atoms to resolve (the target list, "
        "after expanding any @world/@system, is empty)"
    )


def test_system_expands_to_the_fixture_profile_chains_own_packages_files(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/repo/profiles/base/packages contributes
    dev-libs/newpkg (plus a non-"*"-prefixed "hint" line that must never
    contribute an atom of its own), and PORTING/fixtures/repo/profiles/
    default/packages (the leaf) contributes dev-libs/withdeps -- proving
    @system stacks across multiple profile levels (not just the leaf,
    real PackagesSystemSet's own behavior) and that its expanded atoms
    feed the exact same multi-atom/recursion machinery every other
    invocation already uses: withdeps' own RDEPEND recurses into newpkg
    again (deduped against base's own @system entry) and upgradepkg.
    --update is added purely so upgradepkg's own dependency-level entry
    actually upgrades (see the --update contract tests) rather than
    staying silently AlreadyInstalled -- unrelated to what this test
    itself is about."""
    result = _run([str(emerge_binary)], ["--pretend", "--update", "@system"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/withdeps-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_system_combines_with_an_explicit_atom(emerge_binary, fixture_env):
    """@system can appear alongside an explicit atom in the same
    invocation, expanding in place at whatever position it's given, same
    as @world. --update is added for the same reason as the plain
    @system test above."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "dev-libs/samepkg", "@system"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "dev-libs/samepkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/withdeps-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_some_other_set_as_a_top_level_atom_is_not_implemented(emerge_binary, fixture_env):
    """Only the literal tokens "@world" and "@system" trigger set
    expansion -- any other "@"-prefixed top-level target (a nested set
    reference real portage's own world file/packages files can contain,
    but this pilot doesn't resolve -- see read_world_atoms's and
    resolve_config's own doc comments) falls through to the ordinary
    atom-parsing path and gets a clear "invalid atom" error, not a
    silent no-op."""
    result = _run([str(emerge_binary)], ["--pretend", "@some-other-set"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == 'emerge: invalid atom "@some-other-set"'


def test_newuse_reinstalls_a_package_whose_use_changed(emerge_binary, fixture_env):
    """PORTING/fixtures/var/db/pkg/dev-libs/reinstallpkg-1.0 is installed
    with IUSE="foo" but an empty vdb USE file (foo was off at merge
    time); the fixture profile chain enables "foo" globally now, so
    --newuse must report a Reinstall for the changed "foo" flag -- and,
    since reinstallpkg RDEPENDs on dev-libs/newpkg, still recurse into
    its own dependencies exactly like a New/Upgrade entry would."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--newuse", "dev-libs/reinstallpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_newuse_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-N is --newuse's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pN") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pN", "dev-libs/reinstallpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_newuse_verbose_shows_use_flags_too(emerge_binary, fixture_env):
    """-v combines with -N exactly like it does with New/Upgrade: shows
    this package's own IUSE-declared flags, alphabetically sorted."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-N", "-v", "dev-libs/reinstallpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)  USE="foo"',
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_without_newuse_a_use_changed_package_stays_already_installed(
    emerge_binary, fixture_env
):
    """The exact same fixture as the Reinstall test above, but without
    --newuse: the USE mismatch is real, but nothing checks for it unless
    --newuse is given, so this must stay the pre-existing
    AlreadyInstalled outcome -- not a Reinstall, and not a NEW dependency
    recursion into dev-libs/newpkg either."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/reinstallpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "dev-libs/reinstallpkg-1.0 is already installed; nothing to do\n"


def test_newuse_is_a_noop_when_use_has_not_changed(emerge_binary, fixture_env):
    """dev-libs/samepkg has no IUSE at all (declared or in the vdb), so
    there's nothing for --newuse to detect a change in -- must stay
    AlreadyInstalled even with --newuse enabled, proving --newuse doesn't
    force a reinstall of every already-installed package."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--newuse", "dev-libs/samepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/samepkg-1.0 is already installed; nothing to do\n"


def test_newuse_forced_flags_suppresses_a_spurious_reinstall(emerge_binary, fixture_env):
    """dev-libs/usemaskreinstallpkg is installed with an empty vdb IUSE,
    but its own ebuild now declares IUSE="masked_newly_added_flag" -- a
    flag PORTING/fixtures/repo/profiles/base/use.mask masks off, so it
    was never enabled either before or after. Real depgraph.py's own
    "flags -= forced_flags" line exists exactly to stop a newly-declared,
    permanently-masked (or forced) IUSE flag from spuriously triggering a
    reinstall just because it now exists -- without it, this would
    incorrectly report a Reinstall for "masked_newly_added_flag"."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--newuse", "dev-libs/usemaskreinstallpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/usemaskreinstallpkg-1.0 is already installed; nothing to do\n"


def test_newuse_vs_changed_use_diverge_on_a_newly_added_iuse_flag(emerge_binary, fixture_env):
    """dev-libs/changedusepkg is installed with an empty vdb IUSE, and its
    current ebuild now declares IUSE="brandnewflag" -- a real, unmasked,
    not-globally-enabled flag (unlike usemaskreinstallpkg's own masked
    one above). --newuse's own presence-diff term reacts to a flag
    simply existing in IUSE now when it didn't before, regardless of
    enablement, so it reports a Reinstall; --changed-use's own, narrower
    formula never even looks at IUSE presence, only at enablement of
    flags declared on both sides, so it correctly sees nothing changed
    and stays AlreadyInstalled -- proving the two flags are genuinely
    different checks, not two names for the same one."""
    newuse_result = _run(
        [str(emerge_binary)], ["--pretend", "--newuse", "dev-libs/changedusepkg"], fixture_env
    )
    assert newuse_result.returncode == 0
    assert newuse_result.stdout == (
        "[ebuild  r] dev-libs/changedusepkg-1.0 (reinstall for changed USE: brandnewflag)\n"
    )

    changed_use_result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-use", "dev-libs/changedusepkg"],
        fixture_env,
    )
    assert changed_use_result.returncode == 0
    assert changed_use_result.stdout == (
        "dev-libs/changedusepkg-1.0 is already installed; nothing to do\n"
    )


def test_changed_use_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-U is --changed-use's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pU") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pU", "dev-libs/changedusepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "dev-libs/changedusepkg-1.0 is already installed; nothing to do\n"


def test_changed_use_still_catches_an_enablement_change_on_a_shared_flag(
    emerge_binary, fixture_env
):
    """dev-libs/reinstallpkg's own "foo" flag exists in IUSE on both
    sides (installed and current) -- only its enablement changed. This
    is exactly the shared term both --newuse and --changed-use compute
    the same way, so --changed-use must catch it too, not just
    --newuse."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--changed-use", "dev-libs/reinstallpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_nodeps_disables_recursion_entirely(emerge_binary, fixture_env):
    """dev-libs/withdeps RDEPENDs on dev-libs/newpkg and
    dev-libs/upgradepkg -- see the plain recursion contract test above.
    With --nodeps, real create_depgraph_params.py pops
    "recurse" out of myparams, so depgraph.py's own dependency walk
    returns early -- ported here as never even reading withdeps' own
    DEPEND/RDEPEND, so neither dependency appears at all."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--nodeps", "dev-libs/withdeps"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N] dev-libs/withdeps-1.0\n"


def test_nodeps_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-O is --nodeps's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pO") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pO", "dev-libs/withdeps"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N] dev-libs/withdeps-1.0\n"


def test_nodeps_still_shows_the_top_level_atoms_own_use_display(emerge_binary, fixture_env):
    """Real portage's own -v USE display is about a package's own
    metadata, unrelated to whether its dependencies get walked --
    --nodeps must not blank it out, even though it suppresses the
    foo?-gated dev-libs/newpkg dependency this same package's -v output
    (see the plain --verbose contract test) normally pulls in."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-O", "-v", "dev-libs/useflagpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == '[ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"\n'


def test_onlydeps_suppresses_the_top_level_atom_but_shows_its_dependencies(
    emerge_binary, fixture_env
):
    """man/emerge.1: "Only merge (or pretend to merge) the dependencies
    of the packages specified, not the packages themselves." dev-libs/
    withdeps RDEPENDs on dev-libs/newpkg (New) and dev-libs/upgradepkg
    (Upgrade, with --update -- see the --update contract tests for why
    it's needed here at all) -- --onlydeps must print both dependency
    lines exactly as always, but never withdeps' own [ebuild N] line --
    the exact inverse of what --nodeps (see above) suppresses."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--onlydeps", "dev-libs/withdeps"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_onlydeps_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-o is --onlydeps's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-po") it must behave identically to
    the long-flag invocation above. -u (--update) is bundled in too, for
    the same reason the long-flag invocation above needs --update."""
    result = _run([str(emerge_binary)], ["-pou", "dev-libs/withdeps"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_onlydeps_on_an_already_installed_atom_prints_nothing(emerge_binary, fixture_env):
    """dev-libs/samepkg is already installed, so it has no dependencies
    ever walked regardless of --onlydeps (unaffected: an AlreadyInstalled
    package's own dependencies are already presumed satisfied, same as
    without --onlydeps) -- and --onlydeps suppresses its own "already
    installed; nothing to do" line too, so the whole run prints nothing
    at all, distinct from a genuine no-op."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--onlydeps", "dev-libs/samepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == ""


def test_without_update_an_installed_version_that_satisfies_the_atom_is_kept(
    emerge_binary, fixture_env
):
    """dev-libs/upgradepkg is installed at 1.0; a newer 2.0 is visible in
    the tree too. Real depgraph.py's own `avoid_update` (lines 7814 and
    8448 of lib/_emerge/depgraph.py) means plain `emerge
    dev-libs/upgradepkg`, with no --update, never even looks for a
    better version -- real emerge does NOT offer an upgrade just because
    a newer version exists. Before this slice, this pilot's own default
    behavior always searched for and offered the best available version,
    which -- while a real, working piece of dependency resolution -- was
    not actually what real emerge does by default."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"


def test_update_upgrades_to_the_newer_visible_version(emerge_binary, fixture_env):
    """Same fixture as above, but with --update: now a real Upgrade,
    matching real depgraph.py's own `dont_miss_updates` branch."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--update", "dev-libs/upgradepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)\n"


def test_update_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-u is --update's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pu") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pu", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)\n"


def test_update_threads_through_dependency_recursion_not_just_top_level(
    emerge_binary, fixture_env
):
    """dev-libs/upgradepkg is reached only as a *dependency* of
    dev-libs/withdeps here, never a top-level atom -- --update must
    still upgrade it, proving the flag threads uniformly through the
    whole BFS (see resolve_pretend_graph's own doc comment), not just a
    top-level atom. Without --update (see test_nodeps_disables_recursion_entirely's
    sibling tests above), this same dependency stays silently
    AlreadyInstalled instead."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--update", "dev-libs/withdeps"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/withdeps-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)",
    ]


def test_without_deep_an_already_installed_packages_own_deps_stay_unwalked(
    emerge_binary, fixture_env
):
    """dev-libs/deeppkg is already installed and RDEPENDs on
    dev-libs/deeppkg2 (also already installed), which itself RDEPENDs on
    dev-libs/newpkg (New) -- without --deep, real portage's own default
    (deep=0) never walks an already-installed package's own further
    dependencies, at any depth, so neither deeppkg2 nor newpkg ever
    appears here, only deeppkg's own top-level "nothing to do" line."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/deeppkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"


def test_deep_walks_the_whole_already_installed_chain(emerge_binary, fixture_env):
    """Same fixture as above, but with a bare --deep (unlimited depth,
    real myoptions.deep is True): both already-installed steps
    (deeppkg -> deeppkg2) get walked, reaching deeppkg2's own RDEPEND on
    newpkg (New) -- deeppkg2 itself stays silent (AlreadyInstalled, not
    a top-level atom, same "don't clutter the list" rule as ever), but
    newpkg's own [ebuild N] line now appears."""
    result = _run([str(emerge_binary)], ["--pretend", "--deep", "dev-libs/deeppkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "dev-libs/deeppkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_deep_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-D is --deep's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pD") it must behave identically to
    the long-flag invocation above, never consuming a following token as
    its own value -- same "no ambiguity with another bundled flag
    character" rule already established for a bundled -v."""
    result = _run([str(emerge_binary)], ["-pD", "dev-libs/deeppkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "dev-libs/deeppkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_deep_bounded_depth_stops_short_of_the_full_chain(emerge_binary, fixture_env):
    """--deep=1: deeppkg (depth 0) recurses since 0 < 1, discovering
    deeppkg2 at depth 1 -- but deeppkg2 itself does NOT recurse (1 < 1
    is false), so newpkg is never reached, and the output is identical
    to not passing --deep at all. --deep=2 (one level deeper) reaches
    all the way to newpkg, same as unlimited -- proving the bound is
    real and not silently ignored in either direction."""
    bounded_one = _run(
        [str(emerge_binary)], ["--pretend", "--deep=1", "dev-libs/deeppkg"], fixture_env
    )
    assert bounded_one.returncode == 0
    assert bounded_one.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"

    bounded_two = _run(
        [str(emerge_binary)], ["--pretend", "--deep=2", "dev-libs/deeppkg"], fixture_env
    )
    assert bounded_two.returncode == 0
    assert bounded_two.stdout.splitlines() == [
        "dev-libs/deeppkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_deep_equals_zero_matches_not_passing_deep_at_all(emerge_binary, fixture_env):
    """--deep=0 parses fine (unlike a negative value) but is
    indistinguishable from --deep never being given at all, matching
    real create_depgraph_params.py's own `!= 0` check that excludes it
    from myparams either way."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--deep=0", "dev-libs/deeppkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"


def test_deep_rejects_a_negative_inline_value(emerge_binary, fixture_env):
    """--deep=N is argparse's own native "="-form -- a non-numeric or
    negative value there is a real, immediate parse error (matching real
    parser.error("Invalid --deep parameter: ...")), unlike a negative
    *next token*, which the pre-processor never even consumes as --deep's
    own value in the first place (real valid_integers rejects it), so it
    falls through as a separate, likely-invalid token instead."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--deep=-1", "dev-libs/deeppkg"], fixture_env
    )
    assert result.returncode == 2
    assert result.stdout == ""
    assert result.stderr.strip() == 'emerge: invalid --deep parameter: "-1"'


def test_deep_is_ignored_when_nodeps_disables_the_dependency_walk_entirely(
    emerge_binary, fixture_env
):
    """--nodeps trumps --deep -- real create_depgraph_params.py pops
    "recurse" out of myparams outright, which the dependency walk itself
    checks for before `deep` is ever consulted."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deep", "--nodeps", "dev-libs/deeppkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"


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
    """--jobs is a real emerge option (see lib/_emerge/main.py's
    argument_options) this pilot doesn't implement -- the error must
    name it specifically and say "option", distinct from both a
    genuinely unrecognized flag and an unimplemented action."""
    result = _run([str(emerge_binary)], ["--jobs", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge (pilot v1): option "--jobs" is a real emerge option, but is not '
        "implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, "
        "--onlydeps/-o, --update/-u, --deep/-D, and --help/-h are implemented so far; see PROMPT.md)"
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
        "implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, "
        "--onlydeps/-o, --update/-u, --deep/-D, and --help/-h are implemented so far; see PROMPT.md)"
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
        "implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, "
        "--onlydeps/-o, --update/-u, --deep/-D, and --help/-h are implemented so far; see PROMPT.md)"
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
