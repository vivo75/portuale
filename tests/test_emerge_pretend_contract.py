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
`emerge` binary (portuale, dispatched via a real symlink
-- not a neutral harness, since emerge is an actual product surface per
PROMPT.md's testing decision) and the Python reference implementation
identically, against the synthetic fixture tree at PORTING/fixtures
(whose repos.conf/make.profile/make.conf/package.mask/package.unmask/
package.accept_keywords/package.use now drive real config resolution,
not hardcoded values, and whose repos.conf now defines a second,
higher-priority overlay repo alongside the main one), and asserts their
stdout, stderr, and exit codes all match exactly.
"""

import json
import subprocess

import pytest

# (description, args, expected_exit_code) -- exit codes: 0 success,
# 1 resolution/parse error, 2 CLI-usage error (mirrors both sides' shared
# convention, not real emerge's own exit codes).
CASES = [
    ("new install", ["--pretend", "dev-libs/newpkg"], 0),
    ("already installed", ["--pretend", "dev-libs/samepkg"], 0),
    (
        "without --update, a bare top-level atom still offers a newer visible version",
        ["--pretend", "dev-libs/upgradepkg"],
        0,
    ),
    (
        "--noreplace restores the real avoid_update shortcut without --update",
        ["--pretend", "--noreplace", "dev-libs/upgradepkg"],
        0,
    ),
    ("-n short alias for --noreplace", ["--pretend", "-n", "dev-libs/samepkg"], 0),
    ("-n bundled with -p", ["-pn", "dev-libs/samepkg"], 0),
    ("--selective bare form, same as --noreplace", ["--pretend", "--selective", "dev-libs/samepkg"], 0),
    ("--selective=y inline form", ["--pretend", "--selective=y", "dev-libs/samepkg"], 0),
    (
        "--selective n explicitly cancels it even with --update also given",
        ["--pretend", "--update", "--selective", "n", "dev-libs/upgradepkg"],
        0,
    ),
    (
        "--update alone still lets a no-newer-version package stay already installed",
        ["--pretend", "--update", "dev-libs/samepkg"],
        0,
    ),
    (
        "--update --selective=n forces a bare reinstall even with nothing newer available",
        ["--pretend", "--update", "--selective=n", "dev-libs/samepkg"],
        0,
    ),
    (
        "--selective=n inline form cancels --noreplace too",
        ["--pretend", "--noreplace", "--selective=n", "dev-libs/samepkg"],
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
    ("--exclude: leaves an installed package alone despite --update", ["--pretend", "--update", "--exclude", "dev-libs/upgradepkg", "dev-libs/upgradepkg"], 0),
    ("-X short alias for --exclude", ["--pretend", "--update", "-X", "dev-libs/upgradepkg", "dev-libs/upgradepkg"], 0),
    ("--exclude=ATOM inline form", ["--pretend", "--update", "--exclude=dev-libs/upgradepkg", "dev-libs/upgradepkg"], 0),
    ("--exclude prevents a not-yet-installed package from being offered", ["--pretend", "--exclude", "dev-libs/newpkg", "dev-libs/newpkg"], 1),
    ("--exclude with no argument is a real, immediate usage error", ["--pretend", "--exclude"], 2),
    ("-X bundled with other short flags is not supported", ["-pX", "dev-libs/upgradepkg"], 2),
    ("--json: new install", ["--pretend", "--json", "dev-libs/newpkg"], 0),
    ("--json: with --verbose, includes use_flags", ["--pretend", "-v", "--json", "dev-libs/useflagpkg"], 0),
    ("--json: diamond dependency, required_by lists both owners", ["--pretend", "--json", "dev-libs/diamond"], 0),
    ("--json: upgrade includes from_version", ["--pretend", "--update", "--json", "dev-libs/upgradepkg"], 0),
    ("--json: blocker match", ["--pretend", "--json", "dev-libs/blockerpkg"], 0),
    ("--json: slot conflict", ["--pretend", "--json", "dev-libs/slotconflictparent"], 0),
    ("--json: combined with --deep", ["--pretend", "--update", "--deep", "--json", "dev-libs/deeppkg"], 0),
    (
        "--json: provenance records a mask cancelled by a matching unmask",
        ["--pretend", "--json", "dev-libs/maskedandunmaskedpkg"],
        0,
    ),
    (
        "--json: provenance records the package.accept_keywords entry actually needed",
        ["--pretend", "--json", "dev-libs/wildcardkeywordpkg"],
        0,
    ),
    ("only ~keyword, not visible", ["--pretend", "dev-libs/maskedpkg"], 1),
    ("package does not exist", ["--pretend", "dev-libs/does-not-exist"], 1),
    ("LICENSE in @EULA group, masked by the real default ACCEPT_LICENSE", ["--pretend", "dev-libs/eulapkg"], 1),
    ("LICENSE || any-of group, visible via the accepted alternative", ["--pretend", "dev-libs/anyoflicensepkg"], 0),
    ("package.license unmasks an otherwise EULA-masked package", ["--pretend", "dev-libs/packagelicensepkg"], 0),
    ("cross-repo profile parent: overlay's own license_groups joins the chain", ["--pretend", "dev-libs/crossrepolicensepkg"], 1),
    ("USE-conditional LICENSE, visible with the flag off", ["--pretend", "dev-libs/uselicensepkg"], 0),
    ("USE-conditional LICENSE, masked once package.use forces the flag on", ["--pretend", "dev-libs/uselicensepkgforced"], 1),
    ("PROPERTIES visible under the real default ACCEPT_PROPERTIES=*", ["--pretend", "dev-libs/propertiespkg"], 0),
    ("package.properties narrows acceptance for one package", ["--pretend", "dev-libs/interactivepkg"], 1),
    ("package.accept_restrict narrows acceptance for one package", ["--pretend", "dev-libs/restrictedpkg"], 1),
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
    (
        "recursion: any-of group resolves only the first satisfiable alternative",
        ["--pretend", "dev-libs/anyof"],
        0,
    ),
    (
        "recursion: any-of group falls back to every alternative when none is satisfiable",
        ["--pretend", "dev-libs/anyofunresolvable"],
        0,
    ),
    ("recursion: unresolvable dep doesn't fail the graph", ["--pretend", "dev-libs/missingdep"], 0),
    ("recursion: dedup across DEPEND and RDEPEND", ["--pretend", "dev-libs/dualdep"], 0),
    ("recursion: BDEPEND is walked", ["--pretend", "dev-libs/bdependpkg"], 0),
    ("recursion: PDEPEND is walked", ["--pretend", "dev-libs/pdependpkg"], 0),
    ("recursion: IDEPEND is walked", ["--pretend", "dev-libs/idependpkg"], 0),
    ("recursion: slot-operator dependency atoms are resolved, not dropped", ["--pretend", "dev-libs/slotoperatorpkg"], 0),
    ("recursion: a sub-slot-restricted dependency atom actually matches", ["--pretend", "dev-libs/subslotconsumer"], 0),
    (
        "recursion: a sub-slot-restricted dependency atom genuinely rejects a mismatch",
        ["--pretend", "dev-libs/subslotmismatchconsumer"],
        0,
    ),
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
        "IUSE +/- defaults: satisfy REQUIRED_USE and show correctly in -v, unmentioned by any other USE source",
        ["--pretend", "-v", "dev-libs/iusedefaultpkg"],
        0,
    ),
    (
        "REQUIRED_USE: violated on a dependency, still aborts the whole run",
        ["--pretend", "dev-libs/requiredusebadparentpkg"],
        1,
    ),
    (
        "REQUIRED_USE referencing an implicit (arch.list-only) IUSE flag resolves normally",
        ["--pretend", "-v", "dev-libs/archiuseimplicitpkg"],
        0,
    ),
    (
        "global use.force/use.mask win over a contradicting package.use entry",
        ["--pretend", "-v", "dev-libs/globalprecedencepkg"],
        0,
    ),
    (
        "a profile-level -flag genuinely cancels an IUSE +default",
        ["--pretend", "-v", "dev-libs/cancelledpkg"],
        0,
    ),
    (
        "REQUIRED_USE: two independent top-level violations both get reported, not just the first",
        ["--pretend", "dev-libs/requiredusebadpkg", "dev-libs/requiredusebadpkg2"],
        1,
    ),
    (
        "--autounmask: no keyword suggestion by default",
        ["--pretend", "dev-libs/autounmaskkeywordpkg"],
        1,
    ),
    (
        "--autounmask: keyword suggestion once explicitly enabled",
        ["--pretend", "--autounmask", "dev-libs/autounmaskkeywordpkg"],
        1,
    ),
    (
        "--autounmask: a dependency's own no-visible-candidate gets no suggestion by default",
        ["--pretend", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--autounmask: a dependency's own no-visible-candidate gets a suggestion once enabled",
        ["--pretend", "--autounmask", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--autounmask: dependency-level keyword suggestion also appears in --json",
        ["--pretend", "--autounmask", "--json", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--usepkg: a binary-only package is invisible without it",
        ["--pretend", "dev-libs/binaryonlypkg"],
        1,
    ),
    (
        "--usepkg: a binary-only package resolves once eligible",
        ["--pretend", "--usepkg", "dev-libs/binaryonlypkg"],
        0,
    ),
    (
        "--binpkg-respect-use: a USE-mismatched binary falls back to the ebuild",
        ["--pretend", "--usepkg", "dev-libs/binaryusemismatchpkg"],
        0,
    ),
    (
        "--usepkgonly: excludes ebuild-only packages entirely",
        ["--pretend", "--usepkgonly", "dev-libs/newpkg"],
        1,
    ),
    (
        "downgrade: installed version has no visible candidate of its own",
        ["--pretend", "dev-libs/downgradepkg"],
        0,
    ),
    (
        "downgrade: a keyword-masked-but-installed TOP-LEVEL atom still requires visibility",
        ["--pretend", "dev-libs/keywordmaskedpkg"],
        0,
    ),
    (
        "avoid_update: a keyword-masked-but-installed DEPENDENCY is kept, not downgraded",
        ["--pretend", "dev-libs/needskeywordmasked"],
        0,
    ),
    (
        "avoid_update: a keyword-masked-but-installed DEPENDENCY with a satisfied USE-dep is kept",
        ["--pretend", "dev-libs/needskeywordmaskeduse"],
        0,
    ),
    (
        "overlay package.use: an overlay-only package.use entry gates a dependency",
        ["--pretend", "dev-libs/overlayuseenablepkg"],
        0,
    ),
    (
        "overlay package.use.force: an overlay-only package.use.force entry forces a flag on",
        ["--pretend", "dev-libs/overlayuseforcepkg"],
        0,
    ),
    (
        "overlay package.use.mask: an overlay-only package.use.mask entry masks a default-on flag",
        ["--pretend", "dev-libs/overlayusemaskpkg"],
        0,
    ),
    (
        "--usepkg-exclude: rejects the only binary candidate for an atom",
        ["--pretend", "--usepkg", "--usepkg-exclude", "dev-libs/binaryonlypkg", "dev-libs/binaryonlypkg"],
        1,
    ),
    (
        "--usepkg-include: a non-matching include list rejects the only binary candidate",
        ["--pretend", "--usepkg", "--usepkg-include", "dev-libs/doesnotexist-anywhere", "dev-libs/binaryonlypkg"],
        1,
    ),
    (
        "--usepkg-include: a matching include list keeps the binary candidate eligible",
        ["--pretend", "--usepkg", "--usepkg-include", "dev-libs/binaryonlypkg", "dev-libs/binaryonlypkg"],
        0,
    ),
    (
        "--rebuilt-binaries: off by default, stays already-installed",
        ["--pretend", "--usepkg", "--selective", "dev-libs/rebuiltbinarypkg"],
        0,
    ),
    (
        "--rebuilt-binaries: a differing BUILD_TIME triggers a reinstall",
        ["--pretend", "--usepkg", "--selective", "--rebuilt-binaries", "dev-libs/rebuiltbinarypkg"],
        0,
    ),
    (
        "--rebuilt-binaries-timestamp: cutoff above the binary's own BUILD_TIME suppresses it",
        [
            "--pretend",
            "--usepkg",
            "--selective",
            "--rebuilt-binaries",
            "--rebuilt-binaries-timestamp",
            "3000",
            "dev-libs/rebuiltbinarypkg",
        ],
        0,
    ),
    (
        "--newrepo: off by default, stays already-installed",
        ["--pretend", "--selective", "dev-libs/newrepopkg"],
        0,
    ),
    (
        "--newrepo: a differing vdb repository triggers a reinstall",
        ["--pretend", "--selective", "--newrepo", "dev-libs/newrepopkg"],
        0,
    ),
    (
        "--newrepo: a matching vdb repository does not trigger a reinstall",
        ["--pretend", "--selective", "--newrepo", "dev-libs/samerepopkg"],
        0,
    ),
    (
        "--newrepo: a missing vdb repository file falls back to the __unknown__ sentinel",
        ["--pretend", "--selective", "--newrepo", "dev-libs/samepkg"],
        0,
    ),
    (
        "--buildpkgonly: off by default, a New->New dependency chain resolves fine",
        ["--pretend", "dev-libs/dualdep"],
        0,
    ),
    (
        "--buildpkgonly: a New package depending on another New package fails to resolve",
        ["--pretend", "--buildpkgonly", "dev-libs/dualdep"],
        1,
    ),
    (
        "--buildpkgonly: a New package depending on an already-installed one resolves fine",
        ["--pretend", "--buildpkgonly", "dev-libs/buildpkgonlysatisfied"],
        0,
    ),
    (
        "-B short alias for --buildpkgonly",
        ["--pretend", "-B", "dev-libs/dualdep"],
        1,
    ),
    (
        "opt= USE-dep: parent flag ON evaluates to [flag], matches the child's default-on flag",
        ["--pretend", "dev-libs/useeqparentonpkg"],
        0,
    ),
    (
        "opt= USE-dep: parent flag OFF evaluates to [-flag], mismatches the child's default-on flag",
        ["--pretend", "dev-libs/useeqparentoffpkg"],
        0,
    ),
    (
        "--tree: indents a diamond dependency, shown once",
        ["--pretend", "--tree", "dev-libs/diamond"],
        0,
    ),
    (
        "--tree --unordered-display: children in discovery order, not alphabetical",
        ["--pretend", "--tree", "--unordered-display", "dev-libs/treeorderpkg"],
        0,
    ),
    (
        "--columns: new install right-aligns the version into a fixed column",
        ["--pretend", "--columns", "dev-libs/newpkg"],
        0,
    ),
    (
        "--columns: upgrade shows both the new and old version, each in its own column",
        ["--pretend", "--update", "--columns", "dev-libs/upgradepkg"],
        0,
    ),
    (
        "--tree and --columns together is a usage error",
        ["--pretend", "--tree", "--columns", "dev-libs/newpkg"],
        2,
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
        "USE_EXPAND_UNPREFIXED: ARCH=amd64 contributes the bare flag amd64, gates a dependency",
        ["--pretend", "-v", "dev-libs/archusepkg"],
        0,
    ),
    (
        "-pv groups IUSE by USE_EXPAND variable (VIDEO_CARDS=\"...\")",
        ["--pretend", "-v", "dev-libs/useexpandpkg"],
        0,
    ),
    (
        "-pv omits a USE_EXPAND_HIDDEN group (CPU_FLAGS_X86)",
        ["--pretend", "-v", "dev-libs/hiddenexpandpkg"],
        0,
    ),
    (
        "-pv marks installed-vs-new USE changes (*/%) on an [ebuild U] line",
        ["--pretend", "-v", "--update", "dev-libs/upgradeusepkg"],
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
    ("package.accept_keywords: -amd64 revokes a globally-accepted keyword", ["--pretend", "dev-libs/keywordrevokedpkg"], 1),
    ("package.accept_keywords: \"*\" accepts any stable keyword", ["--pretend", "dev-libs/starkeywordpkg"], 0),
    ("package.accept_keywords: \"~*\" accepts any testing keyword", ["--pretend", "dev-libs/tildestarkeywordpkg"], 0),
    ("package.accept_keywords: bare atom implicitly grants ~arch", ["--pretend", "dev-libs/bareacceptkeywordspkg"], 0),
    ("-pv bracket mask marker: ~ for a testing keyword", ["--pretend", "-v", "dev-libs/bareacceptkeywordspkg"], 0),
    ("-pv bracket mask marker: * for a cross-arch keyword", ["--pretend", "-v", "dev-libs/tildestarkeywordpkg"], 0),
    ("-pv bracket mask marker: # for a masked-then-unmasked package", ["--pretend", "-v", "dev-libs/maskedandunmaskedpkg"], 0),
    ("package.use: wildcard entry enables a flag not on globally", ["--pretend", "dev-libs/packageuseenablepkg"], 0),
    ("package.use: entry disables a flag that is on globally", ["--pretend", "dev-libs/packageusedisablepkg"], 0),
    ("package.use: repo-level entry enables a flag not on globally", ["--pretend", "dev-libs/repouseenablepkg"], 0),
    ("package.use: profile-level entry enables a flag not on globally", ["--pretend", "dev-libs/profileuseenablepkg"], 0),
    ("blocker: strong (!!) blocker matches an installed package", ["--pretend", "dev-libs/blockerpkg"], 0),
    ("blocker: weak (!) blocker matches another new package in the graph", ["--pretend", "dev-libs/graphblockerparent"], 0),
    ("overlay: package exists only in the overlay repo", ["--pretend", "dev-libs/overlayonlypkg"], 0),
    ("overlay: best version wins across repos", ["--pretend", "dev-libs/overlaynewerpkg"], 0),
    ("overlay: same-version tie broken toward higher priority", ["--pretend", "dev-libs/overlaytiepkg"], 0),
    ("overlay: repo-level package.mask scoped to the overlay only", ["--pretend", "dev-libs/overlaymaskedpkg"], 0),
    ("overlay: explicit ::overlay atom still hits the overlay's own mask", ["--pretend", "dev-libs/overlaymaskedpkg::overlay"], 1),
    ("overlay: explicit ::testrepo atom bypasses the overlay's own mask", ["--pretend", "dev-libs/overlaymaskedpkg::testrepo"], 0),
    ("overlay: repo-level package.unmask cancels the same overlay's own mask", ["--pretend", "dev-libs/overlaymaskedthenunmaskedpkg"], 0),
    ("overlay: implicit masters inherits the main repo's own package.mask", ["--pretend", "dev-libs/mastermaskedpkg"], 1),
    ("overlay: package.unmask cancels a masters-inherited mask", ["--pretend", "dev-libs/mastermaskedthenoverlayunmaskedpkg"], 0),
    ("repos.conf explicit masters=: does not inherit the main repo's mask", ["--pretend", "dev-libs/independentmastermainonlypkg"], 0),
    ("repos.conf explicit masters=: inherits a non-main declared master's mask", ["--pretend", "dev-libs/independentmasteroverlaypkg"], 1),
    ("layout.conf masters= middle tier + repo-name= override", ["--pretend", "dev-libs/layoutmasterpkg"], 1),
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
    (
        "--with-bdeps default (auto/y): --deep walks DEPEND/BDEPEND of an already-installed package",
        ["--pretend", "--deep", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "--with-bdeps=n: --deep skips DEPEND/BDEPEND but still walks RDEPEND",
        ["--pretend", "--deep", "--with-bdeps", "n", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "--with-bdeps=y explicit: same as the default",
        ["--pretend", "--deep", "--with-bdeps", "y", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "--with-bdeps=n inline form",
        ["--pretend", "--deep", "--with-bdeps=n", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "--with-bdeps has no effect on a New (not-yet-installed) top-level atom",
        ["--pretend", "--with-bdeps", "n", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "--with-bdeps with no argument is a real, immediate usage error",
        ["--pretend", "--with-bdeps"],
        2,
    ),
    (
        "--with-bdeps with an invalid choice is a real, immediate usage error",
        ["--pretend", "--with-bdeps", "auto", "dev-libs/newpkg"],
        2,
    ),
    (
        "--with-bdeps-auto is now implemented, not rejected",
        ["--pretend", "--with-bdeps-auto", "n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--with-bdeps-auto with an invalid choice is a real, immediate usage error",
        ["--pretend", "--with-bdeps-auto", "maybe", "dev-libs/newpkg"],
        2,
    ),
    (
        "--with-bdeps-auto=n does not override an explicit --with-bdeps",
        ["--pretend", "--deep", "--with-bdeps", "y", "--with-bdeps-auto", "n", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "--with-bdeps-auto n changes the default with_bdeps for --deep",
        ["--pretend", "--deep", "--with-bdeps-auto", "n", "dev-libs/withbdepspkg"],
        0,
    ),
    (
        "without --changed-deps, a dependency change is never detected",
        ["--pretend", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps: reinstalls a package whose vdb-recorded deps differ from the current ebuild",
        ["--pretend", "--changed-deps", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps=y inline form",
        ["--pretend", "--changed-deps=y", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps n explicitly disables it",
        ["--pretend", "--changed-deps", "n", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps: a libc-only dependency change is ignored (strip_libc_deps)",
        ["--pretend", "--changed-deps", "dev-libs/libcnoisepkg"],
        0,
    ),
    (
        "--changed-deps: an atom moved between two dep keys is a change (per-key comparison)",
        ["--pretend", "--changed-deps", "dev-libs/movedkeydepspkg"],
        0,
    ),
    (
        "--changed-deps: a built := dep's resolved slot is not a change (strip_slots)",
        ["--pretend", "--changed-deps", "dev-libs/slotopdepspkg"],
        0,
    ),
    (
        "--changed-deps: a || ( a b ) -> || ( b a ) alternative reorder is a change",
        ["--pretend", "--changed-deps", "dev-libs/anyofreorderdepspkg"],
        0,
    ),
    (
        "--changed-deps: a plain 'a b' -> 'b a' reorder is a change (faithful list ==)",
        ["--pretend", "--changed-deps", "dev-libs/orderchangeddepspkg"],
        0,
    ),
    (
        "--changed-deps: a redundant-bracket difference is not a change",
        ["--pretend", "--changed-deps", "dev-libs/redundantbracketdepspkg"],
        0,
    ),
    (
        "--changed-deps: --json includes the changed_deps field",
        ["--pretend", "--changed-deps", "--json", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps-report=n: silent, same as not giving it at all",
        ["--pretend", "--changed-deps-report=n", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps-report: reports without reinstalling",
        ["--pretend", "--changed-deps-report", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--changed-deps-report combined with --changed-deps: silenced, --changed-deps still reinstalls",
        ["--pretend", "--changed-deps-report", "--changed-deps", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "without --changed-slot, a SLOT change is never detected",
        ["--pretend", "dev-libs/changedslotpkg"],
        0,
    ),
    (
        "--changed-slot: reinstalls a package whose vdb-recorded SLOT differs from the current ebuild",
        ["--pretend", "--changed-slot", "dev-libs/changedslotpkg"],
        0,
    ),
    (
        "--changed-slot=y inline form",
        ["--pretend", "--changed-slot=y", "dev-libs/changedslotpkg"],
        0,
    ),
    (
        "--changed-slot n explicitly disables it",
        ["--pretend", "--changed-slot", "n", "dev-libs/changedslotpkg"],
        0,
    ),
    (
        "--changed-slot: --json includes the changed_slot field",
        ["--pretend", "--changed-slot", "--json", "dev-libs/changedslotpkg"],
        0,
    ),
    (
        "--changed-deps and --changed-slot combine in one reinstall reason",
        ["--pretend", "--changed-deps", "--changed-slot", "dev-libs/changedslotpkg"],
        0,
    ),
    (
        "without --with-test-deps, a test?-gated dep is never pulled in",
        ["--pretend", "dev-libs/withtestdeppkg"],
        0,
    ),
    (
        "--with-test-deps pulls in a top-level atom's own test?-gated dep",
        ["--pretend", "--with-test-deps", "dev-libs/withtestdeppkg"],
        0,
    ),
    (
        "--with-test-deps=y inline form",
        ["--pretend", "--with-test-deps=y", "dev-libs/withtestdeppkg"],
        0,
    ),
    (
        "--with-test-deps n explicitly disables it",
        ["--pretend", "--with-test-deps", "n", "dev-libs/withtestdeppkg"],
        0,
    ),
    (
        "--with-test-deps does not apply beyond a top-level (depth 0) atom",
        ["--pretend", "--with-test-deps", "dev-libs/withtestdepconsumer"],
        0,
    ),
    (
        "USE_EXPAND_IMPLICIT: a foo[elibc_glibc] dep matches a foo that never lists elibc_glibc",
        ["--pretend", "dev-libs/implicitiusepkg"],
        0,
    ),
    (
        "USE_EXPAND_IMPLICIT: elibc_musl is valid implicit IUSE but not enabled, so the dep is unsatisfiable",
        ["--pretend", "dev-libs/implicitiusepkgmusl"],
        0,
    ),
    (
        "USE_EXPAND _* wildcard enables every matching flag in the package's own IUSE",
        ["--pretend", "-v", "dev-libs/wildexpandpkg"],
        0,
    ),
    (
        "new-slot install: :1 requested while only :0 is installed -> [ebuild NS]",
        ["--pretend", "dev-libs/newslotpkg:1"],
        0,
    ),
    (
        "new-slot install: bare atom (non-selective) resolves the highest version into its new slot",
        ["--pretend", "dev-libs/newslotpkg"],
        0,
    ),
    (
        "new-slot install: -v keeps the S column ahead of the -v-only mask column",
        ["--pretend", "-v", "dev-libs/newslotpkg:1"],
        0,
    ),
    (
        "new-slot install: --columns S column",
        ["--pretend", "--columns", "dev-libs/newslotpkg:1"],
        0,
    ),
    (
        "new-slot install: --json new_slot field",
        ["--pretend", "--json", "dev-libs/newslotpkg:1"],
        0,
    ),
    (
        "in-slot request (:0, the installed slot) is not a new-slot install",
        ["--pretend", "dev-libs/newslotpkg:0"],
        0,
    ),
    (
        "interactive bracket column: unconditional PROPERTIES=interactive -> [ebuild IN]",
        ["--pretend", "dev-libs/interactivemergepkg"],
        0,
    ),
    (
        "interactive bracket column: -v keeps I ahead of the mask column",
        ["--pretend", "-v", "dev-libs/interactivemergepkg"],
        0,
    ),
    (
        "interactive bracket column: --columns",
        ["--pretend", "--columns", "dev-libs/interactivemergepkg"],
        0,
    ),
    (
        "interactive bracket column: --json interactive field",
        ["--pretend", "--json", "dev-libs/interactivemergepkg"],
        0,
    ),
    (
        "interactive bracket column: a USE-conditional interactive token gated OFF -> no I",
        ["--pretend", "dev-libs/interactivecondpkg"],
        0,
    ),
    (
        "interactive bracket column: an installed interactive package reinstalls as [ebuild Ir]",
        ["--pretend", "dev-libs/interactiveinstalledpkg"],
        0,
    ),
    (
        "Total: counters line (-v) with a dependency",
        ["--pretend", "-v", "dev-libs/useexpandpkg"],
        0,
    ),
    (
        "Total: counters line (-v) counts a new-slot install separately",
        ["--pretend", "-v", "dev-libs/newslotpkg:1"],
        0,
    ),
    (
        "Total: counters line (-v) with a blocker Conflict: line",
        ["--pretend", "-v", "dev-libs/blockerpkg"],
        0,
    ),
    (
        "Total: counters line survives --columns",
        ["--pretend", "-v", "--columns", "dev-libs/interactivemergepkg"],
        0,
    ),
    (
        "Total: counters line -- nothing to install",
        ["--pretend", "-v", "--noreplace", "dev-libs/samepkg"],
        0,
    ),
    (
        "--tree nests every slot of a multi-slot dependency under its parent",
        ["--pretend", "--tree", "dev-libs/multislotparent"],
        0,
    ),
    (
        "--json required_by is set on every slot of a multi-slot dependency",
        ["--pretend", "--json", "dev-libs/multislotparent"],
        0,
    ),
    (
        "fetch-restrict column: RESTRICT=fetch, distfile present -> f",
        ["--pretend", "dev-libs/fetchrestrictsatisfiedpkg"],
        0,
    ),
    (
        "fetch-restrict column: RESTRICT=fetch, distfile missing -> F",
        ["--pretend", "dev-libs/fetchrestrictmissingpkg"],
        0,
    ),
    (
        "fetch-restrict column: -v keeps f/F ahead of the mask column",
        ["--pretend", "-v", "dev-libs/fetchrestrictmissingpkg"],
        0,
    ),
    (
        "fetch-restrict column: --columns",
        ["--pretend", "--columns", "dev-libs/fetchrestrictsatisfiedpkg"],
        0,
    ),
    (
        "fetch-restrict column: --json fetch_restrict fields",
        ["--pretend", "--json", "dev-libs/fetchrestrictmissingpkg"],
        0,
    ),
    (
        "--pretend --unmerge: a plain installed package",
        ["--pretend", "--unmerge", "dev-libs/unmergepkg"],
        0,
    ),
    (
        "-pC: a versioned atom, other version omitted",
        ["--pretend", "-C", "=dev-libs/unmergepkg-1.0"],
        0,
    ),
    (
        "-pC: a bare package name (null-category vdb lookup)",
        ["--pretend", "-C", "unmergepkg"],
        0,
    ),
    (
        "-pC: multiple atoms in one invocation",
        ["--pretend", "-C", "dev-libs/unmergepkg", "dev-libs/samepkg"],
        0,
    ),
    (
        "-pC sys-apps/portage: refused, nothing selected",
        ["--pretend", "-C", "sys-apps/portage"],
        1,
    ),
    (
        "-pC: an atom that matches no installed package",
        ["--pretend", "-C", "dev-libs/nonexistent"],
        1,
    ),
    (
        "-pC @system: selects the installed @system member, with the profile warning",
        ["--pretend", "-C", "@system"],
        0,
    ),
    (
        "-pC with no atoms",
        ["--pretend", "-C"],
        1,
    ),
    (
        "-C without --pretend is refused",
        ["--unmerge", "dev-libs/unmergepkg"],
        2,
    ),
    (
        "-pC: the 'is part of your system profile' warning",
        ["--pretend", "-C", "dev-libs/systempkg"],
        0,
    ),
    (
        "-pC: the 'still listed in package sets' warning",
        ["--pretend", "-C", "dev-libs/nestedsetpkg"],
        0,
    ),
    (
        "-pC @nestedtestset: the set is active, so no set-protection warning",
        ["--pretend", "-C", "@nestedtestset"],
        0,
    ),
    (
        "-pC: system-profile + set warnings + a plain package in one run",
        ["--pretend", "-C", "dev-libs/systempkg", "dev-libs/nestedsetpkg", "dev-libs/unmergepkg"],
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


def test_root_deps_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--root-deps: rootdepspkg's own BDEPEND (dev-libs/rootdepsprovider)
    has no ebuild anywhere in the fixture repo tree -- only a hand-seeded
    vdb entry. PORTAGE_RUNNING_ROOT (a pilot-specific, test-only override
    -- see running_root_from_env's own doc comment) is pointed at the
    same fixture tree here purely as a convenient real vdb; ordinary
    dependency resolution never consults a root's own vdb at all, only
    the ebuild repo tree, so this is a valid, real proof the new
    running-root check is what's excluding it, not some other
    pre-existing mechanism."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = env["ROOT"]
    args_without = ["--pretend", "dev-libs/rootdepspkg"]
    args_with = ["--pretend", "--root-deps", "dev-libs/rootdepspkg"]

    rust_without = _run([str(emerge_binary)], args_without, env)
    python_without = _run(emerge_pretend_python, args_without, env)
    assert rust_without.returncode == 0
    assert python_without.returncode == 0
    assert rust_without.stdout == python_without.stdout
    assert rust_without.stderr == python_without.stderr
    assert "no visible ebuild for dependency" in rust_without.stderr

    rust_with = _run([str(emerge_binary)], args_with, env)
    python_with = _run(emerge_pretend_python, args_with, env)
    assert rust_with.returncode == 0
    assert python_with.returncode == 0
    assert rust_with.stdout == python_with.stdout
    assert rust_with.stderr == python_with.stderr
    assert rust_with.stderr == ""
    assert rust_with.stdout.strip() == "[ebuild  N] dev-libs/rootdepspkg-1.0"


def test_root_deps_disjunctive_branch_selection_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--root-deps branch-selection feed-in: rootdepsorpkg's own BDEPEND
    is "|| ( dev-libs/rootdepsnonexistent dev-libs/rootdepsprovider )" --
    neither branch has an ebuild anywhere in the fixture repo tree, so
    without --root-deps no branch resolves at all and *both* are reported
    as unresolvable dependencies (this pilot's own pre-existing "leave an
    unresolved || group's branches all in flat_deps" fallback, unrelated
    to --root-deps itself). With --root-deps, rootdepsprovider's own
    running-root satisfaction lets the closure select that branch
    specifically, so neither branch is reported at all: rootdepsprovider
    because it's already satisfied, rootdepsnonexistent because it was
    never selected in the first place."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = env["ROOT"]
    args_without = ["--pretend", "dev-libs/rootdepsorpkg"]
    args_with = ["--pretend", "--root-deps", "dev-libs/rootdepsorpkg"]

    rust_without = _run([str(emerge_binary)], args_without, env)
    python_without = _run(emerge_pretend_python, args_without, env)
    assert rust_without.returncode == 0
    assert python_without.returncode == 0
    assert rust_without.stdout == python_without.stdout
    assert rust_without.stderr == python_without.stderr
    assert "no visible ebuild for dependency" in rust_without.stderr

    rust_with = _run([str(emerge_binary)], args_with, env)
    python_with = _run(emerge_pretend_python, args_with, env)
    assert rust_with.returncode == 0
    assert python_with.returncode == 0
    assert rust_with.stdout == python_with.stdout
    assert rust_with.stderr == python_with.stderr
    assert rust_with.stderr == ""
    assert rust_with.stdout.strip() == "[ebuild  N] dev-libs/rootdepsorpkg-1.0"


def test_root_deps_recursive_build_entry_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real "recursively pull in and build a new package against the
    running root" (--root-deps's own last remaining documented gap, see
    resolve_root_deps_build_entry's own doc comment in portage-repo/src/
    lib.rs): unlike rootdepspkg/rootdepsorpkg above, rootdepsbuildpkg's
    own BDEPEND (dev-libs/rootdepsbuildtool) has a real, tree-visible
    ebuild -- deliberately, so this exercises the new build-entry path
    rather than the older running-root-satisfiability check alone. It
    isn't installed in the running root either way, so both with and
    without --root-deps it falls through to a real New entry -- without
    --root-deps via this pilot's own pre-existing (unrelated to this
    slice) "BDEPEND resolved as an ordinary ROOT-targeted dependency"
    fallback, with --root-deps via the new targets_running_root entry
    instead. The --root-deps case now carries a " to <running root>"
    marker on the build entry (real output.py:841-862) that the fallback
    never has -- see
    test_root_deps_build_entry_output_marks_the_running_root below for a
    dedicated, deterministic (PORTAGE_RUNNING_ROOT=/) check of that
    marker across plain/--json/--tree; this test still proves the two
    code paths agree between Rust and Python for the non-marker parts."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = env["ROOT"]
    args_without = ["--pretend", "dev-libs/rootdepsbuildpkg"]
    args_with = ["--pretend", "--root-deps", "dev-libs/rootdepsbuildpkg"]

    rust_without = _run([str(emerge_binary)], args_without, env)
    python_without = _run(emerge_pretend_python, args_without, env)
    assert rust_without.returncode == 0
    assert python_without.returncode == 0
    assert rust_without.stdout == python_without.stdout
    assert rust_without.stderr == python_without.stderr
    assert rust_without.stdout.strip() == (
        "[ebuild  N] dev-libs/rootdepsbuildpkg-1.0\n"
        "[ebuild  N] dev-libs/rootdepsbuildtool-1.0"
    )

    rust_with = _run([str(emerge_binary)], args_with, env)
    python_with = _run(emerge_pretend_python, args_with, env)
    assert rust_with.returncode == 0
    assert python_with.returncode == 0
    assert rust_with.stdout == python_with.stdout
    assert rust_with.stderr == python_with.stderr
    # The running-root build entry is now visually distinguished from the
    # pre-existing ROOT-targeted fallback (this slice): with --root-deps
    # the rootdepsbuildtool line carries a " to <running root>" marker.
    assert rust_with.stdout == (
        "[ebuild  N] dev-libs/rootdepsbuildpkg-1.0\n"
        f"[ebuild  N] dev-libs/rootdepsbuildtool-1.0 to {env['ROOT']}\n"
    )
    assert rust_with.stdout != rust_without.stdout


def test_root_deps_build_entry_output_marks_the_running_root(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """The " to <running root>" marker this pilot adds to a --root-deps
    running-root build entry (real lib/_emerge/resolver/output.py:841-862's
    own darkgreen("to " + pkg.root), narrowed -- see pretend.rs's own
    root_suffix docstring). PORTAGE_RUNNING_ROOT is pinned to "/" here so
    the marker text is deterministic ("to /", real portage's own common
    case) rather than a per-checkout mktemp path; "/" genuinely has no
    dev-libs/rootdepsbuildtool installed, so the atom still resolves to a
    real New entry. Checked across all three output modes -- plain,
    --json (a "builds_against_running_root" field), and --tree (the
    marker survives the indent) -- with Rust/Python byte-for-byte parity
    asserted for each."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    base = ["--pretend", "--root-deps", "dev-libs/rootdepsbuildpkg"]

    rust_plain = _run([str(emerge_binary)], base, env)
    python_plain = _run(emerge_pretend_python, base, env)
    assert rust_plain.returncode == 0
    assert python_plain.returncode == 0
    assert rust_plain.stdout == python_plain.stdout
    assert rust_plain.stderr == python_plain.stderr
    assert rust_plain.stdout == (
        "[ebuild  N] dev-libs/rootdepsbuildpkg-1.0\n"
        "[ebuild  N] dev-libs/rootdepsbuildtool-1.0 to /\n"
    )

    rust_json = _run([str(emerge_binary)], [*base, "--json"], env)
    python_json = _run(emerge_pretend_python, [*base, "--json"], env)
    assert rust_json.stdout == python_json.stdout
    parsed = json.loads(rust_json.stdout)
    by_pkg = {e["package"]: e for e in parsed["entries"]}
    assert by_pkg["rootdepsbuildtool"]["builds_against_running_root"] == "/"
    assert by_pkg["rootdepsbuildpkg"]["builds_against_running_root"] is None

    rust_tree = _run([str(emerge_binary)], [*base, "--tree"], env)
    python_tree = _run(emerge_pretend_python, [*base, "--tree"], env)
    assert rust_tree.stdout == python_tree.stdout
    assert rust_tree.stdout == (
        "[ebuild  N] dev-libs/rootdepsbuildpkg-1.0\n"
        "[ebuild  N]   dev-libs/rootdepsbuildtool-1.0 to /\n"
    )


def test_root_deps_recursion_walks_the_build_entrys_own_deps(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """A running-root build entry's own DEPEND/BDEPEND/RDEPEND are now
    walked against the running root too, recursively (real
    depgraph.py:4207-4271: a package whose pkg.root is the running root
    has all three keys resolved there). rdrapp BDEPENDs rdrtool, which
    itself BDEPENDs rdrtooldep and RDEPENDs rdrlib -- so all four appear,
    the three build-against-/ entries carrying the " to /" marker, and
    --tree nests each under its immediate requester. RDEPEND being walked
    (rdrlib) is the deliberately-broader half of this slice."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    base = ["--pretend", "--root-deps", "dev-libs/rdrapp"]

    rust = _run([str(emerge_binary)], base, env)
    python = _run(emerge_pretend_python, base, env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout == (
        "[ebuild  N] dev-libs/rdrapp-1.0\n"
        "[ebuild  N] dev-libs/rdrtool-1.0 to /\n"
        "[ebuild  N] dev-libs/rdrtooldep-1.0 to /\n"
        "[ebuild  N] dev-libs/rdrlib-1.0 to /\n"
    )

    rust_tree = _run([str(emerge_binary)], [*base, "--tree"], env)
    python_tree = _run(emerge_pretend_python, [*base, "--tree"], env)
    assert rust_tree.stdout == python_tree.stdout
    assert rust_tree.stdout == (
        "[ebuild  N] dev-libs/rdrapp-1.0\n"
        "[ebuild  N]   dev-libs/rdrtool-1.0 to /\n"
        "[ebuild  N]     dev-libs/rdrlib-1.0 to /\n"
        "[ebuild  N]     dev-libs/rdrtooldep-1.0 to /\n"
    )


def test_root_deps_recursion_walks_a_build_entrys_own_idepend(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real portage resolves IDEPEND against the running root always
    (depgraph.py:4247-4252, independent of --root-deps). The recursion
    into a running-root build entry now covers IDEPEND alongside
    DEPEND/BDEPEND/RDEPEND: rdriapp BDEPENDs rdritool, whose own IDEPEND
    (rdrilib) is pulled in as its own " to /" entry."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    base = ["--pretend", "--root-deps", "dev-libs/rdriapp"]

    rust = _run([str(emerge_binary)], base, env)
    python = _run(emerge_pretend_python, base, env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout == (
        "[ebuild  N] dev-libs/rdriapp-1.0\n"
        "[ebuild  N] dev-libs/rdritool-1.0 to /\n"
        "[ebuild  N] dev-libs/rdrilib-1.0 to /\n"
    )


def test_root_deps_top_level_idepend_resolves_against_the_running_root(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real portage resolves IDEPEND against the running root for *every*
    package, not just recursed running-root build entries
    (depgraph.py:4247-4252). The two ordinary dep-walk sites now route a
    top-level package's own IDEPEND to the running root under --root-deps,
    exactly like DEPEND/BDEPEND: topidepapp IDEPENDs topideplib, so with
    --root-deps topideplib carries the " to /" marker (and is walked
    against the running root), whereas without --root-deps it is a plain
    ROOT-targeted entry."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    without = ["--pretend", "dev-libs/topidepapp"]
    with_ = ["--pretend", "--root-deps", "dev-libs/topidepapp"]

    rust_without = _run([str(emerge_binary)], without, env)
    python_without = _run(emerge_pretend_python, without, env)
    assert rust_without.returncode == 0
    assert python_without.returncode == 0
    assert rust_without.stdout == python_without.stdout
    assert rust_without.stderr == python_without.stderr
    assert rust_without.stdout == (
        "[ebuild  N] dev-libs/topidepapp-1.0\n"
        "[ebuild  N] dev-libs/topideplib-1.0\n"
    )

    rust_with = _run([str(emerge_binary)], with_, env)
    python_with = _run(emerge_pretend_python, with_, env)
    assert rust_with.returncode == 0
    assert python_with.returncode == 0
    assert rust_with.stdout == python_with.stdout
    assert rust_with.stderr == python_with.stderr
    assert rust_with.stdout == (
        "[ebuild  N] dev-libs/topidepapp-1.0\n"
        "[ebuild  N] dev-libs/topideplib-1.0 to /\n"
    )
    assert rust_with.stdout != rust_without.stdout


def test_root_deps_recursion_terminates_on_a_bdepend_cycle(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """rdrcyca BDEPENDs rdrcycb which BDEPENDs rdrcyca -- an unremarkable
    bootstrap pattern. The shared root_deps_build_seen set is the cycle
    guard (a (category, package) is inserted before its own deps are
    walked), so the recursion terminates with each cycle node appearing
    exactly once rather than overflowing the stack."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    base = ["--pretend", "--root-deps", "dev-libs/rdrcyc"]

    rust = _run([str(emerge_binary)], base, env)
    python = _run(emerge_pretend_python, base, env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stdout == (
        "[ebuild  N] dev-libs/rdrcyc-1.0\n"
        "[ebuild  N] dev-libs/rdrcyca-1.0 to /\n"
        "[ebuild  N] dev-libs/rdrcycb-1.0 to /\n"
    )


def test_root_deps_recursion_reports_an_unbuildable_build_dep(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """rdrmisstool (pulled in against the running root) BDEPENDs
    rdrnothere, which has no ebuild anywhere and isn't installed. Before
    this slice --root-deps silently swallowed such a dep; now it's
    surfaced by the renderer's own non-fatal "!!! no visible ebuild for
    dependency" note, exactly as it would be without --root-deps (exit 0
    -- it's a dependency, not the top-level atom)."""
    env = dict(fixture_env)
    env["PORTAGE_RUNNING_ROOT"] = "/"
    base = ["--pretend", "--root-deps", "dev-libs/rdrmiss"]

    rust = _run([str(emerge_binary)], base, env)
    python = _run(emerge_pretend_python, base, env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout == (
        "[ebuild  N] dev-libs/rdrmiss-1.0\n"
        "[ebuild  N] dev-libs/rdrmisstool-1.0 to /\n"
    )
    assert '!!! no visible ebuild for dependency "dev-libs/rdrnothere"' in rust.stderr


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


def test_any_of_group_resolves_only_the_first_satisfiable_alternative(
    emerge_binary, fixture_env
):
    """Real "||" semantics (see use_reduce_flat_disjunctive's own doc
    comment, portage-use-reduce): of `|| ( dev-libs/newpkg
    dev-libs/samepkg )`, only dev-libs/newpkg (listed first, and
    visible) is even enqueued -- dev-libs/samepkg (already installed,
    also satisfiable, but never reached) doesn't show up at all. Same
    displayed stdout either way samepkg would have been silent anyway
    (an AlreadyInstalled dependency never prints under plain --pretend),
    but the underlying resolution now genuinely stops at the first
    satisfiable alternative instead of walking every one."""
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


def test_sub_slot_restricted_dependency_atom_matches_the_real_sub_slot(
    emerge_binary, fixture_env
):
    """dev-libs/subslotconsumer's own RDEPEND is
    "dev-libs/subslotpkg:0/2" -- a real sub-slot restriction (PMS 8.3.3),
    not a slot-operator (":="/"slot=") atom. Prior to this slice,
    portage-repo's own Candidate struct (and every candidate string built
    from it for match_from_list) discarded the sub-slot half of a real
    "SLOT=main/sub" value entirely (`.split('/').next()`), so this atom
    could never match anything -- silently, the same "no entry, no
    error" outcome the slot-operator bug had, just one layer deeper (the
    atom parsed and reached match_from_list fine; the *candidate* was
    the one missing data). dev-libs/subslotpkg's own SLOT is "0/2", an
    exact match for the restriction, so this must resolve normally."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/subslotconsumer"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/subslotconsumer-1.0",
        "[ebuild  N] dev-libs/subslotpkg-1.0",
    ]


def test_sub_slot_restricted_dependency_atom_rejects_a_real_mismatch(
    emerge_binary, fixture_env
):
    """The mirror case: dev-libs/subslotmismatchconsumer's own RDEPEND is
    "dev-libs/subslotpkg:0/3", but dev-libs/subslotpkg's own SLOT is
    "0/2" -- a genuine sub-slot mismatch. Proves the fix is real matching
    (rejects an incompatible sub-slot), not just "always accept" (which
    an implementation that dropped the sub-slot restriction from the
    *atom* side, rather than the candidate side, could still pass the
    companion test above with)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/subslotmismatchconsumer"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/subslotmismatchconsumer-1.0"]
    assert (
        result.stderr.splitlines()
        == ['!!! no visible ebuild for dependency "dev-libs/subslotpkg"']
    )


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
    reported on stderr, not silently dropped or silently accepted. Also
    carries a --autounmask-use suggestion (on by default, unlike the
    keyword one -- see the autounmask_suggest_use tests below) since
    "foo" is genuinely settable via package.use here."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/usedeprejectedpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/usedeprejectedpkg-1.0"]
    assert result.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/useflagpkg"\n'
        '!!! note: dev-libs/useflagpkg-1.0 exists but its USE flags don\'t satisfy '
        'this atom; --autounmask-use suggests adding "=dev-libs/useflagpkg-1.0 -foo" '
        'to package.use'
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
    candidate for this atom at all. Also carries a --autounmask-use
    suggestion (on by default) since "foo" is genuinely settable via
    package.use here."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg[-foo]"], fixture_env
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".\n'
        'note: dev-libs/useflagpkg-1.0 exists but its USE flags don\'t satisfy this '
        'atom; --autounmask-use suggests adding "=dev-libs/useflagpkg-1.0 -foo" to '
        'package.use'
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


def test_iuse_plus_minus_defaults_apply_when_nothing_else_says_otherwise(
    emerge_binary, fixture_env
):
    """A real, previously-undetected gap, found by comparing this pilot's
    own output against the real, installed system emerge on a real
    package (media-video/ffmpeg) -- REQUIRED_USE reported violated for a
    USE combination that's actually fully satisfied once IUSE's own
    "+"/"-" markers are honored. dev-libs/iusedefaultpkg's own IUSE is
    "+enableddefault -disableddefault plainflag": before this slice,
    this pilot's own effective_use_flags never consulted IUSE's own
    default markers at all, so "enableddefault" would have defaulted to
    disabled -- violating this fixture's own REQUIRED_USE
    ("enableddefault !disableddefault") and aborting the whole run with a
    spurious REQUIRED_USE error, the same failure mode discovered live
    against ffmpeg. "plainflag" (no default marker at all) is genuinely
    undecided by IUSE itself, but forced on by this package's own
    package.use entry -- proving IUSE defaults and package.use coexist
    and layer correctly (package.use still wins), not just that IUSE
    defaults exist in isolation."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/iusedefaultpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == (
        '[ebuild  N] dev-libs/iusedefaultpkg-1.0  USE="-disableddefault enableddefault plainflag"\n'
        "\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n"
    )


def test_required_use_referencing_an_implicit_arch_flag_resolves_normally(
    emerge_binary, fixture_env
):
    """A second, related gap from the same ffmpeg investigation -- this
    one surfaced on a downstream dependency, real media-libs/mesa, whose
    own REQUIRED_USE references "x86" without ever declaring it in its
    own IUSE. Real portage validates a REQUIRED_USE-referenced flag
    against pkg.iuse.is_valid_flag, which real config.py's own
    _get_implicit_iuse() extends with PORTAGE_ARCHLIST (profiles/
    arch.list) among other things -- "x86" is a real, valid arch.list
    entry even on an amd64 profile, just not the active arch, so it's
    implicitly valid (and stays disabled). Before this slice, this
    pilot's own iuse_set was built purely from a package's own literal
    IUSE, so this fixture (mirroring mesa's shape: empty IUSE,
    REQUIRED_USE="!x86") would abort with "USE flag 'x86' is not in
    IUSE" instead of resolving -- confirmed live against the real,
    installed system, both before this fix (reproduced the failure) and
    after (mesa resolves)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "dev-libs/archiuseimplicitpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        "[ebuild  N] dev-libs/archiuseimplicitpkg-1.0\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n"
    )


def test_global_use_force_and_use_mask_win_over_a_contradicting_package_use_entry(
    emerge_binary, fixture_env
):
    """Found by reading real config.py's own regenerate() end to end
    (lib/portage/package/ebuild/config.py, ~line 3024):
    myflags.update(self.useforce) followed by
    myflags.difference_update(self.usemask) runs as the literal *last*
    step of the incremental USE walk, strictly *after* the "pkg"
    (package.use) tier -- and setcpv() confirms self.useforce/
    self.usemask are themselves getUseForce(pkg)/getUseMask(pkg), i.e.
    *global* use.force/use.mask combined with the atom-scoped
    package.use.force/.mask this pilot already applies last. Before this
    slice, this pilot folded global use_force/use_mask into `base` early
    (inside portage_profile::resolve_config), before package.use ever
    ran in effective_use_flags -- so a package.use entry could
    previously override a global force/mask decision real portage never
    lets it override. dev-libs/globalprecedencepkg's own IUSE is
    "globalforceflag globalmaskflag" (both markerless, genuinely
    undecided by IUSE itself); its own package.use entry is
    "-globalforceflag globalmaskflag" (an attempt to invert both); the
    fixture profile's own use.force declares "globalforceflag" and
    use.mask declares "globalmaskflag". If package.use won, the result
    would invert to "-globalforceflag globalmaskflag" -- instead, global
    force/mask should win on both, leaving the flags exactly as the
    profile forced/masked them regardless of what package.use tried."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "dev-libs/globalprecedencepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        '[ebuild  N] dev-libs/globalprecedencepkg-1.0  USE="(globalforceflag) (-globalmaskflag)"\n'
        "\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n"
    )


def test_profile_level_minus_flag_genuinely_cancels_an_iuse_plus_default(
    emerge_binary, fixture_env
):
    """The gap this pilot's own IUSE-defaults slice originally left open,
    now closed: real regenerate() runs ONE continuous incremental walk
    (pkginternal -> defaults -> conf -> pkg), so a genuine "-flag" in
    profile/make.conf really does cancel an earlier IUSE "+flag" default
    -- not just fail to add on top of it. Before this slice, this
    pilot's own effective_use_flags union-ed the already-flattened
    profile+make.conf result on top of the IUSE-defaults seed, which
    could only ever *add* a flag, never explicitly cancel one --
    dev-libs/cancelledpkg's own IUSE is "+cancelme" (defaults on), and
    fixtures/repo/profiles/default/make.defaults declares a profile-level
    "-cancelme" -- under the old behavior this would have stayed enabled
    (the union could never see the "-"); now it's correctly cancelled."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/cancelledpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == (
        '[ebuild  N] dev-libs/cancelledpkg-1.0  USE="-cancelme"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
    )


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


def test_required_use_violations_are_collected_across_the_whole_walk_not_just_the_first(
    emerge_binary, fixture_env
):
    """Real depgraph.py's own _add_pkg sets
    _dynamic_config._required_use_unsatisfied = True and returns 0 on a
    violation -- it does NOT abort the whole graph walk (unlike a
    top-level atom's own NoVisibleCandidate). Before this slice, this
    pilot's own resolve_pretend_graph returned Err(...) immediately on
    the first REQUIRED_USE violation, meaning a SECOND, independent
    top-level atom passed on the same command line (here,
    dev-libs/requiredusebadpkg2's own "baz? ( qux )", unrelated to
    dev-libs/requiredusebadpkg's own "foo? ( bar )") would never even be
    attempted, let alone reported -- exactly the same failure mode real
    portage doesn't have. Both violations now show up together, in
    argument order."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/requiredusebadpkg", "dev-libs/requiredusebadpkg2"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: '
        '"foo? ( bar )"\n'
        'REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg2-1.0: '
        '"baz? ( qux )"'
    )


def test_autounmask_no_keyword_suggestion_by_default(emerge_binary, fixture_env):
    """Real --autounmask-keep-keywords defaults to True (suppress keyword
    suggestions) whenever --autounmask itself was never explicitly given
    at all -- confirmed by reading create_depgraph_params.py's own
    default-resolution logic end to end. dev-libs/autounmaskkeywordpkg's
    own KEYWORDS is "~amd64", never accepted by the fixture profile's own
    ACCEPT_KEYWORDS and never granted a package.accept_keywords entry --
    masked by KEYWORDS alone. With no --autounmask flag at all (the
    common case), no suggestion is appended, matching real portage's own
    "quiet by default" behavior for this specific sub-flag."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/autounmaskkeywordpkg"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: there are no ebuilds to satisfy "dev-libs/autounmaskkeywordpkg".'
    )


def test_autounmask_suggests_a_keyword_once_explicitly_enabled(emerge_binary, fixture_env):
    """Real portage's own asymmetry (create_depgraph_params.py):
    --autounmask-keep-keywords defaults to False (i.e. keyword
    suggestions ARE generated) once --autounmask itself was explicitly
    given, unlike the ambient always-on default (see the sibling test
    above) -- "explicitly asking for autounmask implies wanting its
    keyword suggestions too." A deliberately narrow v1 (see
    resolve_pretend_graph's own doc comment, portage-repo): only the
    single "masked by KEYWORDS alone" case is detected, and the
    suggestion is a pilot-specific summary, not real portage's own exact
    suggested-atom syntax or dependency-chain-comment formatting."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "dev-libs/autounmaskkeywordpkg"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: there are no ebuilds to satisfy "dev-libs/autounmaskkeywordpkg".\n'
        'note: dev-libs/autounmaskkeywordpkg-1.0 exists but is masked by KEYWORDS; '
        '--autounmask-keep-keywords=n suggests adding "dev-libs/autounmaskkeywordpkg '
        '~amd64" to package.accept_keywords'
    )


def test_autounmask_dependency_gets_no_keyword_suggestion_by_default(emerge_binary, fixture_env):
    """dev-libs/autounmaskdepconsumer RDEPENDs on dev-libs/
    autounmaskkeywordpkg (the same keyword-masked-only fixture the
    top-level tests above use) -- a *dependency's* own
    no-visible-candidate, previously always silent beyond the bare
    "no visible ebuild" line, regardless of --autounmask. With no
    --autounmask flag at all (the real, correct default), no suggestion
    is appended here either, matching the top-level case's own default."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/autounmaskdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N] dev-libs/autounmaskdepconsumer-1.0\n"
    assert result.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/autounmaskkeywordpkg"'
    )


def test_autounmask_dependency_gets_a_keyword_suggestion_once_enabled(emerge_binary, fixture_env):
    """Extends --autounmask's own keyword-suggestion sub-feature (task
    #51) to a *dependency's* own no-visible-candidate -- previously
    deliberately out of scope (resolve_pretend_graph's own doc comment:
    "suggestions for a dependency's own NoVisibleCandidate"). Unlike the
    top-level case, this dependency's own no-visible-candidate is never
    fatal -- the graph still resolves, dev-libs/autounmaskdepconsumer
    itself still prints as a normal New entry on stdout, and the note is
    just an extra stderr line alongside the pre-existing "no visible
    ebuild" one."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "dev-libs/autounmaskdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N] dev-libs/autounmaskdepconsumer-1.0\n"
    assert result.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/autounmaskkeywordpkg"\n'
        '!!! note: dev-libs/autounmaskkeywordpkg-1.0 exists but is masked by KEYWORDS; '
        '--autounmask-keep-keywords=n suggests adding "dev-libs/autounmaskkeywordpkg '
        '~amd64" to package.accept_keywords'
    )


def test_autounmask_dependency_keyword_suggestion_appears_in_json(emerge_binary, fixture_env):
    """--json's own mirror of the plain-text note above: a
    "no_visible_candidate" entry carries a "keyword_suggestion" field
    (present only for that one outcome, the mirror image of
    "provenance", which is absent there instead) -- {"version",
    "keyword"} when --autounmask found something to suggest, null
    otherwise."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "--json", "dev-libs/autounmaskdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    dep = next(e for e in payload["entries"] if e["package"] == "autounmaskkeywordpkg")
    assert dep["outcome"] == "no_visible_candidate"
    assert "provenance" not in dep
    assert dep["keyword_suggestion"] == {"version": "1.0", "keyword": "~amd64"}
    consumer = next(e for e in payload["entries"] if e["package"] == "autounmaskdepconsumer")
    assert consumer["outcome"] == "new"
    assert "keyword_suggestion" not in consumer


def test_autounmask_use_dependency_suggestion_is_suppressed_by_autounmask_use_n(
    emerge_binary, fixture_env
):
    """--autounmask-use has no "suppressed unless --autounmask itself was
    explicitly given" asymmetry the way --autounmask-keep-keywords does
    (see resolve_pretend_graph's own docstring) -- it's on by default
    whenever autounmask itself is (which itself defaults on), so
    dev-libs/usedeprejectedpkg's own dependency-level suggestion (see the
    plain-text test above) already appears with no flag at all. An
    explicit --autounmask-use=n is the only way to suppress it."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/usedeprejectedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/usedeprejectedpkg-1.0"]
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/useflagpkg"'
    )


def test_autounmask_use_dependency_suggestion_appears_in_json(emerge_binary, fixture_env):
    """--json's own mirror of the plain-text note above: a
    "no_visible_candidate" entry carries a "use_suggestion" field
    (present only for that one outcome, the mirror image of
    "provenance") -- {"version", "flags": [{"flag", "enabled"}, ...]}
    when --autounmask-use found something to suggest, null otherwise."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/usedeprejectedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    dep = next(e for e in payload["entries"] if e["package"] == "useflagpkg")
    assert dep["outcome"] == "no_visible_candidate"
    assert "provenance" not in dep
    assert dep["use_suggestion"] == {
        "version": "1.0",
        "flags": [{"flag": "foo", "enabled": False}],
    }
    parent = next(e for e in payload["entries"] if e["package"] == "usedeprejectedpkg")
    assert parent["outcome"] == "new"
    assert "use_suggestion" not in parent


def test_autounmask_use_parent_flip_suggestion_appears_in_json(emerge_binary, fixture_env):
    """--autounmask-use's own second, architecturally distinct mechanism
    (real _show_unsatisfied_dep's own opt=/opt? handling, flipping the
    *requesting parent's* own flag rather than the candidate's):
    dev-libs/useeqparentoffpkg's own RDEPEND on dev-libs/useeqchildpkg
    "[eqflag=]" was originally conditional on the parent's own eqflag
    (currently off) -- carries both use_suggestion (Part A, flip the
    child's own eqflag) and the new parent_use_suggestion field (Part B,
    flip the parent's own eqflag instead) simultaneously, matching real
    portage's own missing_use_reasons allowing both fixes for the same
    mismatch."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/useeqparentoffpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    dep = next(e for e in payload["entries"] if e["package"] == "useeqchildpkg")
    assert dep["outcome"] == "no_visible_candidate"
    assert dep["use_suggestion"] == {
        "version": "1.0",
        "flags": [{"flag": "eqflag", "enabled": False}],
    }
    assert dep["parent_use_suggestion"] == {
        "category": "dev-libs",
        "package": "useeqparentoffpkg",
        "version": "1.0",
        "flags": [{"flag": "eqflag", "enabled": True}],
    }
    parent = next(e for e in payload["entries"] if e["package"] == "useeqparentoffpkg")
    assert parent["outcome"] == "new"
    assert "parent_use_suggestion" not in parent


def test_autounmask_use_parent_flip_suggestion_is_suppressed_by_autounmask_use_n(
    emerge_binary, fixture_env
):
    """Both --autounmask-use mechanisms (Part A's plain candidate flip
    and Part B's opt=-aware parent flip) share the same
    autounmask_suggest_use gate -- an explicit --autounmask-use=n
    suppresses both at once."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/useeqparentoffpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/useeqparentoffpkg-1.0"
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/useeqchildpkg"'
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


def test_usepkg_makes_a_binary_only_package_eligible(emerge_binary, fixture_env):
    """dev-libs/binaryonlypkg exists only in PKGDIR's own Packages index
    (see fixtures/pkgdir/Packages), no ebuild anywhere. Real depgraph.py's
    own _dynamic_depgraph_config.__init__ only adds the "binary" db to the
    candidate-pool list when --usepkg is True -- without it, the package
    is entirely invisible, matching the ebuild-only-package "no visible
    ebuild" failure mode."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--usepkg", "dev-libs/binaryonlypkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[binary  N] dev-libs/binaryonlypkg-1.0"]
    assert result.stderr == ""


def test_binpkg_respect_use_rejects_a_use_mismatched_binary_by_default(
    emerge_binary, fixture_env
):
    """Real create_depgraph_params.py's own default-resolution: with
    --usepkg alone (not --usepkgonly), --binpkg-respect-use defaults to
    on. dev-libs/binaryusemismatchpkg's own binary entry has USE: (empty),
    while the fixture profile's own global USE=confflag/foo would select
    "foo" for its IUSE="foo" -- a mismatch, so the binary candidate is
    rejected and the identical-version ebuild (also present) is used
    instead."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "dev-libs/binaryusemismatchpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/binaryusemismatchpkg-1.0"]
    assert result.stderr == ""


def test_usepkgonly_defaults_binpkg_respect_use_off(emerge_binary, fixture_env):
    """The opposite asymmetry: create_depgraph_params.py:47-55 defaults
    --binpkg-respect-use to off once --usepkgonly is given (no ebuild
    fallback exists to reject *to*, so real portage doesn't bother
    rejecting). The same USE-mismatched binary from the sibling test
    above is accepted here."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkgonly", "dev-libs/binaryusemismatchpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[binary  N] dev-libs/binaryusemismatchpkg-1.0"]
    assert result.stderr == ""


def test_downgrade_is_distinguished_from_upgrade(emerge_binary, fixture_env):
    """dev-libs/downgradepkg is installed at 2.0, but only 1.0 is visible
    in the tree (its own 2.0 ebuild is gone) -- real output.py's own
    in-slot best() check (around line 750) flags this as a genuine
    downgrade, not an "upgrade" to an older version; before this slice,
    resolve_pretend labeled ANY version change for an installed package
    as Upgrade without ever comparing versions. The installed version
    (2.0) has no visible candidate of its own, so real avoid_update's
    shortcut doesn't apply even without --update -- see resolve_pretend's
    own doc comment on requiring a *visible* candidate, not just a vdb
    entry."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/downgradepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  D] dev-libs/downgradepkg-1.0 (downgrade from 2.0)"
    ]
    assert result.stderr == ""


def test_keyword_masked_but_installed_top_level_atom_still_downgrades(emerge_binary, fixture_env):
    """dev-libs/keywordmaskedpkg is installed at 2.0 (KEYWORDS="~amd64",
    not accepted under the fixture profile's own default
    ACCEPT_KEYWORDS="amd64") -- only 1.0 (KEYWORDS="amd64") is currently
    visible. As a TOP-LEVEL atom, real depgraph.py's own later
    avoid_update block (`_pkg_visibility_check`) still requires
    visibility, so this stays a real downgrade -- distinct from the
    DEPENDENCY case in the very next test below."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/keywordmaskedpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  D] dev-libs/keywordmaskedpkg-1.0 (downgrade from 2.0)"
    ]
    assert result.stderr == ""


def test_keyword_masked_but_installed_dependency_is_kept_not_downgraded(
    emerge_binary, fixture_env
):
    """dev-libs/needskeywordmasked (New) RDEPENDs on the same
    dev-libs/keywordmaskedpkg as the test above -- but reached only as a
    DEPENDENCY here, real depgraph.py's own EARLIER avoid_update return
    (no visibility check at all, see resolve_pretend's own doc comment)
    means it's kept exactly as installed (2.0), never even considered
    for a downgrade. Confirmed live against a real system
    (sys-fs/fuse's own sys-libs/liburing dependency) before this fix:
    this pilot used to (wrongly) print a spurious downgrade line here."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/needskeywordmasked"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/needskeywordmasked-1.0"]
    assert result.stderr == ""


def test_keyword_masked_but_installed_dependency_with_a_use_dep_is_kept(
    emerge_binary, fixture_env
):
    """dev-libs/needskeywordmaskeduse (New) RDEPENDs on
    dev-libs/keywordmaskedusepkg[flag] -- same keyword-masked-but-
    installed situation as the test above, but with a real USE-dep on
    the atom too (mirroring real sys-fs/fuse's own real
    sys-libs/liburing:=[abi_x86_64(-)?,...] dependency, the actual
    real-world case this fix was built for). The installed version
    (2.0) has real vdb USE="flag" (see the fixture's own IUSE/USE
    files), checked against that real vdb-recorded USE rather than the
    current tree's -- so this is kept exactly as installed too, never
    even considered for a downgrade."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/needskeywordmaskeduse"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/needskeywordmaskeduse-1.0"]
    assert result.stderr == ""


def test_any_of_group_falls_back_to_every_alternative_when_none_satisfiable(
    emerge_binary, fixture_env
):
    """dev-libs/anyofunresolvable's own RDEPEND is
    "|| ( dev-libs/doesnotexist-anywhere dev-libs/alsodoesnotexist-anywhere )"
    -- NEITHER alternative has a visible candidate anywhere, so real
    "||" resolution (use_reduce_flat_disjunctive, portage-use-reduce)
    falls back to keeping every alternative exactly like plain
    use_reduce(flat=True) always did, matching this pilot's own
    pre-existing "never silently wrong about whether a dependency
    exists" invariant -- both get reported on stderr, neither silently
    dropped just because they're inside an unresolvable || group."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/anyofunresolvable"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/anyofunresolvable-1.0"]
    assert result.stderr.strip().splitlines() == [
        '!!! no visible ebuild for dependency "dev-libs/doesnotexist-anywhere"',
        '!!! no visible ebuild for dependency "dev-libs/alsodoesnotexist-anywhere"',
    ]


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
        '[ebuild  N] dev-libs/useexpandpkg-1.0  VIDEO_CARDS="-amdgpu nvidia"',
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 2 packages (2 new), Size of downloads: 0 KiB",
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
        '[ebuild  N] dev-libs/packageuseexpandpkg-1.0  PYTHON_TARGETS="python3_12"',
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 2 packages (2 new), Size of downloads: 0 KiB",
    ]


def test_use_expand_unprefixed_variable_drives_a_dependency(emerge_binary, fixture_env):
    """PORTING/fixtures/repo/profiles/arch/amd64/make.defaults declares
    USE_EXPAND_UNPREFIXED="ARCH" and ARCH="amd64" -- real config.py's own
    USE_EXPAND_UNPREFIXED mechanism (the same one that makes "amd64"
    exist as a real USE flag in actual Gentoo at all) contributes the
    bare pseudo-USE flag "amd64" directly, with no "arch_" prefix at all
    (unlike an ordinary USE_EXPAND variable). dev-libs/archusepkg's own
    "amd64? ( dev-libs/newpkg )" proves the flag genuinely drives
    dependency recursion, not just USE display; "riscv" (never set by
    anything) stays off, so its own "? ( dev-libs/hiddendep )" clause is
    never pulled in."""
    result = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/archusepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N] dev-libs/archusepkg-1.0  USE="amd64 -riscv"',
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 2 packages (2 new), Size of downloads: 0 KiB",
    ]
    assert "hiddendep" not in result.stdout


def test_use_expand_star_wildcard_expands_against_the_packages_own_iuse(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """fixtures/etc/portage/package.use: "dev-libs/wildexpandpkg
    linguas_*" -- real config.py setcpv's own _* wildcard: enable every
    linguas_<x> flag declared in THIS package's own IUSE
    ("linguas_en linguas_de") that isn't masked. profiles/base/
    package.use.mask keeps linguas_en off, so USE ends up
    "linguas_de -linguas_en" (the linguas_* pseudo-flag itself stripped),
    and RDEPEND's "linguas_de? ( wildexpanddep )" fires while
    "linguas_en? ( wildexpandmasked )" does not."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/wildexpandpkg"], fixture_env
    )
    result_py = _run(
        emerge_pretend_python, ["--pretend", "-v", "dev-libs/wildexpandpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == result_py.stdout
    assert result.stderr == result_py.stderr
    assert result.stdout.splitlines() == [
        '[ebuild  N] dev-libs/wildexpandpkg-1.0  LINGUAS="de (-en)"',
        "[ebuild  N] dev-libs/wildexpanddep-1.0",
        "",
        "Total: 2 packages (2 new), Size of downloads: 0 KiB",
    ]
    assert "wildexpandmasked" not in result.stdout
    assert "linguas_*" not in result.stdout


def test_pv_groups_use_by_use_expand_variable(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real output.py:_display_use / map_to_use_expand: `emerge -pv` shows
    IUSE flags split into the plain USE="..." group plus one VAR="..."
    group per USE_EXPAND variable (prefix stripped), empty groups omitted.
    dev-libs/useexpandpkg (IUSE video_cards_nvidia video_cards_amdgpu,
    VIDEO_CARDS a USE_EXPAND var) shows VIDEO_CARDS="-amdgpu nvidia" and
    no USE="" at all."""
    for pkg, expected in [
        ("useexpandpkg", 'VIDEO_CARDS="-amdgpu nvidia"'),
        ("packageuseexpandpkg", 'PYTHON_TARGETS="python3_12"'),
    ]:
        args = ["--pretend", "-v", f"dev-libs/{pkg}"]
        rust = _run([str(emerge_binary)], args, fixture_env)
        python = _run(emerge_pretend_python, args, fixture_env)
        assert rust.returncode == 0
        assert rust.stdout == python.stdout, pkg
        assert rust.stdout.splitlines()[0] == f"[ebuild  N] dev-libs/{pkg}-1.0  {expected}", pkg
        assert 'USE="' not in rust.stdout.splitlines()[0], pkg


def test_pv_omits_a_use_expand_hidden_group(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """USE_EXPAND_HIDDEN="CPU_FLAGS_X86" (fixtures/repo/profiles/base/
    make.defaults): real output.py:map_to_use_expand's remove_hidden
    drops that group from the -pv display entirely. dev-libs/
    hiddenexpandpkg (IUSE cpu_flags_x86_sse2 cpu_flags_x86_avx, sse2
    enabled via CPU_FLAGS_X86="sse2") therefore shows no USE display at
    all -- the flags are still real (they'd gate a dependency), just not
    printed."""
    args = ["--pretend", "-v", "dev-libs/hiddenexpandpkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild  N] dev-libs/hiddenexpandpkg-1.0",
        "",
        "Total: 1 package (1 new), Size of downloads: 0 KiB",
    ]
    assert "CPU_FLAGS_X86" not in rust.stdout
    assert "cpu_flags_x86" not in rust.stdout


def test_pv_marks_use_changes_against_the_installed_version(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real output_helpers.py::_create_use_string, for an entry that
    replaces an installed one (is_new=False), diffs each flag against the
    installed version's own recorded USE/IUSE and appends `*` (value
    changed) / `%` (flag newly in IUSE); an unchanged flag is omitted
    entirely. dev-libs/upgradeusepkg was installed at 1.0 with
    IUSE="+keep change drop" / USE="keep change"; its 2.0 ebuild has
    IUSE="+keep -change +added":

      - keep:   on, was on         -> omitted (unchanged)
      - change: was on, now off    -> -change*
      - added:  on, not in old IUSE -> added%*
      - drop:   removed from IUSE   -> not shown (no --all-flags)
    """
    args = ["--pretend", "-v", "--update", "dev-libs/upgradeusepkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild  U] dev-libs/upgradeusepkg-2.0 (upgrade from 1.0)  USE="added%* -change*"',
        "",
        "Total: 1 package (1 upgrade), Size of downloads: 0 KiB",
    ]

    # A New install has no installed side -> no markers, every flag plain.
    new = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/useflagpkg"], fixture_env
    )
    assert new.stdout.splitlines()[0] == (
        '[ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"'
    )


def test_use_expand_implicit_flag_is_valid_iuse_even_when_unlisted(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """PORTING/fixtures/repo/profiles/base/make.defaults declares
    USE_EXPAND_IMPLICIT="ELIBC", USE_EXPAND_VALUES_ELIBC="glibc musl",
    ELIBC="glibc" -- real config.py's _calc_iuse_effective: elibc_glibc /
    elibc_musl become valid *implicit* IUSE for every package (EAPI 5+
    pkg.iuse.is_valid_flag). dev-libs/implicitiusepkg RDEPENDs
    implicitiuseprov[elibc_glibc]; implicitiuseprov never lists
    elibc_glibc in its own IUSE, and elibc_glibc is enabled globally
    (ELIBC="glibc"), so the dep resolves. dev-libs/implicitiusepkgmusl's
    own [elibc_musl] dep is valid but unsatisfiable (elibc_musl not
    enabled), so it's reported as an unresolvable dependency."""
    ok = _run([str(emerge_binary)], ["--pretend", "dev-libs/implicitiusepkg"], fixture_env)
    ok_py = _run(emerge_pretend_python, ["--pretend", "dev-libs/implicitiusepkg"], fixture_env)
    assert ok.returncode == 0
    assert ok.stdout == ok_py.stdout
    assert ok.stderr == ok_py.stderr
    assert ok.stdout == (
        "[ebuild  N] dev-libs/implicitiusepkg-1.0\n"
        "[ebuild  N] dev-libs/implicitiuseprov-1.0\n"
    )

    bad = _run([str(emerge_binary)], ["--pretend", "dev-libs/implicitiusepkgmusl"], fixture_env)
    bad_py = _run(emerge_pretend_python, ["--pretend", "dev-libs/implicitiusepkgmusl"], fixture_env)
    assert bad.returncode == 0
    assert bad.stdout == bad_py.stdout
    assert bad.stderr == bad_py.stderr
    assert bad.stdout == "[ebuild  N] dev-libs/implicitiusepkgmusl-1.0\n"
    assert '!!! no visible ebuild for dependency "dev-libs/implicitiuseprov"' in bad.stderr


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
        '[ebuild  N] dev-libs/stableusepkg-1.0  USE="(-maskflag) (stableforceflag)"',
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 2 packages (2 new), Size of downloads: 0 KiB",
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
    # `~` bracket marker: unstableusepkg (KEYWORDS="~amd64") is visible
    # only via a "dev-libs/unstableusepkg ~amd64" package.accept_keywords
    # entry -- a testing keyword for our own arch (real gen_mask_str).
    assert result.stdout == (
        '[ebuild  N ~] dev-libs/unstableusepkg-1.0  USE="maskflag -stableforceflag"\n'
        "\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n"
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
    same file -- it must resolve completely normally (visible, matched),
    proving -atom removal actually took effect rather than the mask
    lingering. A bare top-level atom with no other flags reports a plain
    reinstall (real portage's own "selective" gap -- see resolve_pretend's
    own doc comment, portage-repo), not "already installed"."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/samepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  r] dev-libs/samepkg-1.0"


def test_license_eula_style_group_is_masked_by_the_real_default_accept_license(
    emerge_binary, fixture_env
):
    """Neither the fixture profile chain nor make.conf sets
    ACCEPT_LICENSE at all -- real portage's own "* -@EULA" default
    applies, and PORTING/fixtures/repo/profiles/base/license_groups
    defines EULA="SomeEula", so dev-libs/eulapkg's own
    LICENSE="SomeEula" is masked, same "no ebuilds to satisfy" outcome
    package.mask already produces."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/eulapkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip() == 'emerge: there are no ebuilds to satisfy "dev-libs/eulapkg".'
    )


def test_license_any_of_group_is_visible_via_the_accepted_alternative(
    emerge_binary, fixture_env
):
    """dev-libs/anyoflicensepkg's own LICENSE="|| ( GPL-2 SomeEula )" --
    GPL-2 is accepted via the real default's own "*" token, so the ||
    group is satisfied even though SomeEula alone would be masked."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/anyoflicensepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/anyoflicensepkg-1.0"


def test_license_package_license_unmasks_an_otherwise_eula_masked_package(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/etc/portage/package.license accepts SomeEula for
    dev-libs/packagelicensepkg specifically, despite the same global
    "* -@EULA" default that masks dev-libs/eulapkg above."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/packagelicensepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/packagelicensepkg-1.0"


def test_cross_repo_profile_parent_lets_an_overlay_license_groups_join_the_chain(
    emerge_binary, fixture_env
):
    """PORTING/fixtures/repo/profiles/default/parent's own third entry,
    "overlay:crossrepo-parent", is real portage's cross-repo profile
    parent syntax (LocationsManager._expand_parent_colon) -- it must
    resolve to PORTING/fixtures/overlay/profiles/crossrepo-parent, whose
    own license_groups extends EULA with "CrossRepoNonfree" (on top of
    the main repo's own "SomeEula" member, proving the two stack rather
    than one replacing the other). dev-libs/crossrepolicensepkg's own
    LICENSE="CrossRepoNonfree" is masked by the real default
    "* -@EULA" only if that overlay-level license_groups entry actually
    got read as part of the active chain -- exactly the mechanism this
    slice unlocks (an overlay's own profiles/license_groups previously
    couldn't join the chain at all)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/crossrepolicensepkg"], fixture_env
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/crossrepolicensepkg".'
    )


def test_license_use_conditional_visible_when_flag_off_masked_when_forced_on(
    emerge_binary, fixture_env
):
    """dev-libs/uselicensepkg's own LICENSE="GPL-2 nonfreeflag? (
    SomeEula )" -- visible with nonfreeflag off (the default); its
    sibling dev-libs/uselicensepkgforced has the identical LICENSE, but
    PORTING/fixtures/etc/portage/package.use forces nonfreeflag on for
    it specifically, activating the conditional and masking it via the
    same real default that masks dev-libs/eulapkg."""
    off = _run([str(emerge_binary)], ["--pretend", "dev-libs/uselicensepkg"], fixture_env)
    assert off.returncode == 0
    assert off.stdout.strip() == "[ebuild  N] dev-libs/uselicensepkg-1.0"

    forced_on = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/uselicensepkgforced"], fixture_env
    )
    assert forced_on.returncode == 1
    assert forced_on.stdout == ""
    assert (
        forced_on.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/uselicensepkgforced".'
    )


def test_properties_default_star_accepts_a_declared_property(emerge_binary, fixture_env):
    """dev-libs/propertiespkg's own PROPERTIES="live" is visible under
    the real default ACCEPT_PROPERTIES=* (from real cnf/make.globals,
    replicated as a hardcoded fallback -- neither the fixture profile
    chain nor make.conf sets ACCEPT_PROPERTIES at all)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/propertiespkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/propertiespkg-1.0"


def test_package_properties_narrows_acceptance_for_one_package(emerge_binary, fixture_env):
    """PORTING/fixtures/etc/portage/package.properties revokes
    "interactive" for dev-libs/interactivepkg specifically ("-interactive"
    layered on top of the permissive global "*" default still narrows
    that one package's own effective accept set), despite the same real
    default that leaves dev-libs/propertiespkg (above) visible."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/interactivepkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/interactivepkg".'
    )


def test_package_accept_restrict_narrows_acceptance_for_one_package(emerge_binary, fixture_env):
    """PORTING/fixtures/etc/portage/package.accept_restrict revokes
    "bindist" for dev-libs/restrictedpkg specifically -- same "-token
    narrows despite a permissive global default" mechanism as
    package.properties above, just for RESTRICT instead of PROPERTIES."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/restrictedpkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/restrictedpkg".'
    )


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


def test_package_accept_keywords_negation_revokes_a_globally_accepted_keyword(
    emerge_binary, fixture_env
):
    """dev-libs/keywordrevokedpkg is stable amd64 (globally accepted),
    but PORTING/fixtures/etc/portage/package.accept_keywords has a
    "dev-libs/keywordrevokedpkg -amd64" entry that revokes it
    specifically -- real KeywordsManager._getEgroups folds "-token"
    removals over the combined global+package keyword list, not just
    unions everything a matching entry ever mentions."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/keywordrevokedpkg"], fixture_env
    )
    assert result.returncode == 1
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/keywordrevokedpkg".'
    )


def test_package_accept_keywords_star_accepts_any_stable_keyword(emerge_binary, fixture_env):
    """dev-libs/starkeywordpkg is KEYWORDS="arm64" -- not globally
    accepted, and not otherwise mentioned anywhere -- but
    PORTING/fixtures/etc/portage/package.accept_keywords has a
    "dev-libs/starkeywordpkg *" entry, real portage's own "accept any
    stable keyword" wildcard (distinct from "**", which additionally
    accepts an empty KEYWORDS)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/starkeywordpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/starkeywordpkg-1.0"


def test_package_accept_keywords_tilde_star_accepts_any_testing_keyword(
    emerge_binary, fixture_env
):
    """dev-libs/tildestarkeywordpkg is KEYWORDS="~arm64" (testing-only),
    made visible by a "dev-libs/tildestarkeywordpkg ~*" entry -- real
    portage's own "accept any testing keyword" wildcard. "*" alone
    would NOT have accepted this (it only ever covers stable-classified
    keywords), proving the two wildcards are genuinely distinct, not
    just differently-spelled synonyms."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/tildestarkeywordpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/tildestarkeywordpkg-1.0"


def test_package_accept_keywords_bare_atom_implicitly_grants_tilde_arch(
    emerge_binary, fixture_env
):
    """dev-libs/bareacceptkeywordspkg is KEYWORDS="~amd64" (testing-only,
    not covered by the global ACCEPT_KEYWORDS="amd64"), made visible by
    a bare "dev-libs/bareacceptkeywordspkg" package.accept_keywords entry
    -- no keyword tokens at all. Real accept_keywords_defaults
    (KeywordsManager.__init__/getPKeywords) gives a bare atom an
    implicit "~" + every plain global ACCEPT_KEYWORDS token, i.e.
    "~amd64" here -- not a no-op, the same outcome as if the user had
    written "dev-libs/bareacceptkeywordspkg ~amd64" by hand."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/bareacceptkeywordspkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/bareacceptkeywordspkg-1.0"


def test_pv_bracket_mask_marker(emerge_binary, emerge_pretend_python, fixture_env):
    """Real output.py::gen_mask_str (verbosity>1 only): the [ebuild N]
    bracket gains a one-character marker for a package pulled in despite
    not being visible via the global ACCEPT_KEYWORDS alone --

      - dev-libs/bareacceptkeywordspkg (~amd64 via a bare
        package.accept_keywords entry): '~' (a testing keyword for our
        own arch, real get_keyword_mask "unstable")
      - dev-libs/tildestarkeywordpkg (~arm64 via "~*"): '*' (a different
        arch, real "missing")
      - dev-libs/maskedandunmaskedpkg (package.mask'd then unmask'd): '#'
        (real isHardMasked, ignores package.unmask, wins first)

    Only with -v; a plain `emerge -p` shows no marker, and a plain
    stable-amd64 package (dev-libs/newpkg) shows none even with -v."""
    for pkg, marker in [
        ("bareacceptkeywordspkg", "~"),
        ("tildestarkeywordpkg", "*"),
        ("maskedandunmaskedpkg", "#"),
    ]:
        v = _run([str(emerge_binary)], ["--pretend", "-v", f"dev-libs/{pkg}"], fixture_env)
        vp = _run(emerge_pretend_python, ["--pretend", "-v", f"dev-libs/{pkg}"], fixture_env)
        assert v.returncode == 0
        assert v.stdout == vp.stdout, pkg
        assert v.stdout.splitlines()[0] == f"[ebuild  N {marker}] dev-libs/{pkg}-1.0", pkg
        # No -v -> no marker.
        p = _run([str(emerge_binary)], ["--pretend", f"dev-libs/{pkg}"], fixture_env)
        assert p.stdout.splitlines()[0] == f"[ebuild  N] dev-libs/{pkg}-1.0", pkg

    v = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/newpkg"], fixture_env)
    assert v.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 1 package (1 new), Size of downloads: 0 KiB",
    ]


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
    at its off-by-default). In `-pv`, real `_create_use_string` wraps a
    force-enabled / mask-disabled flag in `( … )` (`self.forced_flags =
    pkg.use.force | pkg.use.mask`): `(forceflag)` and `(-maskflag)`, but
    `-specflag` plain since its mask was cancelled by the more-specific
    `-specflag` entry."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/pkgusemaskforcepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == (
        '[ebuild  N] dev-libs/pkgusemaskforcepkg-1.0  USE="(forceflag) (-maskflag) -specflag"\n'
        "\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n"
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


def test_overlay_own_package_mask_hides_only_the_overlay_copy(emerge_binary, fixture_env):
    """dev-libs/overlaymaskedpkg exists in both the main repo and the
    overlay; only the overlay's own profiles/package.mask masks it, with
    no explicit "::repo" constraint on the entry -- proving real
    append_repo's own auto-scoping ("::overlay") keeps the mask from
    also hiding the identically-named main-repo package."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlaymaskedpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/overlaymaskedpkg-1.0"


def test_overlay_own_package_mask_still_hides_the_explicit_overlay_atom(
    emerge_binary, fixture_env
):
    """An explicit "::overlay" atom constraint must still hit the
    overlay's own auto-scoped mask entry."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/overlaymaskedpkg::overlay"], fixture_env
    )
    assert result.returncode == 1
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/overlaymaskedpkg::overlay".'
    )


def test_overlay_own_package_mask_does_not_affect_the_explicit_main_repo_atom(
    emerge_binary, fixture_env
):
    """An explicit "::testrepo" atom constraint is unaffected by the
    overlay's own mask -- it was auto-scoped to "::overlay", not
    "::testrepo"."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/overlaymaskedpkg::testrepo"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/overlaymaskedpkg-1.0"


def test_overlay_own_package_unmask_cancels_the_same_overlay_own_package_mask(
    emerge_binary, fixture_env
):
    """dev-libs/overlaymaskedthenunmaskedpkg is masked and then unmasked
    by two entries in the overlay's own profiles/package.mask and
    profiles/package.unmask -- both entries get the identical "::overlay"
    auto-scoping, so they must still cancel each other out."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/overlaymaskedthenunmaskedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/overlaymaskedthenunmaskedpkg-1.0"


def test_overlay_implicit_masters_inherits_the_main_repos_own_package_mask(
    emerge_binary, fixture_env
):
    """dev-libs/mastermaskedpkg exists only in the overlay repo, and is
    masked only by the MAIN repo's own profiles/package.mask -- never
    mentioned in the overlay's own package.mask at all. Real portage's
    own masters default (a repo with no explicit "masters =" implicitly
    masters the main repo) means the main repo's own mask entry is
    stacked in ahead of the overlay's own lines before "::overlay"
    scoping, so it still applies to the overlay's own copy."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/mastermaskedpkg"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/mastermaskedpkg".'
    )


def test_overlay_package_unmask_cancels_a_masters_inherited_mask(emerge_binary, fixture_env):
    """dev-libs/mastermaskedthenoverlayunmaskedpkg is masked the same
    masters-inherited way as mastermaskedpkg above, but the overlay's
    own package.unmask also names it -- both the inherited mask and the
    overlay's own unmask get the identical "::overlay" scoping, so they
    still cancel out even though the mask itself originated in the main
    repo, not the overlay."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/mastermaskedthenoverlayunmaskedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/mastermaskedthenoverlayunmaskedpkg-1.0"


def test_explicit_masters_does_not_inherit_the_main_repos_mask(emerge_binary, fixture_env):
    """Real repos.conf explicit "masters =" key (real RepoConfigLoader.
    __init__, lib/portage/repository/config.py:1229-1260), now parsed and
    resolved for the first time: the fixture repos.conf declares
    "[independentoverlay] masters = overlay", NOT the main repo. dev-libs/
    independentmastermainonlypkg exists only in independentoverlay, and
    is masked only by the MAIN repo's own profiles/package.mask -- unlike
    the implicit-default case above (mastermaskedpkg), main is NOT a
    declared master here, so that mask entry must not apply. This is the
    first fixture that actually distinguishes "explicit masters=" from
    the pre-existing implicit default -- every previously-constructible
    repo relationship happened to match "masters main" anyway."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/independentmastermainonlypkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/independentmastermainonlypkg-1.0"


def test_explicit_masters_inherits_a_non_main_declared_masters_mask(emerge_binary, fixture_env):
    """Same "masters = overlay" declaration as the sibling test above,
    the other half: dev-libs/independentmasteroverlaypkg exists only in
    independentoverlay, and is masked only by the OVERLAY repo's own
    profiles/package.mask (not the main repo's). Since overlay IS a
    declared master of independentoverlay, that mask entry does apply --
    proving an explicit, non-main masters chain is genuinely resolved and
    consulted, not just an on/off switch for the main repo alone."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/independentmasteroverlaypkg"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/independentmasteroverlaypkg".'
    )


def test_layout_conf_masters_middle_tier_and_repo_name_override(emerge_binary, fixture_env):
    """layoutmasteroverlay has NO repos.conf masters key; its own
    metadata/layout.conf declares "masters = overlay" (the middle tier,
    below repos.conf and above the implicit main-repo default) and
    "repo-name = layoutrenamed". dev-libs/layoutmasterpkg exists only
    there and is masked only by the OVERLAY repo's own
    profiles/package.mask -- so it resolves to "no ebuilds" exactly like
    the repos.conf-masters sibling above, proving the layout.conf masters
    tier feeds package.mask stacking and the overlay loads under its
    layout.conf name."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/layoutmasterpkg"], fixture_env
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: there are no ebuilds to satisfy "dev-libs/layoutmasterpkg".'
    )


def test_profiles_repo_name_is_the_canonical_name_source(emerge_binary, fixture_env):
    """repnamerepo's section is [repnamesection] but its
    profiles/repo_name says "repnamefromfile" -- so its packages carry
    ::repnamefromfile, not ::repnamesection. It's kept (not dropped for
    the section-vs-name mismatch) because its layout.conf lists
    "aliases = repnamesection". dev-libs/repnamepkg::repnamefromfile
    resolves; ::repnamesection does not (::alias atom matching is a
    documented cut)."""
    ok = _run([str(emerge_binary)], ["--pretend", "dev-libs/repnamepkg"], fixture_env)
    assert ok.returncode == 0
    assert ok.stdout.strip() == "[ebuild  N] dev-libs/repnamepkg-1.0"
    assert ok.stderr == ""

    by_file = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/repnamepkg::repnamefromfile"],
        fixture_env,
    )
    assert by_file.returncode == 0
    assert by_file.stdout.strip() == "[ebuild  N] dev-libs/repnamepkg-1.0"

    by_section = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/repnamepkg::repnamesection"],
        fixture_env,
    )
    assert by_section.returncode == 1


def test_repo_name_section_mismatch_drops_the_repo_with_a_warning(
    emerge_binary, emerge_pretend_python, tmp_path
):
    """A repo whose profiles/repo_name differs from its repos.conf
    [section] name -- and with no matching alias -- is dropped entirely
    with a "!!! Section ..." error to stderr (real config.py:1121-1136).
    Its packages then don't resolve."""
    cfg = tmp_path / "cfg"
    repo = tmp_path / "the-repo"
    (cfg / "etc/portage").mkdir(parents=True)
    (cfg / "etc/portage/repos.conf").write_text(
        "[DEFAULT]\nmain-repo = main\n\n"
        "[main]\nlocation = " + str(repo) + "\n\n"
        "[mismatched-section]\nlocation = " + str(repo) + "\n"
    )
    (repo / "profiles").mkdir(parents=True)
    (repo / "metadata/md5-cache/dev-libs").mkdir(parents=True)
    (repo / "profiles/repo_name").write_text("main\n")
    (repo / "profiles/make.defaults").write_text('ACCEPT_KEYWORDS="amd64"\n')
    (cfg / "etc/portage/make.profile").symlink_to(repo / "profiles")
    pkgdir = repo / "dev-libs/mainpkg"
    pkgdir.mkdir(parents=True)
    (pkgdir / "mainpkg-1.0.ebuild").write_text(
        'EAPI=8\nDESCRIPTION="x"\nSLOT="0"\nKEYWORDS="amd64"\n'
    )
    (repo / "metadata/md5-cache/dev-libs/mainpkg-1.0").write_text(
        "DEFINED_PHASES=-\nDEPEND=\nDESCRIPTION=x\nEAPI=8\nIUSE=\n"
        "KEYWORDS=amd64\nRDEPEND=\nSLOT=0\n_md5_=0000000000000000000000000000000\n"
    )
    env = {"PORTAGE_CONFIGROOT": str(cfg), "ROOT": str(cfg)}

    rust = _run([str(emerge_binary)], ["--pretend", "dev-libs/mainpkg"], env)
    py = _run(emerge_pretend_python, ["--pretend", "dev-libs/mainpkg"], env)
    assert rust.returncode == py.returncode == 0  # [main] still fine
    assert rust.stdout == py.stdout
    assert rust.stderr == py.stderr
    assert (
        "!!! Section 'mismatched-section' in repos.conf has name different"
        in rust.stderr
    )


def test_overlay_own_package_use_gates_a_dependency(emerge_binary, fixture_env):
    """package.mask/.unmask already read from every repo (task #40), but
    package.use/.mask/.force/.stable.mask/.stable.force were still main-
    repo-only -- real UseManager.py's own repos_with_profiles() loop
    confirms every one of these files is read from every configured
    repo, the same mechanism. dev-libs/overlayuseenablepkg exists only
    in the overlay, whose own profiles/package.use enables
    "overlayuseflag" for it with a bare atom (no "::repo" part) -- must
    get auto-scoped to "::overlay" (scope_repo_package_use_lines) so it
    pulls in dev-libs/newpkg."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/overlayuseenablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/overlayuseenablepkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_overlay_own_package_use_force_gates_a_dependency(emerge_binary, fixture_env):
    """Same proof as package.use above, for package.use.force:
    dev-libs/overlayuseforcepkg's own "overlayforceflag" is off by
    IUSE default and every other source, forced on only by the
    overlay's own profiles/package.use.force."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/overlayuseforcepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/overlayuseforcepkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_overlay_own_package_use_mask_blocks_a_dependency(emerge_binary, fixture_env):
    """Same proof as package.use/.force above, for package.use.mask:
    dev-libs/overlayusemaskpkg's own IUSE="+overlaymaskflag" defaults
    the flag on, but the overlay's own profiles/package.use.mask masks
    it back off, so its flag?-gated dependency is never pulled in."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/overlayusemaskpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == ["[ebuild  N] dev-libs/overlayusemaskpkg-1.0"]


def test_usepkg_exclude_drops_the_only_binary_candidate(emerge_binary, fixture_env):
    """dev-libs/binaryonlypkg exists only as a binary build -- --usepkg
    alone makes it eligible ([binary N]), but real depgraph.py's own
    per-candidate check ("in_usepkg_exclude = have_usepkg_exclude and
    usepkg_exclude.findAtomForPackage(pkg, ...)", "if in_usepkg_exclude
    ...: break") drops it from the binary pool entirely once
    --usepkg-exclude names it -- with no ebuild fallback, the atom
    becomes entirely unsatisfiable."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "--usepkg-exclude", "dev-libs/binaryonlypkg", "dev-libs/binaryonlypkg"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".'
    )


def test_usepkg_include_gates_binary_eligibility_both_ways(emerge_binary, fixture_env):
    """--usepkg-include's own real semantics are inverted from
    --usepkg-exclude's: "in_usepkg_include = not have_usepkg_include or
    usepkg_include.findAtomForPackage(pkg, ...)" -- once ANY
    --usepkg-include atom is given, a binary candidate must match one of
    them to stay eligible at all. A non-matching include list rejects
    dev-libs/binaryonlypkg exactly like --usepkg-exclude does; a
    matching one leaves it eligible."""
    non_matching = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--usepkg",
            "--usepkg-include",
            "dev-libs/doesnotexist-anywhere",
            "dev-libs/binaryonlypkg",
        ],
        fixture_env,
    )
    assert non_matching.returncode == 1
    assert non_matching.stdout == ""

    matching = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "--usepkg-include", "dev-libs/binaryonlypkg", "dev-libs/binaryonlypkg"],
        fixture_env,
    )
    assert matching.returncode == 0
    assert matching.stdout.splitlines() == ["[binary  N] dev-libs/binaryonlypkg-1.0"]


def test_newrepo_off_by_default_stays_already_installed(emerge_binary, fixture_env):
    """dev-libs/newrepopkg is installed with a vdb "repository" file
    recording "oldrepo", while the current best candidate for this exact
    version lives in "testrepo" instead. Without --newrepo at all, the
    mismatch is never even checked, so a --selective query (avoiding the
    unrelated "always reinstall a bare top-level atom" behavior) stays
    already-installed."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--selective", "dev-libs/newrepopkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "dev-libs/newrepopkg-1.0 is already installed; nothing to do"


def test_newrepo_triggers_a_reinstall_for_a_differing_recorded_repository(
    emerge_binary, fixture_env
):
    """Same fixture as above, but with --newrepo explicitly given: real
    depgraph.py's own "pkg.repo != inst_pkg.repo" comparison fires since
    the vdb's own recorded "oldrepo" doesn't match "testrepo", the repo
    that actually provides this version now."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--selective", "--newrepo", "dev-libs/newrepopkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/newrepopkg-1.0 (reinstall for new repository)"
    ]


def test_newrepo_does_not_fire_when_the_recorded_repository_matches(
    emerge_binary, fixture_env
):
    """dev-libs/samerepopkg's own vdb "repository" file records
    "testrepo", exactly matching the repo that currently provides this
    version -- --newrepo must not trigger a reinstall here."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--selective", "--newrepo", "dev-libs/samerepopkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "dev-libs/samerepopkg-1.0 is already installed; nothing to do"


def test_newrepo_fires_via_the_unknown_repo_sentinel_when_unrecorded(
    emerge_binary, fixture_env
):
    """dev-libs/samepkg has no vdb "repository" file at all (real portage
    predates this tracking, or a hand-installed/synthetic entry) -- real
    portage.versions._unknown_repo ("__unknown__") applies, which never
    equals a real repo name, so --newrepo still fires even though
    nothing about this package actually changed. A real, sometimes-
    surprising consequence of real portage's own comparison having no
    tolerant "missing data means unchanged" fallback the way
    --changed-slot/--changed-deps do."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--selective", "--newrepo", "dev-libs/samepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/samepkg-1.0 (reinstall for new repository)"
    ]


def test_newrepo_appears_in_json(emerge_binary, fixture_env):
    """--json's own mirror: a "reinstall" entry's own "new_repo" field,
    alongside the pre-existing changed_use/changed_deps/changed_slot/
    rebuilt_binary siblings."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--selective", "--newrepo", "--json", "dev-libs/newrepopkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    entry = payload["entries"][0]
    assert entry["outcome"] == "reinstall"
    assert entry["new_repo"] is True


def test_buildpkgonly_reports_the_merge_list_then_the_real_error(
    emerge_binary, fixture_env
):
    """Real depgraph.py's own display_problems(): the merge list is
    printed first (real _show_merge_list()), and only then does the
    real "--buildpkgonly requires all dependencies to be merged" error
    follow, on stderr -- dev-libs/dualdep (New) RDEPENDs on dev-libs/
    newpkg (also New), so both need building."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--buildpkgonly", "dev-libs/dualdep"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/dualdep-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]
    assert result.stderr.strip().splitlines() == [
        "!!! --buildpkgonly requires all dependencies to be merged.",
        "!!! Cannot merge requested packages. Merge deps and try again.",
    ]


def test_buildpkgonly_does_not_fire_when_the_dependency_is_already_installed(
    emerge_binary, fixture_env
):
    """dev-libs/buildpkgonlysatisfied (New) RDEPENDs on dev-libs/samepkg,
    which is already installed -- nothing else needs building, so real
    --buildpkgonly's own depgraph check has nothing to object to."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--buildpkgonly", "dev-libs/buildpkgonlysatisfied"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/buildpkgonlysatisfied-1.0"
    assert result.stderr == ""


def test_rebuilt_binaries_off_by_default_stays_already_installed(emerge_binary, fixture_env):
    """dev-libs/rebuiltbinarypkg is installed at 1.0 with its own vdb-
    recorded BUILD_TIME=1000; the binary index's own copy at the same
    version has BUILD_TIME=2000. Without --rebuilt-binaries at all (and
    with none of --usepkgonly/--deep/--update present to trigger the
    real auto-on default -- create_depgraph_params.py:185-193), the
    differing BUILD_TIME is never even checked, so a --selective query
    (avoiding the unrelated "always reinstall a bare top-level atom"
    behavior) stays already-installed."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "--selective", "dev-libs/rebuiltbinarypkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do"


def test_rebuilt_binaries_triggers_a_reinstall_for_a_differing_build_time(
    emerge_binary, fixture_env
):
    """Same fixture as above, but with --rebuilt-binaries explicitly
    given: real depgraph.py's own "don't care if the binary has an older
    BUILD_TIME ... this is for closely tracking a binhost" comment means
    ANY difference (2000 vs 1000, either direction) triggers a reinstall
    once no --rebuilt-binaries-timestamp cutoff is given."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "--selective", "--rebuilt-binaries", "dev-libs/rebuiltbinarypkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[binary  r] dev-libs/rebuiltbinarypkg-1.0 (reinstall for rebuilt binary)"
    ]


def test_rebuilt_binaries_timestamp_gates_the_reinstall(emerge_binary, fixture_env):
    """--rebuilt-binaries-timestamp changes the comparison from "any
    difference" to "strictly newer AND at or above this cutoff" (real
    depgraph.py: "built_timestamp > installed_timestamp and
    built_timestamp >= minimal_timestamp"). The binary's own BUILD_TIME
    is 2000: a cutoff of 3000 suppresses the reinstall (2000 < 3000),
    while a cutoff of 1500 still triggers it (2000 > 1000 installed, and
    2000 >= 1500)."""
    too_high = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--usepkg",
            "--selective",
            "--rebuilt-binaries",
            "--rebuilt-binaries-timestamp",
            "3000",
            "dev-libs/rebuiltbinarypkg",
        ],
        fixture_env,
    )
    assert too_high.returncode == 0
    assert (
        too_high.stdout.strip() == "dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do"
    )

    low_enough = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--usepkg",
            "--selective",
            "--rebuilt-binaries",
            "--rebuilt-binaries-timestamp",
            "1500",
            "dev-libs/rebuiltbinarypkg",
        ],
        fixture_env,
    )
    assert low_enough.returncode == 0
    assert low_enough.stdout.splitlines() == [
        "[binary  r] dev-libs/rebuiltbinarypkg-1.0 (reinstall for rebuilt binary)"
    ]


def test_rebuilt_binaries_auto_enables_under_usepkgonly_deep_update(emerge_binary, fixture_env):
    """The real, non-obvious default-resolution asymmetry
    (create_depgraph_params.py:185-193): --rebuilt-binaries auto-enables
    even with no explicit flag at all, but only when --usepkgonly, bare
    --deep (no explicit number), and --update are ALL given together. A
    bounded --deep 3 does NOT count as the real "deep is True" bare
    form, so it must NOT auto-enable."""
    auto_on = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--usepkgonly",
            "--deep",
            "--update",
            "--selective",
            "dev-libs/rebuiltbinarypkg",
        ],
        fixture_env,
    )
    assert auto_on.returncode == 0
    assert auto_on.stdout.splitlines() == [
        "[binary  r] dev-libs/rebuiltbinarypkg-1.0 (reinstall for rebuilt binary)"
    ]

    bounded_deep = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--usepkgonly",
            "--deep",
            "3",
            "--update",
            "--selective",
            "dev-libs/rebuiltbinarypkg",
        ],
        fixture_env,
    )
    assert bounded_deep.returncode == 0
    assert (
        bounded_deep.stdout.strip()
        == "dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do"
    )


def test_use_dep_equal_parent_matches_when_parent_flag_is_enabled(emerge_binary, fixture_env):
    """PMS 8.3.4's "opt=" conditional use-dep: "the flag must be enabled
    if the flag is enabled for the package with the dependency, or
    disabled otherwise" -- real Atom.evaluate_conditionals
    (lib/portage/dep/__init__.py:1387), confirmed by reading it, applied
    at real use_reduce's own per-token integration point
    (__init__.py:1045-1046). dev-libs/useeqparentonpkg's own
    IUSE="+eqflag" defaults it ON; its RDEPEND
    "dev-libs/useeqchildpkg[eqflag=]" evaluates to "[eqflag]" (must be
    enabled), which matches dev-libs/useeqchildpkg's own default-on
    eqflag, so the dependency resolves normally."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/useeqparentonpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/useeqparentonpkg-1.0",
        "[ebuild  N] dev-libs/useeqchildpkg-1.0",
    ]
    assert result.stderr == ""


def test_use_dep_equal_parent_mismatches_when_parent_flag_is_disabled(emerge_binary, fixture_env):
    """Same mechanism as the sibling test above, the other half of
    "opt="'s own truth table: dev-libs/useeqparentoffpkg's own
    IUSE="eqflag" (no "+") defaults it OFF, so the identical
    "[eqflag=]" use-dep now evaluates to "[-eqflag]" (must be disabled)
    -- which does NOT match dev-libs/useeqchildpkg's own default-on
    eqflag, so the dependency is reported unresolvable (same "genuinely
    unresolvable, but doesn't fail the whole call" precedent
    dev-libs/missingdep already established), not silently dropped or
    incorrectly satisfied. Also carries *two* --autounmask-use
    suggestions: Part A's own plain per-candidate flip (flip the child's
    own eqflag off), and Part B's own opt=-aware parent flip (flip the
    parent's own eqflag on instead, which would re-evaluate the RDEPEND's
    own "[eqflag=]" to "[eqflag]", satisfied by the child's own
    default-on eqflag) -- both real, independent fixes for the same
    mismatch, matching real portage's own missing_use_reasons allowing
    both at once."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/useeqparentoffpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "[ebuild  N] dev-libs/useeqparentoffpkg-1.0"
    assert result.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/useeqchildpkg"\n'
        '!!! note: dev-libs/useeqchildpkg-1.0 exists but its USE flags don\'t satisfy '
        'this atom; --autounmask-use suggests adding "=dev-libs/useeqchildpkg-1.0 '
        '-eqflag" to package.use\n'
        "!!! note: dev-libs/useeqparentoffpkg-1.0's own USE flags need to change to "
        'satisfy this dependency; --autounmask-use suggests adding '
        '"=dev-libs/useeqparentoffpkg-1.0 eqflag" to package.use'
    )


def test_tree_indents_a_diamond_dependency_and_shows_it_once(emerge_binary, fixture_env):
    """--tree/-t: pilot-specific simplified indentation (real
    output_helpers.py's own _tree_display needs a genuine merge
    scheduler this pilot doesn't have -- see pretend.rs's own print_tree
    docstring for the full grounding). dev-libs/diamond's own two
    children (shared-a, shared-b) both RDEPEND on dev-libs/common -- the
    diamond dependency must be shown exactly once, nested under whichever
    parent's own subtree reaches it first (shared-a, the alphabetically
    first child), not silently duplicated under both and not silently
    dropped either -- real _unordered_tree_display's own "seen_nodes"
    behavior, ported exactly."""
    result = _run([str(emerge_binary)], ["--pretend", "--tree", "dev-libs/diamond"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/diamond-1.0",
        "[ebuild  N]   dev-libs/shared-a-1.0",
        "[ebuild  N]     dev-libs/common-1.0",
        "[ebuild  N]   dev-libs/shared-b-1.0",
    ]


def test_tree_unordered_display_preserves_discovery_order(emerge_binary, fixture_env):
    """--unordered-display (only meaningful together with --tree): real
    portage's own man page wording -- does NOT sort the tree in merging
    order. dev-libs/treeorderpkg's own RDEPEND deliberately lists its
    two children in reverse-alphabetical order
    ("dev-libs/ztreechild dev-libs/atreechild"). The default (--tree
    alone) sorts children alphabetically, this pilot's own deterministic
    stand-in for real portage's genuine merge-order sort (no scheduler
    exists to be more faithful than that) -- --unordered-display instead
    preserves the RDEPEND string's own literal order, using
    already-existing BFS discovery-order data, not sorted at all."""
    ordered = _run(
        [str(emerge_binary)], ["--pretend", "--tree", "dev-libs/treeorderpkg"], fixture_env
    )
    assert ordered.returncode == 0
    assert ordered.stdout.splitlines() == [
        "[ebuild  N] dev-libs/treeorderpkg-1.0",
        "[ebuild  N]   dev-libs/atreechild-1.0",
        "[ebuild  N]   dev-libs/ztreechild-1.0",
    ]

    unordered = _run(
        [str(emerge_binary)],
        ["--pretend", "--tree", "--unordered-display", "dev-libs/treeorderpkg"],
        fixture_env,
    )
    assert unordered.returncode == 0
    assert unordered.stdout.splitlines() == [
        "[ebuild  N] dev-libs/treeorderpkg-1.0",
        "[ebuild  N]   dev-libs/ztreechild-1.0",
        "[ebuild  N]   dev-libs/atreechild-1.0",
    ]


def test_tree_onlydeps_suppresses_only_the_root_line(emerge_binary, fixture_env):
    """--tree combined with --onlydeps: the same suppression rule flat
    mode already has (a directly-requested top-level atom's own line is
    hidden, its dependencies print normally) applies per-node in tree
    mode too -- only the root's own line disappears, its children still
    render, at their own normal indent."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--tree", "--onlydeps", "dev-libs/diamond"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N]   dev-libs/shared-a-1.0",
        "[ebuild  N]     dev-libs/common-1.0",
        "[ebuild  N]   dev-libs/shared-b-1.0",
    ]


def test_columns_right_aligns_the_version_into_a_fixed_column(emerge_binary, fixture_env):
    """--columns: real _set_root_columns's own layout (this pilot's own
    port, see columns_line's docstring) -- category/package with no
    version, padded to columnwidth - 60, then "[version]" padded to
    columnwidth - 30. COLUMNWIDTH is pinned in the test's own env (70,
    picked small enough for a readable exact-string assertion) rather
    than relying on the real default of 130."""
    env = dict(fixture_env)
    env["COLUMNWIDTH"] = "70"
    result = _run(
        [str(emerge_binary)], ["--pretend", "--columns", "dev-libs/newpkg"], env
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N] dev-libs/newpkg [1.0]       \n"


def test_columns_shows_both_new_and_old_version_for_an_upgrade(emerge_binary, fixture_env):
    """An Upgrade's own "oldbest" column (real pkg_info.oldbest_list) is
    the installed version in brackets -- the same information the
    non-columns format's own "(upgrade from X)" parenthetical carries,
    just repositioned into a fixed trailing column instead."""
    env = dict(fixture_env)
    env["COLUMNWIDTH"] = "70"
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--columns", "dev-libs/upgradepkg"],
        env,
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  U] dev-libs/upgradepkg [2.0]   [1.0]\n"


def test_columns_and_tree_together_is_a_usage_error(emerge_binary, fixture_env):
    """Real actions.py: "can't specify both of --tree and --columns" --
    this pilot's own CLI-usage-error convention (exit 2, stderr) rather
    than real portage's own literal exit 1/stdout, matching every other
    CLI-usage error this pilot already reports."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--tree", "--columns", "dev-libs/newpkg"],
        fixture_env,
    )
    assert result.returncode == 2
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: can\'t specify both of "--tree" and "--columns".'
    )


def test_columns_columnwidth_falls_back_to_default_on_an_unparsable_value(
    emerge_binary, fixture_env
):
    """An unparsable COLUMNWIDTH warns (a fixed, pilot-authored message,
    not either language's own raw parse-error text -- see
    columnwidth_from_env's own docstring for why) and falls back to the
    real default of 130, rather than treating it as a hard error."""
    env = dict(fixture_env)
    env["COLUMNWIDTH"] = "notanumber"
    result = _run(
        [str(emerge_binary)], ["--pretend", "--columns", "dev-libs/newpkg"], env
    )
    assert result.returncode == 0
    assert result.stderr.strip() == '!!! Unable to parse COLUMNWIDTH="notanumber"'
    assert result.stdout == (
        "[ebuild  N] dev-libs/newpkg                                            "
        "[1.0]                        \n"
    )


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

    # --tree: *both* slots nest under the parent that pulled them in.
    # (Regression: a destructive required_by merge used to hand the owner
    # to only the first slot's entry, so the second fell through to the
    # flush-left safety net.)
    tree = _run(
        [str(emerge_binary)], ["--pretend", "--tree", "dev-libs/multislotparent"], fixture_env
    )
    assert tree.returncode == 0
    assert tree.stdout.splitlines() == [
        "[ebuild  N] dev-libs/multislotparent-1.0",
        "[ebuild  N]   dev-libs/multislotpkg-1.0",
        "[ebuild  N]   dev-libs/multislotpkg-2.0",
    ]

    # --json: every slot's entry names the same owner in required_by.
    payload = json.loads(
        _run(
            [str(emerge_binary)], ["--pretend", "--json", "dev-libs/multislotparent"], fixture_env
        ).stdout
    )
    slots = [e for e in payload["entries"] if e["package"] == "multislotpkg"]
    assert len(slots) == 2
    for e in slots:
        assert e["required_by"] == [{"category": "dev-libs", "package": "multislotparent"}]


def test_new_slot_install_renders_the_S_bracket_column(emerge_binary, fixture_env):
    """dev-libs/newslotpkg-1.0 (SLOT 0) is installed; -2.0 (SLOT 1) is
    not. Requesting :1 (or the bare atom, non-selective) resolves -2.0
    into a slot the package isn't installed in -- real
    output.py::_get_installed_best's new_slot flag: an "S" right after
    the "N" code letter (rendered with plain -p, not only -v), and NOT
    an "(upgrade from 1.0)" off the unrelated slot-0 install."""
    for atom in ("dev-libs/newslotpkg:1", "dev-libs/newslotpkg"):
        result = _run([str(emerge_binary)], ["--pretend", atom], fixture_env)
        assert result.returncode == 0
        assert result.stdout.splitlines() == [
            "[ebuild  NS] dev-libs/newslotpkg-2.0",
        ], atom
        assert "upgrade from" not in result.stdout, atom

    # The slot that IS installed stays an in-slot outcome (no S).
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/newslotpkg:0"], fixture_env)
    assert result.returncode == 0
    assert "NS]" not in result.stdout


def test_interactive_bracket_column(emerge_binary, fixture_env):
    """Real output.py:833: `if "interactive" in pkg.properties and
    pkg.operation == "merge": attr_display.interactive = True`, rendered
    as `I` before the N/r code letter (unconditional, like the S column).
    pkg.properties is PROPERTIES after real USE-conditional evaluation."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/interactivemergepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  IN] dev-libs/interactivemergepkg-1.0",
    ]

    # An installed interactive package reinstalls as [ebuild Ir].
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/interactiveinstalledpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  Ir] dev-libs/interactiveinstalledpkg-1.0",
    ]

    # `gtk? ( interactive )` with gtk disabled -> the conditional gates
    # the interactive token out, no I.
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/interactivecondpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/interactivecondpkg-1.0",
    ]


def test_fetch_restrict_bracket_column(emerge_binary, fixture_env):
    """Real output.py:633: for a merge-bound ebuild whose evaluated
    RESTRICT contains `fetch`, attr_display.fetch_restrict; then
    fetch_restrict_satisfied if `not getfetchsizes(only_restricted=True)`
    -- every SRC_URI distfile already in DISTDIR at its Manifest size.
    __str__ renders green `f` (satisfied) / red `F` (missing), after the
    S column. fixture_env points DISTDIR at PORTING/fixtures/distfiles/,
    which holds frs-1.0.tar.gz (matching frs's Manifest) but not
    frm-1.0.tar.gz."""
    ok = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/fetchrestrictsatisfiedpkg"], fixture_env
    )
    assert ok.returncode == 0
    assert ok.stdout.splitlines() == [
        "[ebuild  Nf] dev-libs/fetchrestrictsatisfiedpkg-1.0",
    ]

    missing = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/fetchrestrictmissingpkg"], fixture_env
    )
    assert missing.returncode == 0
    assert missing.stdout.splitlines() == [
        "[ebuild  NF] dev-libs/fetchrestrictmissingpkg-1.0",
    ]

    # Point DISTDIR somewhere empty -> even the pre-seeded one is now F.
    empty_env = {**fixture_env, "DISTDIR": "/var/empty/no-such-distdir"}
    both_missing = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/fetchrestrictsatisfiedpkg"], empty_env
    )
    assert both_missing.stdout.splitlines() == [
        "[ebuild  NF] dev-libs/fetchrestrictsatisfiedpkg-1.0",
    ]

    # A package with no RESTRICT=fetch has no f/F column at all.
    plain = _run([str(emerge_binary)], ["--pretend", "dev-libs/newpkg"], fixture_env)
    assert plain.stdout.splitlines() == ["[ebuild  N] dev-libs/newpkg-1.0"]


def test_pv_totals_summary_line(emerge_binary, fixture_env):
    """Real output.py::display: `if self.conf.verbosity == 3:
    self.print_verbose(...)` -> `writemsg_stdout(f"\\n{self.counters}\\n")`
    -- the trailing `Total: N packages (...)` line, only under `-v`,
    including `, Size of downloads: N KiB` (real `_calc_size` /
    `localized_size`, KiB-only, no locale grouping) and the `Fetch
    Restriction: N package[s][ (M unsatisfied)]` line. Ported minus the
    `Conflict:` line's `(N unsatisfied)`/`(all satisfied)` suffix (this
    pilot resolves no blocker)."""
    # Plain `-p` (no -v): no Total line at all.
    plain = _run([str(emerge_binary)], ["--pretend", "dev-libs/newpkg"], fixture_env)
    assert "Total:" not in plain.stdout

    def totals(args):
        r = _run([str(emerge_binary)], ["--pretend", "-v", *args], fixture_env)
        assert r.returncode == 0
        return r.stdout.splitlines()[-1]

    assert totals(["dev-libs/newpkg"]) == "Total: 1 package (1 new), Size of downloads: 0 KiB"
    assert totals(["--update", "dev-libs/upgradepkg"]) == "Total: 1 package (1 upgrade), Size of downloads: 0 KiB"
    assert totals(["dev-libs/newslotpkg:1"]) == "Total: 1 package (1 in new slot), Size of downloads: 0 KiB"
    assert totals(["dev-libs/interactivemergepkg"]) == "Total: 1 package (1 new, 1 interactive), Size of downloads: 0 KiB"
    assert totals(["--usepkg", "dev-libs/binaryonlypkg"]) == "Total: 1 package (1 new, 1 binary), Size of downloads: 0 KiB"
    assert totals(["dev-libs/multislotparent"]) == "Total: 3 packages (3 new), Size of downloads: 0 KiB"

    # Nothing to install -> Total: 0 packages, Size of downloads: 0 KiB, no parenthetical.
    installed = _run(
        [str(emerge_binary)], ["--pretend", "-v", "-n", "dev-libs/samepkg"], fixture_env
    )
    assert installed.stdout.splitlines()[-1] == "Total: 0 packages, Size of downloads: 0 KiB"

    # A blocker adds a trailing Conflict: line (no satisfied/unsatisfied
    # suffix -- a documented cut).
    blk = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/blockerpkg"], fixture_env)
    assert blk.stdout.splitlines()[-2:] == [
        "Total: 1 package (1 new), Size of downloads: 0 KiB",
        "Conflict: 1 block",
    ]

    # RESTRICT=fetch: `Size of downloads` counts the Manifest bytes of
    # distfiles not in DISTDIR (frm's 12345 -> ceil(12345/1024) = 13
    # KiB), and a `Fetch Restriction:` line appears -- `(1 unsatisfied)`
    # when the file is missing, no suffix when it's already present.
    frm = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/fetchrestrictmissingpkg"], fixture_env
    )
    assert frm.stdout.splitlines()[-2:] == [
        "Total: 1 package (1 new), Size of downloads: 13 KiB",
        "Fetch Restriction: 1 package (1 unsatisfied)",
    ]
    frs = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "dev-libs/fetchrestrictsatisfiedpkg"],
        fixture_env,
    )
    assert frs.stdout.splitlines()[-2:] == [
        "Total: 1 package (1 new), Size of downloads: 0 KiB",
        "Fetch Restriction: 1 package",
    ]


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
    len(entries) == 1 special case). --noreplace restores "already
    installed" for a bare top-level atom (see resolve_pretend's own
    doc comment, portage-repo, on real portage's own "selective" gap --
    without it, a bare top-level atom reports a plain reinstall
    instead)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "dev-libs/samepkg", "dev-libs/samepkg"],
        fixture_env,
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
        '[ebuild  N] dev-libs/packageusedisablepkg-1.0  USE="-foo"',
        "",
        "Total: 1 package (1 new), Size of downloads: 0 KiB",
    ]


def test_verbose_on_a_package_with_no_iuse_shows_no_use_line(emerge_binary, fixture_env):
    """dev-libs/newpkg declares no IUSE at all -- -v must not print an
    empty USE="" line, matching real portage's own "nothing to show"
    behavior (_create_use_string returns "" when there's nothing to
    join)."""
    result = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 1 package (1 new), Size of downloads: 0 KiB",
    ]


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
        "--onlydeps/-o, --update/-u, --deep/-D, --exclude/-X, --deselect/-W, --unmerge/-C, --depclean/-c, --prune/-P, --with-bdeps, --with-bdeps-auto, --changed-deps, --changed-deps-report, --changed-slot, --with-test-deps, --noreplace/-n, --selective, and --help/-h are implemented so far; see PROMPT.md)"
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
        "   -X, --exclude ATOMS  leave any matching already-installed package as-is, and never install a matching new one (repeatable, space-separated)\n"
        "   -W, --deselect  a standalone action: report which world favorites ATOMS would remove (never writes; requires --pretend)\n"
        "       --with-bdeps y|n  include (y, the default) or skip (n) DEPEND/BDEPEND when --deep walks an already-installed package's own dependencies\n"
        '       --with-bdeps-auto y|n  changes the *default* --with-bdeps value (only when --with-bdeps itself isn\'t given) -- n makes it default to n instead of the real "auto" (y here)\n'
        "       --changed-deps[=y|n]  reinstall an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's\n"
        "       --changed-deps-report[=y|n]  report (without reinstalling) an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's; silent if --changed-deps is also given\n"
        "       --changed-slot[=y|n]  reinstall an already-installed package whose own vdb-recorded SLOT differs from the current ebuild's\n"
        '       --with-test-deps[=y|n]  also pull in a top-level atom\'s own test?-gated dependencies, if it has a "test" USE flag not already enabled\n'
        "   -n, --noreplace  a directly-named, already-installed, still-satisfying atom is left as-is (real portage's own default without this needs --update/--newuse/--changed-use/--changed-deps/--changed-slot/--selective to get the same result)\n"
        '       --selective[=y|n]  identical to --noreplace; "n" explicitly cancels it even if another flag above would otherwise set it\n'
        "   -h, --help      show this message and exit\n"
        "       --json      dump the whole resolved graph as one line of JSON instead "
        "of the lines above (pilot-specific, not a real emerge option)\n"
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
    must be silently skipped, not mishandled (a "@"-prefixed line in the
    plain world FILE itself really does fail real portage's own
    validation too -- see _read_world_atoms's own docstring; nested sets
    live in the separate world_sets file, exercised below in this same
    test) -- proving @world expansion feeds the exact same multi-atom/
    recursion machinery every other invocation already uses, not a
    separate code path. PORTING/fixtures/var/lib/portage/world_sets
    lists "@nestedtestset" (PORTING/fixtures/etc/portage/sets/
    nestedtestset), which itself contributes dev-libs/nestedsetpkg
    directly (installed since the `-pC` set-protection slice, so it
    shows as "already installed; nothing to do" here rather than
    "[ebuild N]") and nests a further "@innernestedset" reference
    (contributing dev-libs/innernestedsetpkg, and -- proving the cycle
    guard -- referencing "@nestedtestset" right back without looping
    forever or erroring). --update is added purely so upgradepkg's own
    dependency-level entry actually upgrades (see the --update contract
    tests) rather than staying silently AlreadyInstalled -- unrelated to
    what this test itself is about."""
    result = _run([str(emerge_binary)], ["--pretend", "--update", "@world"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/withdeps-1.0",
        "dev-libs/nestedsetpkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/innernestedsetpkg-1.0",
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
        "dev-libs/nestedsetpkg-1.0 is already installed; nothing to do",
        "[ebuild  N] dev-libs/innernestedsetpkg-1.0",
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


def test_world_sets_unresolvable_set_name_is_a_real_error(emerge_binary, fixture_env, tmp_path):
    """A "@name" listed in world_sets with no matching
    etc/portage/sets/<name> file is a real, immediate error (real
    PackageSetNotFound) -- unlike a missing world/world_sets *file*
    itself (a real, valid "nothing selected" state), a name explicitly
    listed but unresolvable is a genuine configuration error. Only ROOT
    is redirected (for a throwaway world_sets); PORTAGE_CONFIGROOT stays
    on the real fixtures, whose own etc/portage/sets/ has no
    "doesnotexist" file."""
    world_lib = tmp_path / "var" / "lib" / "portage"
    world_lib.mkdir(parents=True)
    (world_lib / "world_sets").write_text("@doesnotexist\n")
    env = dict(fixture_env)
    env["ROOT"] = str(tmp_path)
    result = _run([str(emerge_binary)], ["--pretend", "@world"], env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == "emerge: set 'doesnotexist' not found"


def _deselect_root(tmp_path):
    """A minimal, self-contained ROOT with its own world file and vdb --
    real action_deselect (lib/_emerge/actions.py) only ever touches the
    world file and vardb, never repos/config at all, so --deselect's own
    tests stay fully isolated from the shared PORTING/fixtures tree
    (avoiding any ripple effect on its own @world-dependent tests, which
    an added world-file entry there would otherwise cause) rather than
    reusing it. "dev-libs/foo" (world, no slot) and "dev-libs/bar:1"
    (world, slot-restricted) are both actually installed and thus
    matchable; "dev-libs/baz:2" is installed at a *different* slot (1),
    proving the slot check actually rejects a mismatch rather than
    matching on category/package alone; "dev-libs/qux" is world-listed
    but never installed; "dev-libs/notinworld" is installed but never
    world-listed; "@some-nested-set-reference" proves a "@"-prefixed
    world line is still silently skipped here too, same as @world
    expansion already does. "=dev-libs/vers-1.0" is an explicit-version
    world entry, never installed either -- since an explicit-category
    target needs no installed check at all (see run_deselect's own doc
    comment), this exercises real Atom.intersects()'s own deliberately
    narrow cpv/operator matching directly, without an installed-status
    confound: an exact "=dev-libs/vers-1.0" target matches, but neither
    a different version ("=dev-libs/vers-2.0") nor even the same
    version under a different operator (">=dev-libs/vers-1.0", which
    would actually be *satisfied* by 1.0 under a real range check) does
    -- real Atom.intersects() requires the operator itself to match
    exactly, not just range-satisfaction, per its own docstring's "TODO:
    Detect more forms of intersection". The separate world_sets file (real
    portage's own WORLD_SETS_FILE, genuinely distinct from the world
    file above) lists "@myselectedset" (matchable by
    "--deselect @myselectedset") and "@anotherselectedset" (present but
    never targeted by any test, so it must never appear in a "Would
    remove" line on its own)."""
    world = tmp_path / "var" / "lib" / "portage" / "world"
    world.parent.mkdir(parents=True)
    world.write_text(
        "dev-libs/foo\n"
        "dev-libs/bar:1\n"
        "dev-libs/baz:2\n"
        "dev-libs/qux\n"
        "=dev-libs/vers-1.0\n"
        "@some-nested-set-reference\n"
    )
    world_sets = tmp_path / "var" / "lib" / "portage" / "world_sets"
    world_sets.write_text("@myselectedset\n@anotherselectedset\n")

    def install(category, package, version, slot="0"):
        pkg_dir = tmp_path / "var" / "db" / "pkg" / category / f"{package}-{version}"
        pkg_dir.mkdir(parents=True)
        (pkg_dir / "CATEGORY").write_text(f"{category}\n")
        (pkg_dir / "SLOT").write_text(f"{slot}\n")

    install("dev-libs", "foo", "1.0")
    install("dev-libs", "bar", "1.0", slot="1")
    install("dev-libs", "baz", "1.0", slot="1")
    install("dev-libs", "notinworld", "1.0")
    return tmp_path


def _deselect_env(fixture_env, tmp_path):
    env = dict(fixture_env)
    env["ROOT"] = str(_deselect_root(tmp_path))
    return env


def _depclean_root(tmp_path):
    """A self-contained ROOT for --depclean: a small installed dependency
    graph plus its own world file. PORTAGE_CONFIGROOT stays at the shared
    fixtures (so @system = {newpkg, withdeps, systempkg} from the profile
    packages files), but only systempkg of those is installed here.

    Reachable from @world (dev-libs/dcworld) + @system (dev-libs/
    systempkg): dcworld -> dcdep -> dcsub, dcworld -[bar?]-> dccond
    (USE="bar"), and systempkg itself. dcworld also DEPENDs dcbuilddep
    and dcdep BDEPENDs dcbdep -- real `emerge --depclean` follows
    build-time deps (bdeps="auto" in remove mode), so both are kept.
    dev-libs/dcorphan (nothing needs it) and dev-libs/dcorphandep (only
    dcorphan's RDEPEND) are the cleanlist."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("dev-libs/dcworld\n")

    def install(package, rdepend="", use="", version="1.0", slot="0", depend="", bdepend=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-{version}"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text(f"{slot}\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")
        if depend:
            (d / "DEPEND").write_text(depend + "\n")
        if bdepend:
            (d / "BDEPEND").write_text(bdepend + "\n")
        if use:
            (d / "USE").write_text(use + "\n")

    install(
        "dcworld",
        rdepend="dev-libs/dcdep bar? ( dev-libs/dccond )",
        use="bar",
        depend="dev-libs/dcbuilddep",
    )
    install("dcdep", rdepend="dev-libs/dcsub", bdepend="dev-libs/dcbdep")
    install("dcsub")
    install("dccond")
    install("dcbuilddep")
    install("dcbdep")
    install("dcorphan", rdepend="dev-libs/dcorphandep")
    install("dcorphandep")
    install("systempkg")
    return tmp_path


def _depclean_env(fixture_env, tmp_path):
    env = dict(fixture_env)
    env["ROOT"] = str(_depclean_root(tmp_path))
    return env


def test_depclean_pretend_lists_orphans(emerge_binary, fixture_env, tmp_path):
    """emerge --pretend --depclean: everything nothing in @world+@system
    needs at runtime. dcorphan + its private dep dcorphandep are the
    cleanlist; the dcworld subtree and the @system member systempkg are
    kept."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--depclean"], _depclean_env(fixture_env, tmp_path)
    )
    assert result.returncode == 0
    out = result.stdout.splitlines()
    assert " * Always study the list of packages to be cleaned for any obvious" in out
    assert ">>> Calculating removal order..." in out
    assert ">>> These are the packages that would be unmerged:" in out
    # The cleanlist is exactly the two orphans, sorted.
    assert [ln for ln in out if ln.startswith(" dev-libs/")] == [
        " dev-libs/dcorphan",
        " dev-libs/dcorphandep",
    ]
    for kept in ("dcworld", "dcdep", "dcsub", "dccond", "dcbuilddep", "dcbdep", "systempkg"):
        assert f" dev-libs/{kept}\n" not in result.stdout
    assert "All selected packages: =dev-libs/dcorphan-1.0 =dev-libs/dcorphandep-1.0" in out
    assert out[-5:] == [
        "Packages installed:   9",
        "Packages in world:    1",
        "Packages in system:   3",
        "Required packages:    7",
        "Number to remove:     2",
    ]


def test_depclean_pretend_nothing_to_remove(emerge_binary, fixture_env, tmp_path):
    """When every installed package is reachable, depclean reports
    nothing (and hints at --verbose)."""
    root = _depclean_root(tmp_path)
    # World-list the two orphans too -> nothing is unreachable.
    (root / "var" / "lib" / "portage" / "world").write_text(
        "dev-libs/dcworld\ndev-libs/dcorphan\n"
    )
    env = dict(fixture_env)
    env["ROOT"] = str(root)
    result = _run([str(emerge_binary)], ["--pretend", "--depclean"], env)
    assert result.returncode == 0
    assert ">>> No packages selected for removal by depclean" in result.stdout
    assert ">>> To see reverse dependencies, use --verbose" in result.stdout
    assert ">>> Calculating removal order..." not in result.stdout
    assert result.stdout.splitlines()[-1] == "Number to remove:     0"


def test_depclean_keeps_a_build_only_dependency(emerge_binary, fixture_env, tmp_path):
    """Real _calc_depclean runs the depgraph in "remove" mode, where
    create_depgraph_params(myopts, "remove") sets bdeps="auto" and
    depgraph.py:4208-4213 keeps DEPEND/BDEPEND in the walk unless
    --with-bdeps=n. dcbuilddep is reachable *only* through dcworld's
    DEPEND, dcbdep *only* through dcdep's BDEPEND -- nothing RDEPENDs
    either -- yet both are kept, not cleaned."""
    env = _depclean_env(fixture_env, tmp_path)
    result = _run([str(emerge_binary)], ["--pretend", "--depclean"], env)
    assert result.returncode == 0
    cleaned = [ln for ln in result.stdout.splitlines() if ln.startswith(" dev-libs/")]
    assert cleaned == [" dev-libs/dcorphan", " dev-libs/dcorphandep"]
    assert " dev-libs/dcbuilddep" not in cleaned
    assert " dev-libs/dcbdep" not in cleaned

    # args mode: naming the build dep directly still won't remove it,
    # because its build-time parent is protected.
    for dep in ("dev-libs/dcbuilddep", "dev-libs/dcbdep"):
        r = _run([str(emerge_binary)], ["--pretend", "-c", dep], env)
        assert r.returncode == 0
        assert ">>> No packages selected for removal by depclean" in r.stdout


def test_depclean_pretend_with_args_narrows_to_the_named_packages(
    emerge_binary, fixture_env, tmp_path
):
    """`emerge -pc <atom>`: real _calc_depclean drops the world 'selected'
    atoms and protects every non-arg installed package, so the cleanlist
    is just the args-matched packages nothing else needs. No advisory
    block (real portage only shows it with no args)."""
    env = _depclean_env(fixture_env, tmp_path)

    # dcorphan: nothing needs it -> removable (its private dep dcorphandep
    # is protected, being non-arg, so it does NOT cascade).
    orphan = _run([str(emerge_binary)], ["--pretend", "-c", "dev-libs/dcorphan"], env)
    assert orphan.returncode == 0
    assert " * Always study the list" not in orphan.stdout
    assert [ln for ln in orphan.stdout.splitlines() if ln.startswith(" dev-libs/")] == [
        " dev-libs/dcorphan"
    ]
    assert orphan.stdout.splitlines()[-1] == "Number to remove:     1"

    # dcdep: dcworld still RDEPENDs it -> not removable.
    needed = _run([str(emerge_binary)], ["--pretend", "-c", "dev-libs/dcdep"], env)
    assert needed.returncode == 0
    assert ">>> No packages selected for removal by depclean" in needed.stdout
    assert needed.stdout.splitlines()[-1] == "Number to remove:     0"

    # dcworld: in @world, but -pc <atom> deselects + removes it if
    # nothing else needs it.
    world_member = _run([str(emerge_binary)], ["--pretend", "-c", "dev-libs/dcworld"], env)
    assert world_member.returncode == 0
    assert " dev-libs/dcworld" in world_member.stdout.splitlines()

    # An atom matching no installed package.
    missing = _run([str(emerge_binary)], ["--pretend", "-c", "dev-libs/nope"], env)
    assert missing.returncode == 1
    assert "--- Couldn't find 'dev-libs/nope' to depclean." in missing.stderr
    assert ">>> No packages selected for removal by depclean" in missing.stdout


def test_depclean_requires_pretend(emerge_binary, fixture_env):
    result = _run([str(emerge_binary)], ["--depclean"], fixture_env)
    assert result.returncode == 2
    assert "requires --pretend" in result.stderr


def test_depclean_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = _depclean_env(fixture_env, tmp_path)
    for args in (
        ["--pretend", "--depclean"],
        ["--pretend", "-c"],
        ["--pretend", "-c", "dev-libs/dcorphan"],
        ["--pretend", "-c", "dev-libs/dcdep"],
        ["--pretend", "-c", "dev-libs/dcworld"],
        ["--pretend", "-c", "dcorphan"],
        ["--pretend", "-c", "dev-libs/dcorphan", "dev-libs/nope"],
        ["--pretend", "-c", "dev-libs/nope"],
        ["--pretend", "-c", "dev-libs/dcbuilddep"],
        ["--pretend", "-c", "dev-libs/dcbdep"],
    ):
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def _depclean_order_root(tmp_path):
    """A ROOT whose orphan cleanlist has dependency edges between its own
    members, so real _calc_depclean's topological unmerge-order pass
    (actions.py:1591-1731) applies: dev-libs/mmid RDEPENDs dev-libs/zztop
    RDEPENDs dev-libs/aabase, all orphan. The removal order (a package
    before the ones it depends on) is [mmid, zztop, aabase] -- the
    reverse of the cat/pn sort a no-edge cleanlist would get."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("")

    def install(package, rdepend=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")

    install("mmid", rdepend="dev-libs/zztop")
    install("zztop", rdepend="dev-libs/aabase")
    install("aabase")
    return tmp_path


def test_depclean_pretend_removal_order_is_topological(emerge_binary, fixture_env, tmp_path):
    """The per-package blocks follow the topological removal order, not
    the cat/pn sort; '>>> Calculating removal order...' actually does
    something now."""
    env = dict(fixture_env)
    env["ROOT"] = str(_depclean_order_root(tmp_path))
    result = _run([str(emerge_binary)], ["--pretend", "--depclean"], env)
    assert result.returncode == 0
    blocks = [
        ln.strip() for ln in result.stdout.splitlines() if ln.startswith(" dev-libs/")
    ]
    assert blocks == ["dev-libs/mmid", "dev-libs/zztop", "dev-libs/aabase"]
    # "All selected packages" stays sorted (real portage's set-iteration
    # order there is not a meaningful spec; both pilot sides sort it).
    line = next(
        ln for ln in result.stdout.splitlines() if ln.startswith("All selected packages:")
    )
    assert line == (
        "All selected packages: =dev-libs/aabase-1.0 =dev-libs/mmid-1.0 =dev-libs/zztop-1.0"
    )


def test_depclean_removal_order_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = dict(fixture_env)
    env["ROOT"] = str(_depclean_order_root(tmp_path))
    args = ["--pretend", "--depclean"]
    rust = _run([str(emerge_binary)], args, env)
    python = _run(emerge_pretend_python, args, env)
    assert rust.returncode == python.returncode
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr


def _prune_root(tmp_path):
    """A ROOT for --prune: dev-libs/{aa,zz,mm} are each installed at
    multiple versions; dev-libs/single at one. dev-libs/keeper (world)
    RDEPENDs =dev-libs/mm-2.0, pinning that middle version. zz-1.0
    RDEPENDs =dev-libs/aa-1.0 -- both are themselves prunable, so that's
    only an ordering edge. Prune candidates: the non-highest versions of
    aa/zz/mm; mm-2.0 survives (pinned), so the cleanlist is
    {aa-1.0, zz-1.0, mm-1.0}, removal-ordered [zz-1.0, mm-1.0, aa-1.0]."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("dev-libs/keeper\n")

    def install(package, version, rdepend=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-{version}"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")

    install("aa", "1.0")
    install("aa", "2.0")
    install("zz", "1.0", rdepend="=dev-libs/aa-1.0")
    install("zz", "2.0")
    install("mm", "1.0")
    install("mm", "2.0")
    install("mm", "3.0")
    install("keeper", "1.0", rdepend="=dev-libs/mm-2.0")
    install("single", "1.0")
    return tmp_path


def test_prune_pretend_removes_superseded_versions_in_topological_order(
    emerge_binary, fixture_env, tmp_path
):
    """emerge -p --prune: the non-highest installed versions of
    multi-version cps, kept only if something needs that exact old
    version. No advisory block, no stats block (real action_depclean
    returns right after the unmerge() preview for action=="prune")."""
    env = dict(fixture_env)
    env["ROOT"] = str(_prune_root(tmp_path))
    result = _run([str(emerge_binary)], ["--pretend", "--prune"], env)
    assert result.returncode == 0
    assert " * Always study the list" not in result.stdout
    assert "Packages installed:" not in result.stdout
    assert "Number to remove:" not in result.stdout
    out = result.stdout.splitlines()
    assert ">>> Calculating removal order..." in out
    blocks = [ln.strip() for ln in out if ln.startswith(" dev-libs/")]
    assert blocks == ["dev-libs/zz", "dev-libs/mm", "dev-libs/aa"]
    # mm keeps 2.0 and 3.0 as omitted; aa/zz keep 2.0.
    mm_omitted = out[out.index(" dev-libs/mm") + 3]
    assert mm_omitted.strip() == "omitted: 2.0 3.0"
    line = next(ln for ln in out if ln.startswith("All selected packages:"))
    assert line == (
        "All selected packages: =dev-libs/aa-1.0 =dev-libs/mm-1.0 =dev-libs/zz-1.0"
    )


def test_prune_pretend_nothing_to_prune(emerge_binary, fixture_env, tmp_path):
    """A ROOT with only single-version packages: nothing is superseded."""
    root = tmp_path
    portage_dir = root / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("")
    for n in ("one", "two"):
        d = root / "var" / "db" / "pkg" / "dev-libs" / f"{n}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
    env = dict(fixture_env)
    env["ROOT"] = str(root)
    result = _run([str(emerge_binary)], ["--pretend", "--prune"], env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        ">>> No packages selected for removal by prune",
        ">>> To see reverse dependencies, use --verbose",
        ">>> To ignore dependencies, use --nodeps",
    ]


def test_prune_requires_pretend(emerge_binary, fixture_env):
    result = _run([str(emerge_binary)], ["--prune"], fixture_env)
    assert result.returncode == 2
    assert "requires --pretend" in result.stderr


def test_prune_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = dict(fixture_env)
    env["ROOT"] = str(_prune_root(tmp_path))
    for args in (
        ["--pretend", "--prune"],
        ["--pretend", "-P"],
        ["--pretend", "--prune", "dev-libs/mm"],
        ["--pretend", "--prune", "mm"],
        ["--pretend", "--prune", "dev-libs/single"],
        ["--pretend", "--prune", "dev-libs/nope"],
    ):
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def test_prune_matches_between_implementations_on_the_shared_vdb(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """The committed fixtures install dev-libs/unmergepkg at 1.0 and 2.0
    -- a real multi-version cp for --prune to act on."""
    for args in (["--pretend", "--prune"], ["--pretend", "--prune", "dev-libs/unmergepkg"]):
        rust = _run([str(emerge_binary)], args, fixture_env)
        python = _run(emerge_pretend_python, args, fixture_env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def test_unmerge_pretend_lists_selected_and_omitted(emerge_binary, fixture_env):
    """Real _emerge/unmerge.py::_unmerge_display for `unmerge_action ==
    "unmerge"`: every vdb match goes into `selected`, every other
    installed version of the same cp into `omitted`. dev-libs/unmergepkg
    is installed at 1.0 and 2.0. `emerge -pC =dev-libs/unmergepkg-1.0`
    selects 1.0, omits 2.0."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-C", "=dev-libs/unmergepkg-1.0"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        ">>> These are the packages that would be unmerged:",
        "",
        " dev-libs/unmergepkg",
        "    selected: 1.0 ",
        "   protected: none ",
        "     omitted: 2.0 ",
        "",
        "All selected packages: =dev-libs/unmergepkg-1.0",
        "",
        ">>> 'Selected' packages are slated for removal.",
        ">>> 'Protected' and 'omitted' packages will not be removed.",
    ]

    # A bare atom selects every installed version.
    both = _run([str(emerge_binary)], ["--pretend", "--unmerge", "dev-libs/unmergepkg"], fixture_env)
    assert both.returncode == 0
    assert "    selected: 1.0 2.0 " in both.stdout
    assert "All selected packages: =dev-libs/unmergepkg-1.0 =dev-libs/unmergepkg-2.0" in both.stdout


def test_unmerge_pretend_refuses_portage_itself(emerge_binary, fixture_env):
    """Real _unmerge_display: `sys-apps/portage` (PORTAGE_PACKAGE_ATOM)
    is moved from `selected` to `protected` with a note, and if it was
    the only selection the run reports nothing selected and exits 1."""
    result = _run([str(emerge_binary)], ["--pretend", "-C", "sys-apps/portage"], fixture_env)
    assert result.returncode == 1
    assert result.stdout.splitlines() == [
        ">>> These are the packages that would be unmerged:",
        "",
        ">>> No packages selected for removal by unmerge",
    ]
    assert (
        "Not unmerging package sys-apps/portage-1.0 since there is no valid reason"
        in result.stderr
    )


def test_unmerge_pretend_requires_pretend(emerge_binary, fixture_env):
    result = _run([str(emerge_binary)], ["--unmerge", "dev-libs/unmergepkg"], fixture_env)
    assert result.returncode == 2
    assert "requires --pretend" in result.stderr
    assert result.stdout == ""


def test_unmerge_pretend_system_profile_warning(emerge_binary, fixture_env):
    """Real _unmerge_display: `if not (protected or omitted) and cp in
    syslist` -- a cp that would be fully removed and is a @system member
    (dev-libs/systempkg, a *-prefixed atom in profiles/base/packages).
    The two `!!!` lines go to stderr."""
    result = _run([str(emerge_binary)], ["--pretend", "-C", "dev-libs/systempkg"], fixture_env)
    assert result.returncode == 0
    assert "'dev-libs/systempkg' is part of your system profile." in result.stderr
    assert "Unmerging it may be damaging to your system." in result.stderr
    assert result.stdout.splitlines()[2:6] == [
        " dev-libs/systempkg",
        "    selected: 1.0 ",
        "   protected: none ",
        "     omitted: none ",
    ]


def test_unmerge_pretend_still_listed_in_package_sets_warning(emerge_binary, fixture_env):
    """Real _unmerge_display: a `selected` package a user-editable set
    reached via world_sets still lists. dev-libs/nestedsetpkg is in
    etc/portage/sets/nestedtestset, which var/lib/portage/world_sets
    directly selects. The warning (stdout) names that set."""
    result = _run([str(emerge_binary)], ["--pretend", "-C", "dev-libs/nestedsetpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines()[:4] == [
        ">>> These are the packages that would be unmerged:",
        "Package dev-libs/nestedsetpkg-1.0 is going to be unmerged,",
        "but still listed in the following package sets:",
        "    nestedtestset",
    ]
    # Targeting the set itself makes it "active" -> no warning for its members.
    active = _run([str(emerge_binary)], ["--pretend", "-C", "@nestedtestset"], fixture_env)
    assert "still listed in the following package sets" not in active.stdout


def test_deselect_matches_a_plain_world_atom(emerge_binary, fixture_env, tmp_path):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/foo"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/foo from "world" favorites file...\n'


def test_deselect_matches_via_a_bare_package_name(emerge_binary, fixture_env, tmp_path):
    """A bare package name (no category at all) is expanded via real
    portage's own "null category" mechanism: scan the world file's own
    atoms for one sharing that package name, substitute in its
    category."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "foo"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/foo from "world" favorites file...\n'


def test_deselect_matches_regardless_of_installed_slot_when_target_is_unslotted(
    emerge_binary, fixture_env, tmp_path
):
    """dev-libs/baz is installed at slot 1, and the world file's own
    entry restricts it to slot 2 -- yet an *unslotted* "dev-libs/baz"
    CLI target still matches: real Atom.intersects() only rejects a
    slot mismatch when BOTH sides carry a slot restriction ("if
    self.slot is None or other.slot is None or self.slot==other.slot:
    return True"), and dep_expand() never adds one to an explicit-
    category target on its own. So the actually-installed slot is
    irrelevant here, same as the explicit-category target itself never
    needing to be installed at all (see run_deselect's own doc
    comment)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/baz"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/baz:2 from "world" favorites file...\n'


def test_deselect_respects_the_world_atoms_own_slot_restriction_when_target_is_slotted(
    emerge_binary, fixture_env, tmp_path
):
    """Unlike the unslotted case above, a CLI target that itself carries
    an explicit slot restriction ("dev-libs/baz:1") DOES get rejected by
    real Atom.intersects() against a world entry restricted to a
    different slot ("dev-libs/baz:2") -- both sides now have a slot to
    compare, and they disagree."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/baz:1"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> No matching atoms found in "world" favorites file...\n'


def test_deselect_matches_a_slotted_target_against_the_matching_world_slot(
    emerge_binary, fixture_env, tmp_path
):
    """"dev-libs/baz:2" (never installed at slot 2, only at slot 1) still
    matches the world file's own "dev-libs/baz:2" entry -- an explicit-
    category target needs no installed check at all, slot restriction
    included."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/baz:2"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/baz:2 from "world" favorites file...\n'


def test_deselect_matches_a_slot_restricted_world_atom_at_the_right_slot(
    emerge_binary, fixture_env, tmp_path
):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/bar"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/bar:1 from "world" favorites file...\n'


def test_deselect_matches_an_explicit_category_target_never_installed(
    emerge_binary, fixture_env, tmp_path
):
    """dev-libs/qux is listed in the world file but was never actually
    installed -- yet real dep_expand() returns an explicit-category atom
    completely unchanged, with no vardb check at all, before
    action_deselect ever seeds expanded_atoms with it unconditionally.
    So installation is NOT required here: the world file's own text is
    enough by itself for an explicit-category target. (An earlier
    version of this pilot got this backwards -- see run_deselect's own
    doc comment in pretend.rs for the full correction.)"""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/qux"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/qux from "world" favorites file...\n'


def test_deselect_matches_a_bare_name_target_never_installed(
    emerge_binary, fixture_env, tmp_path
):
    """Same correction as the explicit-category case above, but via the
    bare-name/null-category path: "qux" substitutes in "dev-libs" from
    the world file's own "dev-libs/qux" entry unconditionally, no
    installed check at all -- real vardb.match() on the still-null-
    category atom can never match a real vdb entry, so it's dead code
    for this branch and correctly contributes nothing."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "qux"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove dev-libs/qux from "world" favorites file...\n'


def test_deselect_reports_no_match_for_an_installed_but_not_world_listed_target(
    emerge_binary, fixture_env, tmp_path
):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/notinworld"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> No matching atoms found in "world" favorites file...\n'


def test_deselect_matches_an_exact_version_world_atom(emerge_binary, fixture_env, tmp_path):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "=dev-libs/vers-1.0"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove =dev-libs/vers-1.0 from "world" favorites file...\n'


def test_deselect_rejects_a_different_version(emerge_binary, fixture_env, tmp_path):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "=dev-libs/vers-2.0"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> No matching atoms found in "world" favorites file...\n'


def test_deselect_rejects_a_range_operator_even_when_satisfied(
    emerge_binary, fixture_env, tmp_path
):
    """">=dev-libs/vers-1.0" would actually be *satisfied* by the world
    file's own "=dev-libs/vers-1.0" entry under a real version-range
    check, but real Atom.intersects() is deliberately narrower than
    that: it requires the operator itself to match exactly (its own
    docstring: "atoms with different cpv, operator or use attributes
    cause this method to return False even though there may actually be
    some intersection... TODO: Detect more forms of intersection"), so
    this reports no match."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", ">=dev-libs/vers-1.0"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> No matching atoms found in "world" favorites file...\n'


def test_deselect_multiple_targets_discard_sorted_alphabetically(
    emerge_binary, fixture_env, tmp_path
):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/foo", "dev-libs/bar"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '>>> Would remove dev-libs/bar:1 from "world" favorites file...',
        '>>> Would remove dev-libs/foo from "world" favorites file...',
    ]


def test_deselect_at_target_matches_a_world_sets_entry_by_exact_name(
    emerge_binary, fixture_env, tmp_path
):
    """"--deselect @myselectedset" matches the "@myselectedset" line in
    the separate world_sets file (real portage's own WORLD_SETS_FILE) --
    real action_deselect never expands a "@name" target's own set
    members at all, only exact-matches it against a world_set entry
    that's itself a literal "@name" string, so this needs no vdb/atom
    matching whatsoever, unlike every other --deselect target. Reported
    against "world_sets", not "world" -- its own real source file."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "@myselectedset"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> Would remove @myselectedset from "world_sets" favorites file...\n'


def test_deselect_at_target_with_no_matching_world_sets_entry_reports_no_match(
    emerge_binary, fixture_env, tmp_path
):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "@nosuchset"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == '>>> No matching atoms found in "world" favorites file...\n'


def test_deselect_combines_a_world_atom_and_a_world_sets_entry_sorted_together(
    emerge_binary, fixture_env, tmp_path
):
    """A plain atom target and a "@name" target discarded in the same
    run are sorted into ONE combined list (real "sorted(discard_atoms,
    key=str)"), not printed as two separate "world" then "world_sets"
    blocks -- "@myselectedset" sorts before "dev-libs/foo" (real Python
    "@" < "d" and canonical str.sort() ordering, which this pilot's own
    plain string sort already matches)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "dev-libs/foo", "@myselectedset"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '>>> Would remove @myselectedset from "world_sets" favorites file...',
        '>>> Would remove dev-libs/foo from "world" favorites file...',
    ]
    assert "anotherselectedset" not in result.stdout


def test_deselect_with_no_targets_at_all_reports_no_match(emerge_binary, fixture_env, tmp_path):
    result = _run(
        [str(emerge_binary)], ["--pretend", "--deselect"], _deselect_env(fixture_env, tmp_path)
    )
    assert result.returncode == 0
    assert result.stdout == '>>> No matching atoms found in "world" favorites file...\n'


def test_deselect_requires_pretend(emerge_binary, fixture_env, tmp_path):
    """This pilot's whole CLI is dry-run-only regardless of the flag --
    --deselect is no exception, and hits the exact same "only --pretend
    is implemented" error real action_deselect's own (unreachable here)
    file-writing branch would otherwise need."""
    result = _run(
        [str(emerge_binary)],
        ["--deselect", "dev-libs/foo"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 2
    assert (
        result.stderr.strip()
        == "emerge (pilot v1): --deselect requires --pretend (see PROMPT.md)"
    )


def test_deselect_short_alias_and_bundling(emerge_binary, fixture_env, tmp_path):
    env = _deselect_env(fixture_env, tmp_path)
    short = _run([str(emerge_binary)], ["--pretend", "-W", "dev-libs/foo"], env)
    assert short.returncode == 0
    assert short.stdout == '>>> Would remove dev-libs/foo from "world" favorites file...\n'

    bundled = _run([str(emerge_binary)], ["-pW", "dev-libs/foo"], env)
    assert bundled.returncode == 0
    assert bundled.stdout == '>>> Would remove dev-libs/foo from "world" favorites file...\n'


def test_deselect_n_does_not_trigger_deselect_mode(emerge_binary, fixture_env, tmp_path):
    """Real "--deselect": y_or_n -- an explicit "n" leaves this
    invocation as an ordinary --pretend resolution instead (real
    main.py's own "if myaction is None and myoptions.deselect is True"
    check), so "dev-libs/foo" here is treated as a normal target atom,
    not a deselect argument -- and resolves the ordinary way: it's
    already installed (in this throwaway ROOT's own vdb) and satisfies
    the atom, so it's reported as a plain reinstall (real portage's own
    "selective" gap for a bare top-level atom -- see resolve_pretend's
    own doc comment, portage-repo) rather than as a deselect-mode "Would
    remove" line."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--deselect", "n", "dev-libs/foo"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  r] dev-libs/foo-1.0\n"


def test_deselect_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = _deselect_env(fixture_env, tmp_path)
    for args in (
        ["--pretend", "--deselect", "dev-libs/foo"],
        ["--pretend", "--deselect", "foo"],
        ["--pretend", "--deselect", "dev-libs/baz"],
        ["--pretend", "--deselect", "dev-libs/foo", "dev-libs/bar"],
        ["--pretend", "--deselect", "@myselectedset"],
        ["--pretend", "--deselect", "@nosuchset"],
        ["--pretend", "--deselect", "dev-libs/foo", "@myselectedset"],
        ["--pretend", "--deselect"],
        ["--deselect", "dev-libs/foo"],
        ["--pretend", "--deselect", "dev-libs/qux"],
        ["--pretend", "--deselect", "qux"],
        ["--pretend", "--deselect", "dev-libs/notinworld"],
        ["--pretend", "--deselect", "dev-libs/baz:1"],
        ["--pretend", "--deselect", "dev-libs/baz:2"],
        ["--pretend", "--deselect", "=dev-libs/vers-1.0"],
        ["--pretend", "--deselect", "=dev-libs/vers-2.0"],
        ["--pretend", "--deselect", ">=dev-libs/vers-1.0"],
    ):
        rust_result = _run([str(emerge_binary)], args, env)
        python_result = _run(emerge_pretend_python, args, env)
        assert rust_result.returncode == python_result.returncode, args
        assert rust_result.stdout == python_result.stdout, args
        assert rust_result.stderr == python_result.stderr, args


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
        # dev-libs/systempkg is a third *-prefixed @system atom (base/
        # packages), installed since the -pC system-profile slice.
        "dev-libs/systempkg-1.0 is already installed; nothing to do",
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
        "dev-libs/systempkg-1.0 is already installed; nothing to do",
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
    """-v combines with -N exactly like it does with New/Upgrade. For a
    Reinstall (an installed side exists), real output_helpers.py::
    _create_use_string diffs each flag against the installed version's
    recorded USE/IUSE: `foo` was in old IUSE but off, is now on -> `foo*`.
    """
    result = _run(
        [str(emerge_binary)], ["--pretend", "-N", "-v", "dev-libs/reinstallpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  r] dev-libs/reinstallpkg-1.0 (reinstall for changed USE: foo)  USE="foo*"',
        "[ebuild  N] dev-libs/newpkg-1.0",
        "",
        "Total: 2 packages (1 new, 1 reinstall), Size of downloads: 0 KiB",
    ]


def test_without_newuse_a_use_changed_package_stays_already_installed(
    emerge_binary, fixture_env
):
    """The exact same fixture as the Reinstall test above, but without
    --newuse: the USE mismatch is real, but nothing checks for it unless
    --newuse is given, so this must stay the pre-existing
    AlreadyInstalled outcome -- not a Reinstall for that reason, and not
    a NEW dependency recursion into dev-libs/newpkg either. --noreplace
    isolates this from the unrelated "bare top-level atom" reinstall a
    plain invocation would otherwise also trigger (see resolve_pretend's
    own doc comment, portage-repo, on real portage's own "selective"
    gap) -- this test is about --newuse specifically, not that."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--noreplace", "dev-libs/reinstallpkg"], fixture_env
    )
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


def test_changed_deps_reinstalls_and_recurses_into_the_current_ebuilds_own_dependency(
    emerge_binary, fixture_env
):
    """dev-libs/changeddepspkg is installed with a vdb-recorded
    RDEPEND="dev-libs/samepkg", but the repo's current ebuild for that
    exact version now has RDEPEND="dev-libs/newpkg" instead -- real
    depgraph.py's own _changed_deps compares these (flattened against
    the installed package's own recorded USE, real portage's own
    uselist=pkg.use.enabled) and, once --changed-deps is given, this
    pilot reports a reinstall and recurses into the CURRENT ebuild's own
    dependency (newpkg), not the vdb's stale one -- matching how
    --deep's own AlreadyInstalled walk already reuses the repo's current
    metadata rather than a vdb snapshot."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-deps", "dev-libs/changeddepspkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  r] dev-libs/changeddepspkg-1.0 (reinstall for changed dependencies)',
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_changed_deps_ignores_a_libc_only_dependency_change(emerge_binary, fixture_env):
    """dev-libs/libcnoisepkg's own vdb-recorded RDEPEND names
    sys-libs/glibc; its current ebuild names sys-libs/musl instead --
    both are real virtual/libc providers (the fixture vdb's own
    virtual/libc entry RDEPENDs on "|| ( sys-libs/glibc sys-libs/musl
    )"), so real strip_libc_deps strips both out before comparing,
    leaving only the identical dev-libs/samepkg on each side -- no
    reinstall, even with --changed-deps given."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-deps", "dev-libs/libcnoisepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert (
        result.stdout.strip() == "dev-libs/libcnoisepkg-1.0 is already installed; nothing to do"
    )


def test_changed_deps_detects_an_atom_moved_between_two_dep_keys(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/movedkeydepspkg's vdb recorded RDEPEND="dev-libs/samepkg";
    its current ebuild has that exact atom in PDEPEND instead, nothing
    else on either side. The net atom set is identical, so the pre-slice
    "merge every dep key into one string, then flatten and compare"
    approach saw no change -- real _changed_deps (depgraph.py:3168)
    compares built_deps to unbuilt_deps element-wise, one struct per dep
    key, which this pilot now mirrors: the move registers as changed."""
    args = ["--pretend", "--changed-deps", "dev-libs/movedkeydepspkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild  r] dev-libs/movedkeydepspkg-1.0 (reinstall for changed dependencies)",
    ]


def test_changed_deps_ignores_a_built_slot_operators_resolved_slot(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/slotopdepspkg's current ebuild has
    RDEPEND="dev-libs/slotoptarget:="; its vdb recorded the built form
    "dev-libs/slotoptarget:2=" (the concrete slot portage records when a
    := dep is merged). Real strip_slots (lib/portage/dep/_slot_operator.py)
    normalizes the built :2= back to := before comparing, so this is NOT
    a changed dependency -- without that normalization every := dep would
    spuriously trigger a --changed-deps reinstall."""
    args = ["--pretend", "--changed-deps", "dev-libs/slotopdepspkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert (
        rust.stdout.strip()
        == "dev-libs/slotopdepspkg-1.0 is already installed; nothing to do"
    )


def test_changed_deps_structured_comparison(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real _changed_deps (depgraph.py:3168) compares structured
    use_reduce(token_class=Atom) output as Python lists -- order-sensitive
    everywhere, redundant brackets collapsed. The pilot's
    portage_use_reduce::use_reduce_structured ports real use_reduce's own
    flat=False bracket-optimization pass to match:

      - anyofreorderdepspkg: vdb `|| ( reorderdepa reorderdepb )`, ebuild
        swaps the alternatives -> changed
      - orderchangeddepspkg: vdb `reorderdepa reorderdepb`, ebuild
        `reorderdepb reorderdepa` -> changed (faithful to list `!=`,
        which is order-sensitive in AND context too)
      - redundantbracketdepspkg: vdb `reorderdepa reorderdepb`, ebuild
        `( reorderdepa reorderdepb )` -> NOT changed
    """
    changed = ["anyofreorderdepspkg", "orderchangeddepspkg"]
    for pkg in changed:
        args = ["--pretend", "--changed-deps", f"dev-libs/{pkg}"]
        rust = _run([str(emerge_binary)], args, fixture_env)
        python = _run(emerge_pretend_python, args, fixture_env)
        assert rust.returncode == 0
        assert python.returncode == 0
        assert rust.stdout == python.stdout, pkg
        assert rust.stderr == python.stderr, pkg
        assert rust.stdout.splitlines() == [
            f"[ebuild  r] dev-libs/{pkg}-1.0 (reinstall for changed dependencies)",
        ], pkg

    args = ["--pretend", "--changed-deps", "dev-libs/redundantbracketdepspkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert (
        rust.stdout.strip()
        == "dev-libs/redundantbracketdepspkg-1.0 is already installed; nothing to do"
    )


def test_changed_deps_json_includes_the_changed_deps_field(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-deps", "--json", "dev-libs/changeddepspkg"],
        fixture_env,
    )
    assert result.returncode == 0
    parsed = json.loads(result.stdout)
    changeddepspkg_entry = next(
        e for e in parsed["entries"] if e["package"] == "changeddepspkg"
    )
    assert changeddepspkg_entry["outcome"] == "reinstall"
    assert changeddepspkg_entry["changed_deps"] is True
    assert changeddepspkg_entry["changed_use"] == []


def test_without_changed_deps_a_dependency_change_is_never_detected(emerge_binary, fixture_env):
    """--noreplace isolates this from the unrelated "bare top-level
    atom" reinstall a plain invocation would otherwise also trigger --
    see resolve_pretend's own doc comment (portage-repo) on real
    portage's own "selective" gap; this test is about --changed-deps
    specifically, not that."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "dev-libs/changeddepspkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "dev-libs/changeddepspkg-1.0 is already installed; nothing to do"


def test_changed_slot_reinstalls_a_package_whose_vdb_slot_differs_from_the_current_ebuild(
    emerge_binary, fixture_env
):
    """dev-libs/changedslotpkg is installed with a vdb-recorded SLOT="0",
    but the repo's current ebuild for that exact version now has
    SLOT="0/2" instead (an ABI-bump sub-slot change) -- real
    depgraph.py's own _changed_slot compares these and, once
    --changed-slot is given, this pilot reports a reinstall. Without
    --changed-deps, only the slot reason appears even though this same
    fixture package's own RDEPEND also differs (see the combined-reason
    test below)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-slot", "dev-libs/changedslotpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/changedslotpkg-1.0 (reinstall for changed slot)",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_changed_deps_and_changed_slot_combine_in_one_reinstall_line(
    emerge_binary, fixture_env
):
    """dev-libs/changedslotpkg has both a stale vdb RDEPEND and a stale
    vdb SLOT relative to its current ebuild -- real portage treats
    --changed-deps/--changed-slot as independent, freely-combinable
    reinstall triggers, so giving both prints both reasons on the same
    line, deps first."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-deps", "--changed-slot", "dev-libs/changedslotpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  r] dev-libs/changedslotpkg-1.0 (reinstall for changed dependencies; changed slot)",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_changed_slot_json_includes_the_changed_slot_field(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-slot", "--json", "dev-libs/changedslotpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    parsed = json.loads(result.stdout)
    changedslotpkg_entry = next(
        e for e in parsed["entries"] if e["package"] == "changedslotpkg"
    )
    assert changedslotpkg_entry["outcome"] == "reinstall"
    assert changedslotpkg_entry["changed_slot"] is True
    assert changedslotpkg_entry["changed_deps"] is False


def test_without_changed_slot_a_slot_change_is_never_detected(emerge_binary, fixture_env):
    """--noreplace isolates this from the unrelated "bare top-level
    atom" reinstall a plain invocation would otherwise also trigger --
    see resolve_pretend's own doc comment (portage-repo) on real
    portage's own "selective" gap; this test is about --changed-slot
    specifically, not that."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "dev-libs/changedslotpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == "dev-libs/changedslotpkg-1.0 is already installed; nothing to do"


def test_without_with_test_deps_a_test_gated_dependency_is_never_pulled_in(
    emerge_binary, fixture_env
):
    """dev-libs/withtestdeppkg's own RDEPEND is "dev-libs/newpkg test?
    ( dev-libs/testonlydep )" -- without --with-test-deps, only the
    unconditional dev-libs/newpkg is ever pulled in."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/withtestdeppkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/withtestdeppkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_with_test_deps_pulls_in_a_top_level_atoms_own_test_gated_dependency(
    emerge_binary, fixture_env
):
    """Same fixture as above, but with --with-test-deps: real
    accept_keywords_defaults-style extraction (use_reduce_flat_subset,
    subset={"test"}) additionally pulls in dev-libs/testonlydep, on top
    of -- not instead of -- the unconditional dev-libs/newpkg."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--with-test-deps", "dev-libs/withtestdeppkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/withtestdeppkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
        "[ebuild  N] dev-libs/testonlydep-1.0",
    ]


def test_with_test_deps_n_explicitly_disables_it(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--with-test-deps", "n", "dev-libs/withtestdeppkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/withtestdeppkg-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_with_test_deps_does_not_apply_beyond_a_top_level_atom(emerge_binary, fixture_env):
    """dev-libs/withtestdepconsumer RDEPENDs on dev-libs/withtestdeppkg,
    reaching it at depth 1, not depth 0 -- real depgraph.py's own
    "pkg.depth == 0" gate (this pilot's own equivalent) means
    dev-libs/testonlydep must NOT be pulled in even with --with-test-deps
    given, since withtestdeppkg itself isn't the top-level atom here."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--with-test-deps", "dev-libs/withtestdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/withtestdepconsumer-1.0",
        "[ebuild  N] dev-libs/withtestdeppkg-1.0",
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
    assert result.stdout == (
        '[ebuild  N] dev-libs/useflagpkg-1.0  USE="foo -missingflag"\n'
        "\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n"
    )


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


def test_without_update_a_bare_top_level_atom_still_offers_a_newer_version(
    emerge_binary, fixture_env
):
    """dev-libs/upgradepkg is installed at 1.0; a newer 2.0 is visible in
    the tree too. Real depgraph.py's own `avoid_update` (lines 7814 and
    8448) means an already-installed version that's still a *matched
    candidate* is kept as-is without searching further -- but for a
    directly-requested (top-level) atom without `selective`
    (--update/--newuse/--changed-use/--changed-deps/--changed-slot/
    --noreplace/--selective), the installed version is never even a
    matched candidate to begin with (real `want_reinstall =
    found_available_arg and not selective`, see resolve_pretend's own
    doc comment, portage-repo) -- so `avoid_update`'s own shortcut never
    gets a chance to fire, and the ordinary "best visible candidate"
    search proceeds, finding 2.0. Confirmed live against the real,
    installed system `emerge` (not just read from source) during this
    slice's own research. This directly reverses what an earlier version
    of this pilot's own test suite asserted here, before this real
    behavior was discovered."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)\n"


def test_noreplace_restores_the_real_avoid_update_shortcut(emerge_binary, fixture_env):
    """The mirror case: with `selective` restored via --noreplace, the
    installed version (1.0) IS still a matched candidate (real
    `want_reinstall` no longer forces it out), so real `avoid_update`'s
    own shortcut fires normally and 2.0 is never even considered --
    matching this pilot's own pre-existing behavior for every case
    other than a bare top-level atom."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--noreplace", "dev-libs/upgradepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"


def test_selective_n_cancels_selective_even_when_update_would_have_set_it(
    emerge_binary, fixture_env
):
    """Real create_depgraph_params.py's own unconditional `if myopts.get(
    "--selective") == "n": myparams.pop("selective", None)`, checked
    last: an explicit --selective=n wins even over --update, which would
    otherwise set selective on its own. dev-libs/samepkg has no newer
    version available, so --update alone still resolves its own "best
    across everything" search right back to the installed version --
    but that comparison is where `is_top_level and not selective` gets
    checked too (see resolve_pretend's own doc comment, portage-repo:
    it's not just the early `not update` shortcut that consults
    selective), so cancelling selective here still forces a bare
    reinstall even though --update genuinely ran its own search and
    found nothing better."""
    with_update_alone = _run(
        [str(emerge_binary)], ["--pretend", "--update", "dev-libs/samepkg"], fixture_env
    )
    assert with_update_alone.returncode == 0
    assert with_update_alone.stdout == "dev-libs/samepkg-1.0 is already installed; nothing to do\n"

    with_selective_cancelled = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--selective=n", "dev-libs/samepkg"],
        fixture_env,
    )
    assert with_selective_cancelled.returncode == 0
    assert with_selective_cancelled.stdout == "[ebuild  r] dev-libs/samepkg-1.0\n"


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
    appears here, only deeppkg's own top-level "nothing to do" line.
    --noreplace keeps deeppkg itself AlreadyInstalled (see
    resolve_pretend's own doc comment, portage-repo, on real portage's
    own "selective" gap for a bare top-level atom) -- --deep's own
    walk only ever applies to an AlreadyInstalled entry in the first
    place, so this isolates --deep's own gating from that unrelated
    default."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--noreplace", "dev-libs/deeppkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"


def test_deep_walks_the_whole_already_installed_chain(emerge_binary, fixture_env):
    """Same fixture as above, but with a bare --deep (unlimited depth,
    real myoptions.deep is True): both already-installed steps
    (deeppkg -> deeppkg2) get walked, reaching deeppkg2's own RDEPEND on
    newpkg (New) -- deeppkg2 itself stays silent (AlreadyInstalled, not
    a top-level atom, same "don't clutter the list" rule as ever), but
    newpkg's own [ebuild N] line now appears. --noreplace keeps deeppkg
    itself AlreadyInstalled (see resolve_pretend's own doc comment,
    portage-repo, on real portage's own "selective" gap for a bare
    top-level atom) -- --deep's own walk only ever applies to an
    AlreadyInstalled entry in the first place, so this isolates --deep's
    own gating from that unrelated default."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--noreplace", "--deep", "dev-libs/deeppkg"], fixture_env
    )
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
    character" rule already established for a bundled -v. -n (bundled
    too) keeps deeppkg itself AlreadyInstalled, same isolation reasoning
    as the long-flag test above."""
    result = _run([str(emerge_binary)], ["-pnD", "dev-libs/deeppkg"], fixture_env)
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
    real and not silently ignored in either direction. --noreplace keeps
    deeppkg itself AlreadyInstalled (see resolve_pretend's own doc
    comment, portage-repo, on real portage's own "selective" gap for a
    bare top-level atom), isolating --deep's own gating from that
    unrelated default."""
    bounded_one = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "--deep=1", "dev-libs/deeppkg"],
        fixture_env,
    )
    assert bounded_one.returncode == 0
    assert bounded_one.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"

    bounded_two = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "--deep=2", "dev-libs/deeppkg"],
        fixture_env,
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
    from myparams either way. --noreplace keeps deeppkg itself
    AlreadyInstalled (see resolve_pretend's own doc comment,
    portage-repo, on real portage's own "selective" gap for a bare
    top-level atom), isolating --deep's own gating from that unrelated
    default."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "--deep=0", "dev-libs/deeppkg"],
        fixture_env,
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
    checks for before `deep` is ever consulted. --noreplace keeps
    deeppkg itself AlreadyInstalled (see resolve_pretend's own doc
    comment, portage-repo, on real portage's own "selective" gap for a
    bare top-level atom), isolating --deep/--nodeps's own gating from
    that unrelated default."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "--deep", "--nodeps", "dev-libs/deeppkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/deeppkg-1.0 is already installed; nothing to do\n"


def test_exclude_leaves_an_already_installed_package_alone_even_with_update(
    emerge_binary, fixture_env
):
    """dev-libs/upgradepkg is installed at 1.0, a newer 2.0 is visible --
    without --exclude, --update offers the upgrade (see the --update
    contract tests); --exclude matching it overrides --update entirely,
    same as real _want_update_pkg's/_replace_installed_atom's own
    excluded-checked-first precedent."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--exclude", "dev-libs/upgradepkg", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"


def test_exclude_matches_via_a_wildcard_atom_too(emerge_binary, fixture_env):
    """Real WildcardPackageSet accepts wildcard atoms, not just plain
    ones -- ported here as the same two-tier matcher package.mask/
    .unmask already uses."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--exclude", "dev-libs/*", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"


def test_exclude_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-X is --exclude's real short alias (see lib/_emerge/main.py's
    shortmapping); standalone (never bundled -- see the CLI-surface
    tests below) it must behave identically to the long-flag invocation
    above."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "-X", "dev-libs/upgradepkg", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"


def test_exclude_does_not_affect_a_non_matching_package(emerge_binary, fixture_env):
    """A --exclude atom for an unrelated package has no effect at all --
    --update still offers the upgrade normally."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--exclude", "dev-libs/does-not-exist", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  U] dev-libs/upgradepkg-2.0 (upgrade from 1.0)\n"


def test_exclude_prevents_a_not_yet_installed_package_from_being_offered(
    emerge_binary, fixture_env
):
    """dev-libs/newpkg has no installed version at all -- excluding it as
    a top-level atom means there's no eligible candidate left, the same
    fatal "no ebuilds to satisfy" outcome any other unsatisfiable
    top-level atom already gets."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--exclude", "dev-libs/newpkg", "dev-libs/newpkg"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == 'emerge: there are no ebuilds to satisfy "dev-libs/newpkg".'


def test_exclude_threads_through_dependency_recursion_not_just_top_level(
    emerge_binary, fixture_env
):
    """dev-libs/upgradepkg is reached only as a *dependency* of
    dev-libs/withdeps here, never a top-level atom -- --exclude must
    still leave it alone despite --update, proving the flag threads
    uniformly through the whole BFS, not just a top-level atom (same
    precedent --update's own equivalent test already set)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--exclude", "dev-libs/upgradepkg", "dev-libs/withdeps"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N] dev-libs/withdeps-1.0",
        "[ebuild  N] dev-libs/newpkg-1.0",
    ]


def test_exclude_repeated_occurrences_and_space_separated_values_both_accumulate(
    emerge_binary, fixture_env
):
    """Real bin/emerge declares --exclude "action": "append" (repeatable)
    with a help text describing "a space separated list" as one
    occurrence's own value -- both forms must accumulate into the same
    exclude set, not just whichever form happens to be used."""
    repeated = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--update",
            "--exclude",
            "dev-libs/upgradepkg",
            "--exclude",
            "dev-libs/does-not-exist",
            "dev-libs/upgradepkg",
        ],
        fixture_env,
    )
    assert repeated.returncode == 0
    assert repeated.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"

    space_separated = _run(
        [str(emerge_binary)],
        [
            "--pretend",
            "--update",
            "--exclude",
            "dev-libs/does-not-exist dev-libs/upgradepkg",
            "dev-libs/upgradepkg",
        ],
        fixture_env,
    )
    assert space_separated.returncode == 0
    assert space_separated.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"


def test_exclude_inline_equals_form_and_missing_argument(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--exclude=dev-libs/upgradepkg", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "dev-libs/upgradepkg-1.0 is already installed; nothing to do\n"

    missing_arg = _run([str(emerge_binary)], ["--pretend", "--exclude"], fixture_env)
    assert missing_arg.returncode == 2
    assert missing_arg.stdout == ""
    assert missing_arg.stderr.strip() == 'emerge: option "--exclude" requires an argument'


def test_exclude_is_not_bundle_compatible(emerge_binary, fixture_env):
    """Unlike -v/-D, -X's own value is required, not optional, so this
    pilot deliberately doesn't support bundling it -- a specific message
    instead of a misleading generic one."""
    result = _run([str(emerge_binary)], ["-pX", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert result.stderr.strip() == (
        "emerge: -X (--exclude) requires an argument and can't be bundled with "
        "other short flags in this pilot"
    )


def test_json_is_not_a_real_emerge_option(emerge_binary, fixture_env):
    """--json is a pilot-specific addition (real portage has no
    structured-output mode for --pretend at all) -- pinned in full since
    it's this pilot's own content, not derived from any real emerge
    output, unlike every other flag's own contract test."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout == (
        '{"entries":[{"category":"dev-libs","package":"newpkg","outcome":"new",'
        '"version":"1.0","new_slot":false,"interactive":false,'
        '"fetch_restrict":false,"fetch_restrict_satisfied":false,"slot":"0","source":"ebuild",'
        '"provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},'
        '"requested":true,'
        '"required_by":[],"builds_against_running_root":null,"blockers":[]}],'
        '"slot_conflicts":[],"changed_deps_report":[]}\n'
    )


def test_json_upgrade_includes_from_version(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--json", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        '{"entries":[{"category":"dev-libs","package":"upgradepkg","outcome":"upgrade",'
        '"version":"2.0","from_version":"1.0","interactive":false,'
        '"fetch_restrict":false,"fetch_restrict_satisfied":false,"slot":"0","source":"ebuild",'
        '"provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},'
        '"requested":true,"required_by":[],"builds_against_running_root":null,'
        '"blockers":[]}],"slot_conflicts":[],'
        '"changed_deps_report":[]}\n'
    )


def test_json_diamond_dependency_lists_both_required_by_owners(emerge_binary, fixture_env):
    """dev-libs/common is reached via both shared-a and shared-b -- --json
    must list both owners, sorted, not just whichever the BFS resolved
    first (see portage-repo's own required_by_map)."""
    result = _run([str(emerge_binary)], ["--pretend", "--json", "dev-libs/diamond"], fixture_env)
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    common = next(e for e in payload["entries"] if e["package"] == "common")
    assert common["required_by"] == [
        {"category": "dev-libs", "package": "shared-a"},
        {"category": "dev-libs", "package": "shared-b"},
    ]


def test_json_provenance_records_mask_and_unmask_entries(emerge_binary, fixture_env):
    """dev-libs/maskedandunmaskedpkg is matched by a package.mask entry
    that an identical package.unmask entry then cancels (see
    fixtures/etc/portage/package.mask and .unmask) -- --json's own
    "provenance" must record both, not just the fact that the package
    ended up visible."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/maskedandunmaskedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    assert payload["entries"][0]["provenance"] == {
        "mask_entry": "dev-libs/maskedandunmaskedpkg",
        "unmask_entry": "dev-libs/maskedandunmaskedpkg",
        "keyword_entry": None,
    }


def test_json_provenance_records_the_keyword_entry_actually_needed(emerge_binary, fixture_env):
    """dev-libs/wildcardkeywordpkg is ~amd64-only and only visible via the
    "*/wildcardkeywordpkg ~amd64" package.accept_keywords entry (see
    fixtures/etc/portage/package.accept_keywords) -- --json's own
    "provenance" must name that specific entry, not just report the
    package as visible."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/wildcardkeywordpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    assert payload["entries"][0]["provenance"] == {
        "mask_entry": None,
        "unmask_entry": None,
        "keyword_entry": "*/wildcardkeywordpkg",
    }


def test_json_provenance_is_all_null_when_nothing_special_was_needed(emerge_binary, fixture_env):
    """dev-libs/newpkg needs no package.mask/.unmask/.accept_keywords help
    at all -- --json's own "provenance" is present but every field is
    null, not omitted."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    assert payload["entries"][0]["provenance"] == {
        "mask_entry": None,
        "unmask_entry": None,
        "keyword_entry": None,
    }


def test_json_requested_reflects_top_level_vs_dependency(emerge_binary, fixture_env):
    """--json's own "requested" field, unlike the plain-text loop's
    "already installed; nothing to do" line, is available for every
    entry regardless of outcome -- true only for dev-libs/withdeps
    itself, false for everything it pulls in."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/withdeps"], fixture_env
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    requested = {e["package"]: e["requested"] for e in payload["entries"]}
    assert requested == {"withdeps": True, "newpkg": False, "upgradepkg": False}


def test_json_verbose_includes_use_flags(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "--json", "dev-libs/useflagpkg"], fixture_env
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    useflagpkg = next(e for e in payload["entries"] if e["package"] == "useflagpkg")
    assert useflagpkg["use_flags"] == {"foo": True, "missingflag": False}


def test_json_without_verbose_omits_use_flags(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/useflagpkg"], fixture_env
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    useflagpkg = next(e for e in payload["entries"] if e["package"] == "useflagpkg")
    assert "use_flags" not in useflagpkg


def test_json_source_reflects_ebuild_vs_binary_and_is_omitted_for_no_visible_candidate(
    emerge_binary, fixture_env
):
    """"source" mirrors the plain-text loop's own bracket word ("ebuild"/
    "binary", real RootConfig.py's own pkg_tree_map-driven type_name),
    not a hardcoded constant -- entry_to_json used to emit a literal
    "ebuild" regardless of entry.source, a real bug left over from
    before binary-package support (--usepkg/--usepkgonly) existed at
    all, only caught once a binary candidate could actually resolve.
    "source" is omitted entirely for a dependency-level
    no_visible_candidate (nothing was resolved at all)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "--usepkg", "dev-libs/missingdep", "dev-libs/binaryonlypkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    by_package = {e["package"]: e for e in payload["entries"]}
    assert by_package["missingdep"]["source"] == "ebuild"
    assert by_package["binaryonlypkg"]["source"] == "binary"
    assert "source" not in by_package["doesnotexist-anywhere"]


def test_json_dumps_the_whole_graph_unaffected_by_onlydeps(emerge_binary, fixture_env):
    """Unlike the plain-text loop, --json's own output isn't suppressed
    by --onlydeps -- withdeps itself still appears (requested: true),
    letting a consumer filter on "requested" if they want the
    --onlydeps view instead."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--onlydeps", "--json", "dev-libs/withdeps"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    packages = {e["package"] for e in payload["entries"]}
    assert packages == {"withdeps", "newpkg", "upgradepkg"}


def test_json_includes_slot_conflicts(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/slotconflictparent"], fixture_env
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    assert len(payload["slot_conflicts"]) == 1
    conflict = payload["slot_conflicts"][0]
    assert conflict["category"] == "dev-libs"
    assert conflict["package"] == "slotconflicttarget"


def test_virtual_is_resolved_directly(emerge_binary, fixture_env):
    """virtual/texteditor is shaped exactly like a real virtual (e.g.
    virtual/pager in the real Gentoo tree, confirmed by inspection): an
    ordinary ebuild whose RDEPEND is a "|| ( ... )" any-of group of real
    providers, no PROVIDE mechanism or special resolution involved. It
    must resolve through the exact same category + any-of-group
    machinery as any other package -- real "||" semantics pick only
    dev-libs/newpkg (listed first, visible); dev-libs/samepkg (second,
    already installed -- also satisfiable, but never reached at all)."""
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
        "--onlydeps/-o, --update/-u, --deep/-D, --exclude/-X, --deselect/-W, --unmerge/-C, --depclean/-c, --prune/-P, --with-bdeps, --with-bdeps-auto, --changed-deps, --changed-deps-report, --changed-slot, --with-test-deps, --noreplace/-n, --selective, and --help/-h are implemented so far; see PROMPT.md)"
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
        "--onlydeps/-o, --update/-u, --deep/-D, --exclude/-X, --deselect/-W, --unmerge/-C, --depclean/-c, --prune/-P, --with-bdeps, --with-bdeps-auto, --changed-deps, --changed-deps-report, --changed-slot, --with-test-deps, --noreplace/-n, --selective, and --help/-h are implemented so far; see PROMPT.md)"
    )


def test_real_action_not_implemented_message_says_action_not_option(emerge_binary, fixture_env):
    """--search is a real emerge action (see main.py's actions
    frozenset), not an option -- the error must say "action", and its
    short alias -s (see shortmapping) must report the same canonical
    "--search" name. (--depclean/-c and --unmerge/-C used to be the
    example here; both are implemented now.)"""
    result = _run([str(emerge_binary)], ["--search"], fixture_env)
    assert result.returncode == 2
    expected = (
        'emerge (pilot v1): action "--search" is a real emerge action, but is not '
        "implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, "
        "--onlydeps/-o, --update/-u, --deep/-D, --exclude/-X, --deselect/-W, --unmerge/-C, --depclean/-c, --prune/-P, "
        "--with-bdeps, --with-bdeps-auto, --changed-deps, --changed-deps-report, --changed-slot, --with-test-deps, "
        "--noreplace/-n, --selective, and --help/-h are implemented so far; see PROMPT.md)"
    )
    assert result.stderr.strip() == expected

    short_result = _run([str(emerge_binary)], ["-s"], fixture_env)
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
