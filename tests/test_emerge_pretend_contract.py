"""Black-box contract suite for the `emerge --pretend` slice (see
docs/agent-context.md and rust/portage-repo/src/lib.rs for the full
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
docs/agent-context.md's testing decision) and the Python reference implementation
identically, against the synthetic fixture tree at fixtures
(whose repos.conf/make.profile/make.conf/package.mask/package.unmask/
package.accept_keywords/package.use now drive real config resolution,
not hardcoded values, and whose repos.conf now defines a second,
higher-priority overlay repo alongside the main one), and asserts their
stdout, stderr, and exit codes all match exactly.
"""

import json
import shutil
import subprocess

import pytest

# (description, args, expected_exit_code) -- exit codes: 0 success,
# 1 resolution/parse error, 2 CLI-usage error (mirrors both sides' shared
# convention, not real emerge's own exit codes).
CASES = [
    ("new install", ["--pretend", "dev-libs/newpkg"], 0),
    ("already installed", ["--pretend", "dev-libs/samepkg"], 0),
    (
        "a New package shows its full USE=\"...\" list at plain -p (verbosity 2)",
        ["--pretend", "dev-libs/useflagpkg"],
        0,
    ),
    (
        "a New package's USE_EXPAND group shows at plain -p too",
        ["--pretend", "dev-libs/useexpandpkg"],
        0,
    ),
    (
        "an Upgrade shows only its changed USE flags at plain -p",
        ["--pretend", "--update", "dev-libs/upgradeusepkg"],
        0,
    ),
    (
        "the merge list is in dependency-first order (diamond: leaf, consumers, root)",
        ["--pretend", "dev-libs/diamond"],
        0,
    ),
    (
        "merge order threads through --tree too (roots re-derived from required_by)",
        ["--pretend", "--tree", "dev-libs/diamond"],
        0,
    ),
    (
        "--json entries carry an explicit merge_order index",
        ["--pretend", "--json", "dev-libs/diamond"],
        0,
    ),
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
    ("--emptytree: the whole deep tree reinstalls", ["--pretend", "--emptytree", "dev-libs/deeppkg"], 0),
    ("-e short alias for --emptytree", ["--pretend", "-e", "dev-libs/deeppkg"], 0),
    ("-pe bundled", ["-pe", "dev-libs/withdeps"], 0),
    ("--emptytree -v: reinstall counters", ["--pretend", "-v", "--emptytree", "dev-libs/deeppkg"], 0),
    ("--emptytree --update: still upgrades", ["--pretend", "--emptytree", "--update", "dev-libs/withdeps"], 0),
    ("--emptytree --json", ["--pretend", "--emptytree", "--json", "dev-libs/deeppkg"], 0),
    (
        "--reinstall-atoms forces one deep dep to reinstall",
        ["--pretend", "--deep", "--reinstall-atoms", "dev-libs/deeppkg2", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--reinstall-atoms= inline form",
        ["--pretend", "--deep", "--reinstall-atoms=dev-libs/deeppkg2", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--reinstall-atoms wildcard atom",
        ["--pretend", "--deep", "--reinstall-atoms", "dev-libs/*", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--reinstall-atoms repeated + multi-atom value",
        [
            "--pretend", "--deep",
            "--reinstall-atoms", "dev-libs/deeppkg2 dev-libs/nothingmatches",
            "--reinstall-atoms", "=dev-libs/newpkg-1.0",
            "dev-libs/deeppkg",
        ],
        0,
    ),
    (
        "--reinstall-atoms with no value is a usage error",
        ["--pretend", "dev-libs/deeppkg", "--reinstall-atoms"],
        2,
    ),
    ("--reinstall-atoms also reflected in --json", ["--pretend", "--deep", "--json", "--reinstall-atoms", "dev-libs/deeppkg2", "dev-libs/deeppkg"], 0),
    (
        "--rebuild-if-unbuilt: a build-dep upgrade rebuilds its installed consumer",
        ["--pretend", "-u", "--rebuild-if-unbuilt", "dev-libs/rebuildtrigger"],
        0,
    ),
    (
        "--rebuild-if-new-ver: same as unbuilt for a version bump",
        ["--pretend", "-u", "--rebuild-if-new-ver", "dev-libs/rebuildtrigger"],
        0,
    ),
    (
        "--rebuild-if-new-rev inline form",
        ["--pretend", "-u", "--rebuild-if-new-rev=y", "dev-libs/rebuildtrigger"],
        0,
    ),
    (
        "--rebuild-if-new-ver does NOT rebuild for a same-version re-merge",
        ["--pretend", "--rebuild-if-new-ver", "dev-libs/rebuildnochange"],
        0,
    ),
    (
        "--rebuild-if-unbuilt DOES rebuild for a same-version re-merge",
        ["--pretend", "--rebuild-if-unbuilt", "dev-libs/rebuildnochange"],
        0,
    ),
    (
        "--rebuild-exclude keeps the parent out",
        ["--pretend", "-u", "--rebuild-if-unbuilt", "--rebuild-exclude", "dev-libs/rebuildconsumer", "dev-libs/rebuildtrigger"],
        0,
    ),
    (
        "--rebuild-ignore keeps the dep from triggering",
        ["--pretend", "-u", "--rebuild-if-unbuilt", "--rebuild-ignore", "dev-libs/rebuildtrigger", "dev-libs/rebuildtrigger"],
        0,
    ),
    (
        "--rebuild-if-unbuilt=n is a no-op",
        ["--pretend", "-u", "--rebuild-if-unbuilt=n", "dev-libs/rebuildtrigger"],
        0,
    ),
    (
        "--rebuild-if-new-slot=n disables the := slot-op rebuild scan",
        ["--pretend", "--rebuild-if-new-slot=n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--rebuild-exclude with no value is a usage error",
        ["--pretend", "dev-libs/newpkg", "--rebuild-exclude"],
        2,
    ),
    (
        "--dynamic-deps default: an installed deep dep's CURRENT ebuild deps are walked",
        ["--pretend", "-D", "--noreplace", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--dynamic-deps=n: the vdb dep snapshot is walked instead",
        ["--pretend", "-D", "--noreplace", "--dynamic-deps=n", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--dynamic-deps=y is the same as the default",
        ["--pretend", "-D", "--noreplace", "--dynamic-deps=y", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--dynamic-deps is inert without --deep",
        ["--pretend", "--noreplace", "--dynamic-deps=n", "dev-libs/changeddepspkg"],
        0,
    ),
    (
        "--complete-graph forces the deep walk (== -D) even without --deep",
        ["--pretend", "--complete-graph", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--complete-graph=n leaves it as a shallow resolve",
        ["--pretend", "--complete-graph=n", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--complete-graph=y is the same as the bare flag",
        ["--pretend", "--complete-graph=y", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--complete-graph under -pv",
        ["-pv", "--complete-graph", "dev-libs/deeppkg"],
        0,
    ),
    (
        "--complete-graph-if-new-ver auto-enables complete mode on an upgrade",
        ["--pretend", "--update", "dev-libs/completegraphpkg"],
        0,
    ),
    (
        "--complete-graph-if-new-ver=n opts out of that auto-enable",
        ["--pretend", "--update", "--complete-graph-if-new-ver=n", "dev-libs/completegraphpkg"],
        0,
    ),
    (
        "--complete-graph-if-new-use=n alone still lets the version trigger fire",
        ["--pretend", "--update", "--complete-graph-if-new-use=n", "dev-libs/completegraphpkg"],
        0,
    ),
    (
        "--nodeps pops complete back off (no forced deep walk)",
        ["--pretend", "--update", "--nodeps", "--complete-graph", "dev-libs/completegraphpkg"],
        0,
    ),
    (
        "the auto-enable trigger is inert when nothing installed changes",
        ["--pretend", "--noreplace", "dev-libs/completegraphpkg"],
        0,
    ),
    ("-pv: cpv decorated with ::repo", ["--pretend", "-v", "dev-libs/newpkg"], 0),
    ("-pv: :slot/sub_slot decoration on a sub-slotted dep", ["--pretend", "-v", "dev-libs/subslotconsumer"], 0),
    ("-pv: [old-ver] decorated for an Upgrade", ["--pretend", "-v", "--update", "dev-libs/upgradepkg"], 0),
    ("-pv: new-slot other-version list, decorated", ["--pretend", "-v", "dev-libs/newslotpkg:1"], 0),
    ("-pv --columns: decorated version + [old-ver] columns", ["--pretend", "-v", "--columns", "--update", "dev-libs/upgradepkg"], 0),
    ("package.provided: a matching dependency is silently dropped", ["--pretend", "dev-libs/needsprovided"], 0),
    ("package.provided: a matching top-level target triggers the WARNING block", ["--pretend", "dev-libs/providedpkg"], 0),
    ("package.provided: WARNING block coloured", ["--pretend", "--color=y", "dev-libs/providedpkg"], 0),
    ("package.provided: plural WARNING for two matching targets", ["--pretend", "dev-libs/providedpkg", "dev-libs/providedpkg2"], 0),
    ("package.provided: --json unaffected by the dropped dep", ["--pretend", "--json", "dev-libs/needsprovided"], 0),
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
    ("--json: solvable slot conflict reconciled", ["--pretend", "--json", "dev-libs/slotconflictparent"], 0),
    ("--json: unsolvable slot conflict reported", ["--pretend", "--json", "dev-libs/slotconflictunsolvable"], 0),
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
    ("an overlay's own profiles/license_groups stacks with the main repo's", ["--pretend", "dev-libs/crossrepolicensepkg"], 1),
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
    # (A plain `emerge <atom>` with no --pretend is now a real source
    #  build + merge -- a non-dry-run path, Rust-black-box-tested in
    #  test_portuale.py, not exercised via these shared CASES.)
    ("real emerge option, value-taking, not implemented", ["--accept-properties", "dev-libs/newpkg"], 2),
    ("real emerge option, boolean, not implemented", ["--quiet-repo-display", "--pretend", "dev-libs/newpkg"], 2),
    ("real emerge option, inline =value form, not implemented", ["--accept-properties=*", "--pretend", "dev-libs/newpkg"], 2),
    # (`emerge --depclean` / `-c` / `--prune` / `-P` / `-C` with no
    #  --pretend are all real removals now -- non-dry-run paths,
    #  Rust-black-box-tested in test_portuale.py, deliberately not run
    #  through these shared CASES against the read-only fixture ROOT.)
    ("genuinely unrecognized option", ["--totally-fake-option", "dev-libs/newpkg"], 2),
    ("recursion: basic dependency chain", ["--pretend", "dev-libs/withdeps"], 0),
    ("recursion: diamond dependency dedup", ["--pretend", "dev-libs/diamond"], 0),
    ("recursion: dependency cycle terminates", ["--pretend", "dev-libs/cycle-a"], 0),
    (
        "recursion: unbreakable build-time cycle is a fatal error",
        ["--pretend", "dev-libs/hardcyclea"],
        1,
    ),
    (
        "circular dep: USE-flag suggestion (_find_suggestions)",
        ["--pretend", "dev-libs/usecyclea"],
        1,
    ),
    (
        "circular dep: USE-flag suggestion, --color y",
        ["--pretend", "--color", "y", "dev-libs/usecyclea"],
        1,
    ),
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
        "autounmask backward cascade: [flag] on an already-resolved slot re-resolves the whole graph",
        ["--pretend", "dev-libs/aucasctop"],
        0,
    ),
    (
        "autounmask levels: a lower license-masked version beats a higher ~arch one",
        ["--pretend", "--autounmask", "dev-libs/levelconsumer"],
        0,
    ),
    (
        "autounmask backward cascade, -v: the flipped-in dep and the counters line",
        ["--pretend", "-v", "dev-libs/aucasctop"],
        0,
    ),
    (
        "autounmask backward cascade, --autounmask-use=n: no flip, dep stays unresolvable",
        ["--pretend", "--autounmask-use=n", "dev-libs/aucasctop"],
        0,
    ),
    (
        "autounmask backward cascade, --json: the change in autounmask_use_changes",
        ["--pretend", "--json", "dev-libs/aucasctop"],
        0,
    ),
    (
        "autounmask breakage: default (no --autounmask-backtrack) collects the change",
        ["--pretend", "dev-libs/aubreaktop"],
        0,
    ),
    (
        "autounmask breakage, --autounmask-backtrack=y: flag wanted both ways -> abandon",
        ["--pretend", "--autounmask-backtrack=y", "dev-libs/aubreaktop"],
        0,
    ),
    (
        "autounmask breakage, -v",
        ["--pretend", "-v", "dev-libs/aubreaktop"],
        0,
    ),
    (
        "autounmask breakage, --autounmask --autounmask-backtrack=y",
        ["--pretend", "--autounmask", "--autounmask-backtrack=y", "dev-libs/aubreaktop"],
        0,
    ),
    (
        "autounmask backward cascade, --autounmask-backtrack=y: aucascleaf appears",
        ["--pretend", "--autounmask-backtrack=y", "dev-libs/aucasctop"],
        0,
    ),
    (
        "autounmask backward cascade, --autounmask-backtrack=n is the default",
        ["--pretend", "--autounmask-backtrack=n", "dev-libs/aucasctop"],
        0,
    ),
    (
        "autounmask keyword backward cascade: slot narrowed to a ~arch version, default",
        ["--pretend", "dev-libs/kwbacktop"],
        0,
    ),
    (
        "autounmask keyword backward cascade, --autounmask: the slot re-resolves to 2.0",
        ["--pretend", "--autounmask", "dev-libs/kwbacktop"],
        0,
    ),
    (
        "autounmask keyword backward cascade, -pv --autounmask",
        ["--pretend", "-v", "--autounmask", "dev-libs/kwbacktop"],
        0,
    ),
    (
        "autounmask per-level re-scan: ~arch + license unmasked on one version, default",
        ["--pretend", "dev-libs/multimaskconsumer"],
        0,
    ),
    (
        "autounmask per-level re-scan, --autounmask: two categories on the same version",
        ["--pretend", "--autounmask", "dev-libs/multimaskconsumer"],
        0,
    ),
    (
        "autounmask per-level re-scan, -pv --autounmask",
        ["--pretend", "-v", "--autounmask", "dev-libs/multimaskconsumer"],
        0,
    ),
    (
        "USE-dep enforcement: plain flag declared and enabled matches",
        ["--pretend", "dev-libs/useflagpkg[foo]"],
        0,
    ),
    (
        "USE-dep enforcement: negated flag declared but enabled does not match (--autounmask-use=n)",
        ["--pretend", "--autounmask-use=n", "dev-libs/useflagpkg[-foo]"],
        1,
    ),
    (
        "USE-dep enforcement: plain flag declared but disabled does not match (--autounmask-use=n)",
        ["--pretend", "--autounmask-use=n", "dev-libs/useflagpkg[missingflag]"],
        1,
    ),
    (
        "--autounmask-use: a top-level [-flag] mismatch resolves + prints the USE changes block",
        ["--pretend", "dev-libs/useflagpkg[-foo]"],
        0,
    ),
    (
        "--autounmask-use: a top-level [flag] mismatch (flag in IUSE) resolves too",
        ["--pretend", "dev-libs/useflagpkg[missingflag]"],
        0,
    ),
    (
        "--autounmask-use: an opt= dep whose child flag is masked flips the parent instead, exit 0",
        ["--pretend", "dev-libs/parentflipeqpkg"],
        0,
    ),
    (
        "--autounmask-use=n: the masked-child opt= dep stays unresolvable (top-level still merges)",
        ["--pretend", "--autounmask-use=n", "dev-libs/parentflipeqpkg"],
        0,
    ),
    (
        "--autounmask-use parent flip, default: single-dep re-resolve, pf? dep stays",
        ["--pretend", "dev-libs/pfgraphparent"],
        0,
    ),
    (
        "--autounmask-use parent flip, --autounmask-backtrack=y: whole-graph, pf? dep drops",
        ["--pretend", "--autounmask-backtrack=y", "dev-libs/pfgraphparent"],
        0,
    ),
    (
        "--autounmask-use parent flip, -pv",
        ["--pretend", "-v", "dev-libs/pfgraphparent"],
        0,
    ),
    (
        "--autounmask-use parent flip, --autounmask-use=n",
        ["--pretend", "--autounmask-use=n", "dev-libs/pfgraphparent"],
        0,
    ),
    (
        "USE-dep enforcement: negated flag declared and disabled matches",
        ["--pretend", "dev-libs/useflagpkg[-missingflag]"],
        0,
    ),
    (
        "USE-dep enforcement: flag not declared in IUSE at all, no default, never matches (unfixable)",
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
        "--alphabetical collapses the enabled-first USE split into one bare-name-sorted list",
        ["--pretend", "-v", "--alphabetical", "dev-libs/iusedefaultpkg"],
        0,
    ),
    (
        "-pv USE= flag list is natural-sorted (_alnum_sort_key): n9 before n10",
        ["--pretend", "-v", "dev-libs/naturalsortpkg"],
        0,
    ),
    (
        "-pv --alphabetical USE= list is natural-sorted too",
        ["--pretend", "-v", "--alphabetical", "dev-libs/naturalsortpkg"],
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
        "--autounmask: keyword-masked target resolves + prints the changes block once enabled",
        ["--pretend", "--autounmask", "dev-libs/autounmaskkeywordpkg"],
        0,
    ),
    (
        "--autounmask: a dependency's own no-visible-candidate gets no suggestion by default",
        ["--pretend", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--autounmask: a keyword-masked dependency resolves + prints the changes block once enabled",
        ["--pretend", "--autounmask", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--autounmask: keyword changes also appear in --json",
        ["--pretend", "--autounmask", "--json", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--autounmask-license: a EULA-masked top-level target is fatal by default",
        ["--pretend", "dev-libs/licensemaskedpkg"],
        1,
    ),
    (
        "--autounmask-license: --autounmask resolves it + prints the license block",
        ["--pretend", "--autounmask", "dev-libs/licensemaskedconsumer"],
        0,
    ),
    (
        "--autounmask-license=y alone enables the license block",
        ["--pretend", "--autounmask-license=y", "dev-libs/licensemaskedpkg"],
        0,
    ),
    (
        "--autounmask-license=n over --autounmask suppresses it",
        ["--pretend", "--autounmask", "--autounmask-license=n", "dev-libs/licensemaskedpkg"],
        1,
    ),
    (
        "--autounmask-license: change also appears in --json",
        ["--pretend", "--autounmask", "--json", "dev-libs/licensemaskedconsumer"],
        0,
    ),
    (
        "--autounmask-keep-masks: a package.mask'd top-level target is fatal by default",
        ["--pretend", "dev-libs/hardmaskedpkg"],
        1,
    ),
    (
        "--autounmask-keep-masks=n resolves a package.mask'd target + prints the mask block",
        ["--pretend", "--autounmask-keep-masks=n", "dev-libs/hardmaskedpkg"],
        0,
    ),
    (
        "--autounmask-keep-masks=n on a package.mask'd dependency",
        ["--pretend", "--autounmask-keep-masks=n", "dev-libs/maskmaskedconsumer"],
        0,
    ),
    (
        "--autounmask alone does NOT unmask package.mask (masks kept by default)",
        ["--pretend", "--autounmask", "dev-libs/hardmaskedpkg"],
        1,
    ),
    (
        "--autounmask-keep-masks: change also appears in --json",
        ["--pretend", "--autounmask-keep-masks=n", "--json", "dev-libs/maskmaskedconsumer"],
        0,
    ),
    (
        "--autounmask-only: only the changes block, no merge list",
        ["--pretend", "--autounmask", "--autounmask-only", "dev-libs/autounmaskkeywordpkg"],
        0,
    ),
    (
        "--autounmask-only=y: same, explicit value form",
        ["--pretend", "--autounmask-only=y", "dev-libs/autounmaskdepconsumer"],
        0,
    ),
    (
        "--autounmask-only=n: back to the normal merge list",
        ["--pretend", "--autounmask-only=n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-only: a plain package still prints nothing but exits 0",
        ["--pretend", "--autounmask-only", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-continue + --autounmask=n prints the actions.py:3772 warning",
        ["--pretend", "--autounmask-continue", "--autounmask=n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-continue alone is inert under --pretend",
        ["--pretend", "--autounmask-continue", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-continue=n still trips the --autounmask=n warning (flag was given)",
        ["--pretend", "--autounmask-continue=n", "--autounmask=n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-backtrack y is recognized and inert",
        ["--pretend", "--autounmask-backtrack", "y", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-backtrack=n inline form",
        ["--pretend", "--autounmask-backtrack=n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--autounmask-backtrack rejects a non-y/n value",
        ["--pretend", "--autounmask-backtrack", "maybe", "dev-libs/newpkg"],
        2,
    ),
    (
        "--autounmask-backtrack with no argument is a usage error",
        ["--pretend", "dev-libs/newpkg", "--autounmask-backtrack"],
        2,
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
        "avoid_update: a DEPENDENCY whose USE-dep flag is only in its built USE (bug 640318) is kept",
        ["--pretend", "dev-libs/needsbuiltusediverge"],
        0,
    ),
    (
        "the same [divergedflag] atom as a TOP-LEVEL target still needs a visible ebuild",
        ["--pretend", "dev-libs/builtusedivergedep[divergedflag]"],
        1,
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
        "--useoldpkg-atoms: prefer the old binary over the newer ebuild",
        ["--pretend", "--usepkg", "--useoldpkg-atoms", "dev-libs/useoldpkgpkg", "dev-libs/useoldpkgpkg"],
        0,
    ),
    (
        "--useoldpkg-atoms: the newer ebuild still wins by default",
        ["--pretend", "--usepkg", "dev-libs/useoldpkgpkg"],
        0,
    ),
    (
        "--useoldpkg-atoms: inert without --usepkg (no binary in the pool)",
        ["--pretend", "--useoldpkg-atoms", "dev-libs/useoldpkgpkg", "dev-libs/useoldpkgpkg"],
        0,
    ),
    (
        "--useoldpkg-atoms: a wildcard atom matches",
        ["--pretend", "--usepkg", "--useoldpkg-atoms", "dev-libs/*", "dev-libs/useoldpkgpkg"],
        0,
    ),
    (
        "--useoldpkg-atoms=ATOM inline form",
        ["--pretend", "--usepkg", "--useoldpkg-atoms=dev-libs/useoldpkgpkg", "dev-libs/useoldpkgpkg"],
        0,
    ),
    (
        "--useoldpkg-atoms: a non-matching atom leaves the ebuild winning",
        ["--pretend", "--usepkg", "--useoldpkg-atoms", "dev-libs/binaryonlypkg", "dev-libs/useoldpkgpkg"],
        0,
    ),
    (
        "--useoldpkg-atoms with no argument is a usage error",
        ["--pretend", "dev-libs/useoldpkgpkg", "--useoldpkg-atoms"],
        2,
    ),
    (
        "--quickpkg-direct=y is inert without --usepkg",
        ["--pretend", "--quickpkg-direct=y", "dev-libs/newpkg"],
        0,
    ),
    (
        "--quickpkg-direct=n is inert",
        ["--pretend", "--usepkg", "--quickpkg-direct=n", "dev-libs/newpkg"],
        0,
    ),
    (
        "--quickpkg-direct rejects a non-y/n value",
        ["--pretend", "--quickpkg-direct", "maybe", "dev-libs/newpkg"],
        2,
    ),
    (
        "--quickpkg-direct with no argument is a usage error",
        ["--pretend", "dev-libs/newpkg", "--quickpkg-direct"],
        2,
    ),
    (
        "--quickpkg-direct-root with no argument is a usage error",
        ["--pretend", "dev-libs/newpkg", "--quickpkg-direct-root"],
        2,
    ),
    (
        "--regen rejects --pretend (real actions.py:4106)",
        ["--pretend", "--regen"],
        1,
    ),
    (
        "--metadata rejects --pretend",
        ["--pretend", "--metadata"],
        1,
    ),
    (
        "--metadata without --pretend prints the cache-update header",
        ["--metadata"],
        0,
    ),
    (
        "--sync points at `emaint sync` (a permanent non-goal), with or without --pretend",
        ["--sync"],
        1,
    ),
    (
        "--sync + --pretend: same `emaint sync` message",
        ["--pretend", "--sync"],
        1,
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
        "--getbinpkg: a remote-only binhost binary becomes eligible",
        ["--pretend", "--getbinpkg", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "-g short alias for --getbinpkg",
        ["--pretend", "-g", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "--getbinpkg=y inline form",
        ["--pretend", "--getbinpkg=y", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "--getbinpkgonly: binary-only, still resolves the remote binhost binary",
        ["--pretend", "--getbinpkgonly", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "-G short alias for --getbinpkgonly",
        ["--pretend", "-G", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "-pG bundled",
        ["-pG", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "--getbinpkg -v: the `g` bracket column + Size of downloads + ::repo",
        ["--pretend", "-v", "--getbinpkg", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "--getbinpkg -v: :slot/::repo decoration on a [binary ... g] line",
        ["--pretend", "-v", "--getbinpkg", "dev-libs/remotebinslotpkg"],
        0,
    ),
    (
        "--getbinpkg -v --columns: g column with the decorated version column",
        ["--pretend", "-v", "--columns", "--getbinpkg", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "--getbinpkg --json: source stays \"binary\" for a remote binhost pick",
        ["--pretend", "--json", "--getbinpkg", "dev-libs/remotebinpkg"],
        0,
    ),
    (
        "--getbinpkg=n leaves a remote-only binhost binary invisible",
        ["--pretend", "--getbinpkg=n", "dev-libs/remotebinpkg"],
        1,
    ),
    (
        "--usepkg alone does not pull remote binhost candidates",
        ["--pretend", "--usepkg", "dev-libs/remotebinpkg"],
        1,
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
    ("package.use depth: repo-level entry loses to the profile make.defaults", ["--pretend", "-v", "dev-libs/repouseweakpkg"], 0),
    ("package.use depth: profile-level entry loses to make.conf", ["--pretend", "-v", "dev-libs/profileuseweakpkg"], 0),
    ("package.env: env-file USE= enables a flag, pulling in a dependency", ["--pretend", "dev-libs/penvpkg"], 0),
    ("repo make.defaults: USE= enables a flag, pulling in a dependency", ["--pretend", "-v", "dev-libs/repomakedefaultpkg"], 0),
    ("env.d: /etc/profile.env USE= enables a flag, pulling in a dependency", ["--pretend", "-v", "dev-libs/envdusepkg"], 0),
    ("bare name: a unique package name is category-qualified", ["--pretend", "newpkg"], 0),
    ("bare name: ambiguous across two categories is rejected", ["--pretend", "ambigpkg"], 1),
    ("bare name: no match reports 'no ebuilds to satisfy'", ["--pretend", "nosuchpkgname"], 1),
    ("bare name + version: dep_expand splices the category (=cat/pkg-ver)", ["--pretend", "newpkg-1.0"], 0),
    ("bare name + operator + version: dep_expand splices the category", ["--pretend", ">=newpkg-1.0"], 0),
    ("bare name + slot: dep_expand splices the category", ["--pretend", "newpkg:0"], 0),
    ("bare name + nonexistent version: 'no ebuilds' on the expanded atom", ["--pretend", "newpkg-9.9"], 1),
    ("bare name + version, ambiguous: still rejected after stripping the version", ["--pretend", "ambigpkg-1.0"], 1),
    ("profile defaults walk: a leaf make.defaults cancels a parent package.use", ["--pretend", "-v", "dev-libs/interleavepkg"], 0),
    ("blocker: strong (!!) blocker matches an installed package", ["--pretend", "dev-libs/blockerpkg"], 0),
    ("blocker: weak (!) blocker matches another new package in the graph", ["--pretend", "dev-libs/graphblockerparent"], 0),
    ("blocker: -v widens the [blocks B ] bracket by the mask column", ["--pretend", "-v", "dev-libs/blockerpkg"], 0),
    ("blocker: line prints after every package line, not inline", ["--pretend", "dev-libs/blockerorderpkg"], 0),
    ("blocker: --color=y colours the [blocks B ] line (PKG_BLOCKER red)", ["--pretend", "--color=y", "dev-libs/blockerpkg"], 0),
    ("blocker: --color=y -v coloured + widened", ["--pretend", "--color=y", "-v", "dev-libs/blockerorderpkg"], 0),
    ("blocker: --tree still ends with the deferred [blocks B ] line", ["--pretend", "--tree", "dev-libs/blockerorderpkg"], 0),
    ("blocker: --json blocker payload is unchanged by the line reformat", ["--pretend", "--json", "dev-libs/blockerorderpkg"], 0),
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
    ("slot conflict: solvable, reconciled by backtracking", ["--pretend", "dev-libs/slotconflictparent"], 0),
    ("slot conflict: --backtrack=0 disables reconciliation", ["--pretend", "--backtrack=0", "dev-libs/slotconflictparent"], 0),
    ("slot conflict: --backtrack 1 still reconciles a one-step conflict", ["--pretend", "--backtrack", "1", "dev-libs/slotconflictparent"], 0),
    ("slot conflict: unsolvable, survives backtracking and is reported", ["--pretend", "dev-libs/slotconflictunsolvable"], 0),
    ("slot conflict: unsolvable, resolved by masking a puller version", ["--pretend", "dev-libs/btparent"], 0),
    ("slot conflict: --backtrack=0 also disables the runtime_pkg_mask trial", ["--pretend", "--backtrack=0", "dev-libs/btparent"], 0),
    ("slot conflict: --backtrack=30 suppresses the try-a-larger-value hint", ["--pretend", "--backtrack=30", "dev-libs/slotconflictunsolvable"], 0),
    ("slot conflict: three same-reason parents collapse to one + '(and N more)'", ["--pretend", "dev-libs/slotconfgroup"], 0),
    ("slot conflict: --verbose-conflicts shows every omitted parent", ["--pretend", "--verbose-conflicts", "dev-libs/slotconfgroup"], 0),
    ("slot conflict: --verbose-conflicts=n is the default (collapsed)", ["--pretend", "--verbose-conflicts=n", "dev-libs/slotconfgroup"], 0),
    ("slot conflict: different slots of the same package coexist", ["--pretend", "dev-libs/multislotparent"], 0),
    ("virtual: resolved directly", ["--pretend", "virtual/texteditor"], 0),
    ("virtual: resolved as a dependency", ["--pretend", "dev-libs/virtualconsumerpkg"], 0),
    ("multi-atom: two independent new packages", ["--pretend", "dev-libs/newpkg", "dev-libs/withdeps"], 0),
    ("multi-atom: literal duplicate atom dedupes silently", ["--pretend", "dev-libs/newpkg", "dev-libs/newpkg"], 0),
    ("multi-atom: dependency shared between two targets dedupes", ["--pretend", "dev-libs/shared-a", "dev-libs/shared-b"], 0),
    ("multi-atom: solvable slot conflict between two targets is reconciled", ["--pretend", "dev-libs/slotconflictnewconsumer", "dev-libs/slotconflictoldconsumer"], 0),
    ("multi-atom: unsolvable slot conflict between two targets is reported", ["--pretend", "dev-libs/slotconflictnewpin", "dev-libs/slotconflictoldpin"], 0),
    ("multi-atom: all requested atoms already installed", ["--pretend", "dev-libs/samepkg", "dev-libs/samepkg"], 0),
    ("multi-atom: a nonexistent atom aborts the whole run, first-bad-wins", ["--pretend", "dev-libs/does-not-exist", "dev-libs/newpkg"], 1),
    ("multi-atom: a later nonexistent atom still aborts the whole run", ["--pretend", "dev-libs/newpkg", "dev-libs/does-not-exist"], 1),
    ("--misspell-suggestions: a near-miss package name gets suggestions", ["--pretend", "dev-libs/newpgk"], 1),
    ("--misspell-suggestions=n: no suggestions", ["--pretend", "--misspell-suggestions=n", "dev-libs/newpgk"], 1),
    ("--misspell-suggestions: a masked (existing) cp gets no name suggestions", ["--pretend", "dev-libs/autounmaskkeywordpkg"], 1),
    ("--debug: recognized, byte-for-byte no-op under --pretend", ["--pretend", "--debug", "dev-libs/newpkg"], 0),
    ("-d: --debug short alias, recognized", ["--pretend", "-d", "dev-libs/newpkg"], 0),
    ("-pd: --debug bundles with -p", ["-pd", "dev-libs/newpkg"], 0),
    ("--debug + a slot conflict: still no resolver debug trace", ["--pretend", "--debug", "dev-libs/slotconfgroup"], 0),
    ("--verbose is now implemented, not rejected", ["--pretend", "--verbose", "dev-libs/newpkg"], 0),
    ("-v short alias is now implemented, not rejected", ["--pretend", "-v", "dev-libs/newpkg"], 0),
    ("without --verbose, USE= is never shown even for a package with IUSE", ["--pretend", "dev-libs/useflagpkg"], 0),
    ("-v on a package with no IUSE at all: no USE= line", ["--pretend", "-v", "dev-libs/newpkg"], 0),
    ("-v combined with a real-but-unimplemented option: still rejected", ["--pretend", "-v", "--accept-properties", "dev-libs/newpkg"], 2),
    ("-v explicit disable via a following \"n\" token", ["--pretend", "-v", "n", "dev-libs/useflagpkg"], 0),
    ("-v explicit enable via a following \"y\" token", ["--pretend", "-v", "y", "dev-libs/useflagpkg"], 0),
    ("--verbose=n inline form disables", ["--pretend", "--verbose=n", "dev-libs/useflagpkg"], 0),
    ("--verbose=y inline form enables", ["--pretend", "--verbose=y", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -pv: both implemented flags", ["-pv", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -vp: order doesn't matter", ["-vp", "dev-libs/useflagpkg"], 0),
    ("short-flag bundle -pf: pretend + unimplemented option", ["-pf", "dev-libs/useflagpkg"], 2),
    ("short-flag bundle -pz: pretend + genuinely unrecognized", ["-pz", "dev-libs/useflagpkg"], 2),
    ("bundled -v never consumes a following token as its value", ["-pv", "n"], 1),
    ("--help is now implemented, not rejected", ["--help"], 0),
    ("-h short alias is now implemented, not rejected", ["-h"], 0),
    ("--help wins over any other flag present, valid or not", ["--accept-properties", "--help"], 0),
    ("-h bundled with other short flags still wins", ["-ph"], 0),
    ("--help wins even without --pretend at all", ["--help", "dev-libs/newpkg"], 0),
    (
        "--shell (portuale-only) is accepted and inert under --pretend",
        ["--pretend", "--shell", "brush", "dev-libs/newpkg"],
        0,
    ),
    (
        "--shell=brush inline form, also inert under --pretend",
        ["--pretend", "--shell=bash", "dev-libs/newpkg"],
        0,
    ),
    (
        "--shell rejects a value that isn't bash or brush",
        ["--pretend", "--shell", "zsh", "dev-libs/newpkg"],
        1,
    ),
    (
        "--shell with no value is a usage error",
        ["--pretend", "dev-libs/newpkg", "--shell"],
        2,
    ),
    ("@world expands to the fixture world file's own atoms", ["--pretend", "@world"], 0),
    ("@world combined with an explicit atom too", ["--pretend", "dev-libs/samepkg", "@world"], 0),
    ("@system expands to the fixture profile chain's own packages files", ["--pretend", "@system"], 0),
    ("@system combined with an explicit atom too", ["--pretend", "dev-libs/samepkg", "@system"], 0),
    ("a user-defined set given directly expands to its members", ["--pretend", "@nestedtestset"], 0),
    ("a user-defined set combined with an explicit atom too", ["--pretend", "dev-libs/samepkg", "@nestedtestset"], 0),
    ("@selected expands like portuale's @world", ["--pretend", "--update", "@selected"], 0),
    ("@selected combined with an explicit atom too", ["--pretend", "--update", "dev-libs/samepkg", "@selected"], 0),
    ("an unknown @set name is a real error", ["--pretend", "@some-other-set"], 1),
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
        "reinst_flags: a flag dropped from IUSE that triggered the reinstall shows at plain -p",
        ["--pretend", "--newuse", "dev-libs/reinstdropiusepkg"],
        0,
    ),
    (
        "reinst_flags: same flag under --changed-use (it was enabled, so also a trigger)",
        ["--pretend", "--changed-use", "dev-libs/reinstdropiusepkg"],
        0,
    ),
    (
        "reinst_flags: -pv is unaffected -- it already showed every flag",
        ["--pretend", "-v", "--newuse", "dev-libs/reinstdropiusepkg"],
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
    # (slot-operator rebuild edges need an installed consumer with a
    #  stale `:S/SS=` binding -- set up in a test-local vdb, not the
    #  shared fixture whose whole vdb feeds every depclean/prune/-C test.
    #  Rust-vs-Python lockstep in
    #  test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer.)
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
        "-pC lower slot: a higher-slot install covers the set atom, no warning",
        ["--pretend", "-C", "dev-libs/dualslotpkg:1"],
        0,
    ),
    (
        "-pC higher slot: nothing higher covers the set atom, warning shown",
        ["--pretend", "-C", "dev-libs/dualslotpkg:2"],
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
    # --color=y: the explicit override that wins over NO_COLOR/isatty, so
    # these stay deterministic under a captured (piped) stdout.
    (
        "--color=y: a New (system member -> PKG_MERGE_SYSTEM) + its dep",
        ["--pretend", "--color=y", "dev-libs/diamond"],
        0,
    ),
    (
        "--color=y: an Upgrade (turquoise U, blue [old-ver], PKG_MERGE_WORLD)",
        ["--pretend", "--color=y", "--update", "dev-libs/upgradepkg"],
        0,
    ),
    (
        "--oneshot: a favorite is no longer world-coloured (PKG_MERGE, not PKG_MERGE_WORLD)",
        ["--pretend", "--color=y", "--oneshot", "--update", "dev-libs/upgradepkg"],
        0,
    ),
    (
        "--oneshot short alias -1, bundled with -p; plain text is identical",
        ["-p1", "dev-libs/newpkg"],
        0,
    ),
    (
        "--color=y -pv: coloured USE_EXPAND line + green N",
        ["--pretend", "-v", "--color=y", "dev-libs/useexpandpkg"],
        0,
    ),
    (
        "--color=y -pv: the mask column is coloured (WARN ~)",
        ["--pretend", "-v", "--color=y", "dev-libs/bareacceptkeywordspkg"],
        0,
    ),
    (
        "--color=y -pv: USE= tokens are coloured (red/blue/green/yellow + plain markers)",
        ["--pretend", "-v", "--color=y", "--update", "dev-libs/upgradeusepkg"],
        0,
    ),
    (
        "--color=y -pv --alphabetical: colour applied after the re-sort",
        ["--pretend", "-v", "--color=y", "--alphabetical", "dev-libs/iusedefaultpkg"],
        0,
    ),
    (
        "--color=y -pC: coloured _unmerge_display (selected red, legend, system warning)",
        ["--pretend", "-C", "--color=y", "dev-libs/systempkg"],
        0,
    ),
    (
        "--color=y -pc: coloured advisory block (WARN ` * `, green backtick commands)",
        ["--pretend", "-c", "--color=y"],
        0,
    ),
    (
        "--color=y -pv: the counters line's `interactive` word is WARN",
        ["--pretend", "-v", "--color=y", "dev-libs/interactivemergepkg"],
        0,
    ),
    (
        "--color=y --columns: nc_len keeps the coloured line aligned",
        ["--pretend", "--color=y", "--columns", "--update", "dev-libs/upgradepkg"],
        0,
    ),
    (
        "--color=y --tree: the marker survives the indent",
        ["--pretend", "--color=y", "--tree", "dev-libs/diamond"],
        0,
    ),
    (
        "--color=n: explicitly disabled",
        ["--pretend", "--color=n", "dev-libs/newpkg"],
        0,
    ),
]


def _run(cmd: list[str], args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        [*cmd, *args], capture_output=True, text=True, env=env, check=False
    )


_SLOT_COLLISION_PREAMBLE = (
    "!!! Multiple package instances within a single package slot have been pulled"
)


def _assert_slot_collision_block(stdout, slot_atom, instances, backtrack_hint=True):
    """`instances` is a list of (cpv, [(parent_cpv_or_None, atom), ...]).
    Checks the real `get_conflict()` `!!! Multiple package instances ...`
    block rather than pinning the whole multi-line paragraph in every
    test. Each parent atom is followed by its ` USE=""` slot (real
    `pkg_use_display`) and a `^` marker line (real `highlight_violations`)
    -- both checked here structurally; the exact caret columns are pinned
    once in the slotconfgroup test."""
    assert _SLOT_COLLISION_PREAMBLE in stdout
    assert f"\n{slot_atom}\n" in stdout
    for cpv, parents in instances:
        assert (
            f'  ({cpv}, ebuild scheduled for merge) USE="" pulled in by\n' in stdout
        )
        for parent_cpv, atom in parents:
            if parent_cpv is None:
                assert f"    {atom} (Argument)\n" in stdout
            else:
                line = (
                    f'    {atom} required by ({parent_cpv}, '
                    f'ebuild scheduled for merge) USE=""\n'
                )
                assert line in stdout
                # the `^` marker line immediately follows
                after = stdout.split(line, 1)[1]
                marker = after.split("\n", 1)[0]
                assert marker.startswith("    ") and set(marker) <= {" ", "^"}
                assert "^" in marker
    assert (
        "It may be possible to solve this problem by using package.mask to" in stdout
    )
    hint = "such as --backtrack=30" in stdout
    assert hint == backtrack_hint


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
    vdb entry. PORTAGE_RUNNING_ROOT (a portuale-specific, test-only override
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
    assert rust_with.stdout.strip() == "[ebuild  N     ] dev-libs/rootdepspkg-1.0"


def test_bdepend_routes_to_the_running_root_for_a_cross_root_build_without_root_deps(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """Real EAPI-7+ portage resolves BDEPEND/IDEPEND against the running
    root `/` unconditionally, not only under --root-deps -- observable for
    a genuine cross-root build (ROOT != running root). Here ROOT is an
    empty tmp tree and PORTAGE_RUNNING_ROOT points at the fixture tree
    (whose hand-seeded vdb has dev-libs/rootdepsprovider). rootdepspkg's
    own BDEPEND (dev-libs/rootdepsprovider, no ebuild anywhere) is
    therefore satisfied by the running root and silently dropped -- no
    --root-deps needed, no "no visible ebuild" note. Same command with
    ROOT == running root falls back to the plain unresolved-dep report."""
    cross = dict(fixture_env)
    cross["ROOT"] = str(tmp_path)
    cross["PORTAGE_RUNNING_ROOT"] = str(fixture_env["ROOT"])
    args = ["--pretend", "dev-libs/rootdepspkg"]

    rust = _run([str(emerge_binary)], args, cross)
    python = _run(emerge_pretend_python, args, cross)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stderr == ""
    assert rust.stdout.strip() == "[ebuild  N     ] dev-libs/rootdepspkg-1.0"

    # Control: running root == target ROOT -> the feature is a no-op and
    # the unresolved BDEPEND is reported as before.
    same = dict(fixture_env)  # fixture_env pins RUNNING_ROOT == ROOT
    rust_same = _run([str(emerge_binary)], args, same)
    assert "no visible ebuild for dependency" in rust_same.stderr


def test_root_deps_disjunctive_branch_selection_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--root-deps branch-selection feed-in: rootdepsorpkg's own BDEPEND
    is "|| ( dev-libs/rootdepsnonexistent dev-libs/rootdepsprovider )" --
    neither branch has an ebuild anywhere in the fixture repo tree, so
    without --root-deps no branch resolves at all and *both* are reported
    as unresolvable dependencies (portuale's own pre-existing "leave an
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
    assert rust_with.stdout.strip() == "[ebuild  N     ] dev-libs/rootdepsorpkg-1.0"


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
    --root-deps via portuale's own pre-existing (unrelated to this
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
        (
        '[ebuild  N     ] dev-libs/rootdepsbuildtool-1.0 \n'
        '[ebuild  N     ] dev-libs/rootdepsbuildpkg-1.0'
        )
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
        f"[ebuild  N     ] dev-libs/rootdepsbuildtool-1.0 to {env['ROOT']}\n"
        "[ebuild  N     ] dev-libs/rootdepsbuildpkg-1.0 \n"
    )
    assert rust_with.stdout != rust_without.stdout


def test_root_deps_build_entry_output_marks_the_running_root(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """The " to <running root>" marker portuale adds to a --root-deps
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
        (
        '[ebuild  N     ] dev-libs/rootdepsbuildtool-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rootdepsbuildpkg-1.0 \n'
        )
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
        "[ebuild  N     ] dev-libs/rootdepsbuildpkg-1.0 \n"
        "[ebuild  N     ]   dev-libs/rootdepsbuildtool-1.0 to /\n"
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
        (
        '[ebuild  N     ] dev-libs/rdrtooldep-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdrlib-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdrtool-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdrapp-1.0 \n'
        )
    )

    rust_tree = _run([str(emerge_binary)], [*base, "--tree"], env)
    python_tree = _run(emerge_pretend_python, [*base, "--tree"], env)
    assert rust_tree.stdout == python_tree.stdout
    assert rust_tree.stdout == (
        "[ebuild  N     ] dev-libs/rdrapp-1.0 \n"
        "[ebuild  N     ]   dev-libs/rdrtool-1.0 to /\n"
        "[ebuild  N     ]     dev-libs/rdrlib-1.0 to /\n"
        "[ebuild  N     ]     dev-libs/rdrtooldep-1.0 to /\n"
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
        (
        '[ebuild  N     ] dev-libs/rdrilib-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdritool-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdriapp-1.0 \n'
        )
    )


def test_root_deps_top_level_idepend_resolves_against_the_running_root(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real portage resolves IDEPEND (and BDEPEND) against the running
    root for *every* package, not just under --root-deps and not just for
    recursed running-root build entries (depgraph.py:4247-4252). The two
    ordinary dep-walk sites route a top-level package's own IDEPEND to the
    running root whenever the running root differs from the target ROOT (a
    cross-root/stage build); --root-deps forces the same routing even when
    the two coincide. topidepapp IDEPENDs topideplib.

    Case 1 -- cross-root (PORTAGE_RUNNING_ROOT=/ != ROOT=fixtures), no
    --root-deps: topideplib already carries the " to /" marker.
    Case 2 -- same, with --root-deps: identical (nothing more to add).
    Case 3 -- running root == target ROOT, no --root-deps: a plain
    ROOT-targeted entry -- the feature is a strict no-op, --root-deps is
    the only trigger then (and it prints the non-deterministic ROOT path,
    so that combination is left to the "/"-pinned tests above)."""
    cross = dict(fixture_env)
    cross["PORTAGE_RUNNING_ROOT"] = "/"
    same = dict(fixture_env)  # fixture_env already pins RUNNING_ROOT == ROOT

    cross_out = (
        '[ebuild  N     ] dev-libs/topideplib-1.0 to /\n'
        '[ebuild  N     ] dev-libs/topidepapp-1.0 \n'
    )
    plain_out = (
        '[ebuild  N     ] dev-libs/topideplib-1.0 \n'
        '[ebuild  N     ] dev-libs/topidepapp-1.0 \n'
    )

    for args, env, expected in [
        (["--pretend", "dev-libs/topidepapp"], cross, cross_out),
        (["--pretend", "--root-deps", "dev-libs/topidepapp"], cross, cross_out),
        (["--pretend", "dev-libs/topidepapp"], same, plain_out),
    ]:
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == 0
        assert python.returncode == 0
        assert rust.stdout == python.stdout
        assert rust.stderr == python.stderr
        assert rust.stdout == expected, (args, env["PORTAGE_RUNNING_ROOT"])


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
        (
        '[ebuild  N     ] dev-libs/rdrcycb-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdrcyca-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdrcyc-1.0 \n'
        )
    )


def test_unbreakable_build_time_cycle_prints_the_circular_deps_error(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """hardcyclea DEPENDs hardcycleb which DEPENDs hardcyclea, both
    unbuilt, empty RDEPEND, no IUSE -- every edge an unsatisfied
    build-time dep with no run-time alternative, so real portage's
    `_ignore_runtime` scan can't linearize it. The full merge list still
    goes to stdout; the `* Error: circular dependencies:` block (real
    `_show_circular_deps`, minus the reduced --tree re-display) goes to
    stderr; exit 1. With no IUSE, `_find_suggestions` finds nothing and
    the generic advisory prints (the `else` branch) -- see
    test_circular_dep_use_flag_suggestion for the suggestion path. By
    contrast the pure-RDEPEND cycle-a/cycle-b cycle stays exit 0 (a
    CASES entry)."""
    base = ["--pretend", "dev-libs/hardcyclea"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    python = _run(emerge_pretend_python, base, fixture_env)

    assert rust.returncode == 1
    assert python.returncode == 1
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout == (
        "[ebuild  N     ] dev-libs/hardcyclea-1.0 \n"
        "[ebuild  N     ] dev-libs/hardcycleb-1.0 \n"
    )
    assert rust.stderr == (
        "\n * Error: circular dependencies:\n"
        "\n"
        "dev-libs/hardcyclea-1.0 depends on\n"
        " dev-libs/hardcycleb-1.0 (buildtime)\n"
        "  dev-libs/hardcyclea-1.0 (buildtime)\n"
        "\n"
        " * Note that circular dependencies can often be avoided by temporarily\n"
        " * disabling USE flags that trigger optional dependencies.\n"
    )


def test_circular_dep_use_flag_suggestion(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """find-suggestions Slice 2: dev-libs/usecyclea (IUSE +x) build-depends
    on dev-libs/usecycleb only when x is on; usecycleb unconditionally
    build-depends back. `circular_dependency_handler._find_suggestions`
    finds that disabling x on usecyclea drops the offending atom without
    violating REQUIRED_USE, so real prints `It might be possible to break
    this cycle / by applying the following change: / - dev-libs/
    usecyclea-1.0 (Change USE: -x)` instead of the generic advisory.
    Rust==Python byte-identical; the `-x` renders blue under --color y."""
    base = ["--pretend", "dev-libs/usecyclea"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    py = _run(emerge_pretend_python, base, fixture_env)
    assert rust.returncode == 1 and py.returncode == 1
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stderr == (
        "\n * Error: circular dependencies:\n"
        "\n"
        "dev-libs/usecyclea-1.0 depends on\n"
        " dev-libs/usecycleb-1.0 (buildtime)\n"
        "  dev-libs/usecyclea-1.0 (buildtime)\n"
        "\n"
        "It might be possible to break this cycle\n"
        "by applying the following change:\n"
        "- dev-libs/usecyclea-1.0 (Change USE: -x)\n"
        "\n"
        "Note that this change can be reverted, once the package has been installed.\n"
    )

    # --color y: the -x flag renders blue (real colorize("blue", ...))
    c = ["--pretend", "--color", "y", "dev-libs/usecyclea"]
    rc = _run([str(emerge_binary)], c, fixture_env)
    cpy = _run(emerge_pretend_python, c, fixture_env)
    assert rc.stderr == cpy.stderr
    assert "(Change USE: \x1b[34;01m-x\x1b[39;49;00m)" in rc.stderr


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
        (
        '[ebuild  N     ] dev-libs/rdrmisstool-1.0 to /\n'
        '[ebuild  N     ] dev-libs/rdrmiss-1.0 \n'
        )
    )
    assert '!!! no visible ebuild for dependency "dev-libs/rdrnothere"' in rust.stderr


def test_diamond_dependency_is_deduped_and_ordered(emerge_binary, fixture_env):
    """Pins the exact recursion output for the diamond fixture (diamond ->
    shared-a, shared-b -> common): "common" appears exactly once despite
    being reachable two ways, and the list is in real portage's
    dependency-first *merge* order (portage_repo::topological_merge_order,
    mirrored _topological_merge_order) -- the shared leaf `common` first,
    then its two consumers in RDEPEND-string order, then the root
    `diamond` last. Before this portuale emitted BFS-discovery order
    (root first)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/diamond"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N     ] dev-libs/common-1.0 ",
        "[ebuild  N     ] dev-libs/shared-a-1.0 ",
        "[ebuild  N     ] dev-libs/shared-b-1.0 ",
        "[ebuild  N     ] dev-libs/diamond-1.0 ",
    ]


def test_json_entries_are_merge_ordered_with_an_explicit_index(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--json: the entries array is emitted in the same dependency-first
    merge order as the plain-text list, and every entry carries an
    explicit "merge_order" integer (its 0-based position in that order) --
    so a consumer that re-sorts or filters the array keeps the schedule.
    Mirrors real portage's `mylist` being the single merge schedule."""
    import json

    args = ["--pretend", "--json", "dev-libs/diamond"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == python.stdout
    entries = json.loads(rust.stdout)["entries"]
    assert [e["package"] for e in entries] == ["common", "shared-a", "shared-b", "diamond"]
    assert [e["merge_order"] for e in entries] == [0, 1, 2, 3]


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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/anyof-1.0 ',
    ]


def test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `dep_zapdeps` re-choosing a `||` alternative once
    backtracking's `runtime_pkg_mask` hides the preferred one. dev-libs/
    orbtblocked's RDEPEND is
    `|| ( >=dev-libs/orbttool-2.0 dev-libs/orbtclean ) =dev-libs/orbttool-1.0`
    -- the first `||` alternative (orbttool-2.0) lands in the same
    `dev-libs/orbttool:0` slot as the hard `=orbttool-1.0` dep, an
    unsolvable slot conflict. The `'backtrack` loop masks the highest
    conflicting instance (orbttool-2.0), and on the retry the `||`
    group's first alternative is no longer satisfiable, so
    `dev-libs/orbtclean` (the second alternative) is chosen -- no
    conflict. `orbttool-1.0` merges ahead of `orbtclean` because a `||`
    group's chosen atom is scheduled after the plain deps of the same
    parent (real `_create_graph` drains `dep_stack` before
    `_dep_disjunctive_stack`). With `--backtrack=0` the conflict is
    reported instead, byte-identical to before this slice.
    Container-verified against real portage 3.0.82.2
    (TEST/scripts/42-or-backtrack.sh)."""
    ok = _run([str(emerge_binary)], ["--pretend", "dev-libs/orbtblocked"], fixture_env)
    assert ok.returncode == 0
    assert ok.stdout.splitlines() == [
        "[ebuild  N     ] dev-libs/orbttool-1.0 ",
        "[ebuild  N     ] dev-libs/orbtclean-1.0 ",
        "[ebuild  N     ] dev-libs/orbtblocked-1.0 ",
    ]
    # Full Rust-vs-Python lockstep: the backtracking path and the
    # --backtrack=0 conflict path both match byte-for-byte.
    for extra in ([], ["--backtrack=0"], ["--tree"]):
        args = ["--pretend", *extra, "dev-libs/orbtblocked"]
        py = _run(emerge_pretend_python, args, fixture_env)
        rs = _run([str(emerge_binary)], args, fixture_env)
        assert rs.stdout == py.stdout, (extra, rs.stdout, py.stdout)
        assert rs.stderr == py.stderr, (extra, rs.stderr, py.stderr)
    nobt = _run([str(emerge_binary)], ["--pretend", "--backtrack=0", "dev-libs/orbtblocked"], fixture_env)
    assert "slot conflict" in nobt.stdout


def test_bdepend_pdepend_idepend_are_walked_same_as_depend_rdepend(
    emerge_binary, fixture_env
):
    """Prior to this slice, resolve_pretend_graph only concatenated
    DEPEND+RDEPEND before flattening -- a package whose only dependency
    was declared via BDEPEND (build-time, EAPI 7+), PDEPEND (post-merge),
    or IDEPEND (install-time, EAPI 8+, rare) would silently resolve with
    no dependencies at all. v1 makes no distinction between any of the
    five real dependency-string keys (portuale has no real merge
    ordering for the distinction to matter to), so each of these three
    single-key fixtures must still pull in dev-libs/newpkg exactly like
    dev-libs/withdeps's own DEPEND/RDEPEND-based fixture does."""
    for pkg in ("bdependpkg", "pdependpkg", "idependpkg"):
        result = _run([str(emerge_binary)], ["--pretend", f"dev-libs/{pkg}"], fixture_env)
        assert result.returncode == 0, pkg
        assert result.stdout.splitlines() == [
            "[ebuild  N     ] dev-libs/newpkg-1.0 ",
            f"[ebuild  N     ] dev-libs/{pkg}-1.0 ",
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/multislotpkg-2.0 ',
        '[ebuild  N     ] dev-libs/slotoperatorpkg-1.0 ',
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
        '[ebuild  N     ] dev-libs/subslotpkg-1.0 ',
        '[ebuild  N     ] dev-libs/subslotconsumer-1.0 ',
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
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/subslotmismatchconsumer-1.0 ',
    ]
    assert (
        result.stderr.splitlines()
        == ['!!! no visible ebuild for dependency "dev-libs/subslotpkg"']
    )


def test_dependency_avoid_update_is_slot_aware(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """Real `avoid_update` for a dependency atom returns an installed
    package only when `vardb.match(atom)` -- which honours the atom's
    slot -- has a hit. dev-libs/avoidslotpkg-1.0 is installed at SLOT=2
    (a version/slot mismatch vs the repo ebuild's SLOT=1); a
    `dev-libs/avoidslotpkg:1` dependency must NOT be short-circuited to
    "already installed" just because version 1.0 exists in *some* slot --
    slot 1 is genuinely new, so it resolves as `[ebuild NS]`."""
    d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / "avoidslotpkg-1.0"
    d.mkdir(parents=True)
    (d / "CATEGORY").write_text("dev-libs\n")
    (d / "SLOT").write_text("2\n")
    (d / "repository").write_text("testrepo\n")
    env = dict(fixture_env)
    env["ROOT"] = str(tmp_path)
    args = ["--pretend", "dev-libs/avoidslotconsumer"]
    result = _run([str(emerge_binary)], args, env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  NS    ] dev-libs/avoidslotpkg-1.0 [1.0]",
        "[ebuild  N     ] dev-libs/avoidslotconsumer-1.0 ",
    ]
    assert _run(emerge_pretend_python, args, env).stdout == result.stdout


def _slotbind_root(tmp_path):
    """A test-local ROOT: dev-libs/slotbindtarget-1.0 installed at
    SLOT="2" (sub-slot 2), plus two := consumers -- slotbindconsumer,
    bound to the stale "dev-libs/slotbindtarget:2/2=", and slotbindfresh,
    already bound to "dev-libs/slotbindtarget:2/9=" (what -2.0 provides).
    Both consumers are in @world so the slot-operator-rebuild scan's
    reachability gate (real _complete_graph's required-set re-walk) can
    see them. PORTAGE_CONFIGROOT stays at the shared fixtures so the
    slotbindtarget-2.0 (SLOT="2/9") ebuild is visible."""
    for name, files in {
        "slotbindtarget-1.0": {"CATEGORY": "dev-libs\n", "SLOT": "2\n", "repository": "testrepo\n"},
        "slotbindconsumer-1.0": {
            "CATEGORY": "dev-libs\n",
            "SLOT": "0\n",
            "repository": "testrepo\n",
            "RDEPEND": "dev-libs/slotbindtarget:2/2=\n",
        },
        "slotbindfresh-1.0": {
            "CATEGORY": "dev-libs\n",
            "SLOT": "0\n",
            "repository": "testrepo\n",
            "RDEPEND": "dev-libs/slotbindtarget:2/9=\n",
        },
    }.items():
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / name
        d.mkdir(parents=True)
        for fn, content in files.items():
            (d / fn).write_text(content)
    world = tmp_path / "var" / "lib" / "portage" / "world"
    world.parent.mkdir(parents=True)
    world.write_text("dev-libs/slotbindconsumer\ndev-libs/slotbindfresh\n")
    return tmp_path


def test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """Real depgraph's _slot_operator_trigger_reinstalls: dev-libs/
    slotbindconsumer's vdb RDEPEND is "dev-libs/slotbindtarget:2/2="
    (bound when the target was at SLOT="2"). Emerging the target picks
    slotbindtarget-2.0 (SLOT="2/9"), so that built ABI link is stale and
    the consumer is scheduled for a reinstall -- `[ebuild rR]` (real tags
    every forced slot-op rebuild, and the triggering upgrade, with the
    red `r` -- PkgAttrDisplay.force_reinstall), no reason annotation (a
    slot-operator rebuild, not --newuse/--changed-*), in dependency-first
    merge order after the target, plus real _show_abi_rebuild_info's "The
    following packages are causing rebuilds:" block
    (--verbose-slot-rebuilds, default on). dev-libs/slotbindfresh is
    already bound to "2/9=" -> NOT rebuilt. Both consumers are in @world
    so the reachability gate (real _complete_graph's required-set
    re-walk, which auto-enables here because the target upgrade changes
    an installed package) lets the scan see them."""
    env = dict(fixture_env)
    root = str(_slotbind_root(tmp_path))
    env["ROOT"] = root
    args = ["--pretend", "dev-libs/slotbindtarget"]
    result = _run([str(emerge_binary)], args, env)
    assert result.returncode == 0
    # Real _show_abi_rebuild_info prints each side as str(Package):
    # "(cpv:slot/sub_slot::repo, ebuild scheduled for merge to '<root>')".
    assert result.stdout.splitlines() == [
        "[ebuild  r  U  ] dev-libs/slotbindtarget-2.0 [1.0]",
        "[ebuild  rR    ] dev-libs/slotbindconsumer-1.0 ",
        "",
        "The following packages are causing rebuilds:",
        "",
        f"  (dev-libs/slotbindtarget-2.0:2/9::testrepo, ebuild scheduled for merge to '{root}') causes rebuilds for:",
        f"    (dev-libs/slotbindconsumer-1.0:0/0::testrepo, ebuild scheduled for merge to '{root}')",
    ]
    # Full Rust-vs-Python lockstep, incl. --json (slot_operator_rebuild +
    # abi_rebuilds), and --verbose-slot-rebuilds=n dropping the block.
    for extra in (["--json"], ["--verbose-slot-rebuilds=n"]):
        python = _run(emerge_pretend_python, args + extra, env)
        rust = _run([str(emerge_binary)], args + extra, env)
        assert rust.stdout == python.stdout, (extra, rust.stdout, python.stdout)
    assert '"slot_operator_rebuild":true' in _run(
        [str(emerge_binary)], args + ["--json"], env
    ).stdout
    assert '"abi_rebuilds":[{"provider":"dev-libs/slotbindtarget-2.0","consumer":"dev-libs/slotbindconsumer-1.0"}]' in _run(
        [str(emerge_binary)], args + ["--json"], env
    ).stdout
    no_block = _run([str(emerge_binary)], args + ["--verbose-slot-rebuilds=n"], env)
    assert "causing rebuilds" not in no_block.stdout


def test_ignore_built_slot_operator_deps_suppresses_the_rebuild(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """Real --ignore-built-slot-operator-deps (main.py:470, y_or_n): real
    portage strips the built := operator parts out of every installed
    package's recorded *DEPEND, so _slot_operator_trigger_reinstalls
    finds nothing. Same net effect here -- the whole scan is skipped.
    Same fixture as
    test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer, but
    with --ignore-built-slot-operator-deps=y: slotbindconsumer is NOT
    reinstalled, no "causing rebuilds:" block, "abi_rebuilds":[]."""
    env = dict(fixture_env)
    env["ROOT"] = str(_slotbind_root(tmp_path))
    args = ["--pretend", "--ignore-built-slot-operator-deps=y", "dev-libs/slotbindtarget"]
    result = _run([str(emerge_binary)], args, env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild     U  ] dev-libs/slotbindtarget-2.0 [1.0]",
    ]
    # Full Rust-vs-Python lockstep, incl. --json and the bare form.
    for these in (args, args + ["--json"],
                  ["--pretend", "--ignore-built-slot-operator-deps", "dev-libs/slotbindtarget"]):
        python = _run(emerge_pretend_python, these, env)
        rust = _run([str(emerge_binary)], these, env)
        assert rust.stdout == python.stdout, (these, rust.stdout, python.stdout)
    assert '"abi_rebuilds":[]' in _run([str(emerge_binary)], args + ["--json"], env).stdout


def _slotcascade_root(tmp_path):
    """A test-local ROOT for the multi-level slot-operator cascade: a
    three-package chain casctail -> cascmid -> casctarget, all installed
    at sub-slot 0/1. The tree ebuilds: casctarget bumps 0/1 -> 0/2
    (the -2.0 upgrade), cascmid's own tree ebuild is already at 0/2
    while its vdb still records 0/1, casctail's tree ebuild stays 0/1.

    Emerging casctarget picks -2.0 (0/2) -> cascmid's built
    "casctarget:0/1=" is stale -> cascmid rebuilt; the rebuild lands at
    cascmid's *tree* SLOT 0/2, which makes casctail's built
    "cascmid:0/1=" stale in turn -> casctail rebuilt (the cascade). Only
    casctail is in @world; cascmid and casctarget are reachable from it
    over the installed dep graph, so the reachability gate admits the
    whole chain."""
    for name, files in {
        "casctarget-1.0": {"CATEGORY": "dev-libs\n", "SLOT": "0/1\n", "repository": "testrepo\n"},
        "cascmid-1.0": {
            "CATEGORY": "dev-libs\n",
            "SLOT": "0/1\n",
            "repository": "testrepo\n",
            "RDEPEND": "dev-libs/casctarget:0/1=\n",
        },
        "casctail-1.0": {
            "CATEGORY": "dev-libs\n",
            "SLOT": "0/1\n",
            "repository": "testrepo\n",
            "RDEPEND": "dev-libs/cascmid:0/1=\n",
        },
    }.items():
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / name
        d.mkdir(parents=True)
        for fn, content in files.items():
            (d / fn).write_text(content)
    world = tmp_path / "var" / "lib" / "portage" / "world"
    world.parent.mkdir(parents=True)
    world.write_text("dev-libs/casctail\n")
    return tmp_path


def test_slot_operator_rebuild_cascades_through_a_multi_level_chain(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """Real _backtrack_depgraph's slot-operator re-drive to a fixpoint: a
    scheduled rebuild lands at its tree ebuild's SLOT (not the vdb's), so
    when cascmid's tree ebuild has moved 0/1 -> 0/2 since it was
    installed, rebuilding it is itself a sub-slot shift that breaks
    casctail's built "cascmid:0/1=" -- a second forced rebuild. Every
    level is tagged with the red `r`, and every provider->consumer edge
    shows in the "causing rebuilds:" block."""
    env = dict(fixture_env)
    root = str(_slotcascade_root(tmp_path))
    env["ROOT"] = root
    args = ["--pretend", "dev-libs/casctarget"]
    result = _run([str(emerge_binary)], args, env)
    assert result.returncode == 0, result.stderr

    def pkgstr(cpv, slot):
        return f"({cpv}:{slot}::testrepo, ebuild scheduled for merge to '{root}')"

    assert result.stdout.splitlines() == [
        "[ebuild  r  U  ] dev-libs/casctarget-2.0 [1.0]",
        # cascmid's tree ebuild moved 0/1 -> 0/2, so the rebuild lands at
        # a different sub-slot than the installed instance -> real shows
        # the `[1.0]` bracket (output.py::_get_installed_best 723-732).
        "[ebuild  rR    ] dev-libs/cascmid-1.0 [1.0]",
        # casctail's tree SLOT is unchanged (0/1), so no bracket.
        "[ebuild  rR    ] dev-libs/casctail-1.0 ",
        "",
        "The following packages are causing rebuilds:",
        "",
        f"  {pkgstr('dev-libs/cascmid-1.0', '0/2')} causes rebuilds for:",
        f"    {pkgstr('dev-libs/casctail-1.0', '0/1')}",
        f"  {pkgstr('dev-libs/casctarget-2.0', '0/2')} causes rebuilds for:",
        f"    {pkgstr('dev-libs/cascmid-1.0', '0/2')}",
    ]
    # Full Rust-vs-Python lockstep, bare + --json + --tree.
    for extra in ([], ["--json"], ["--tree"]):
        python = _run(emerge_pretend_python, args + extra, env)
        rust = _run([str(emerge_binary)], args + extra, env)
        assert rust.stdout == python.stdout, (extra, rust.stdout, python.stdout)


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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/multislotpkg-2.0 ',
        '[ebuild  N     ] dev-libs/usedeppkg-1.0 ',
    ]


def test_autounmask_use_resolves_a_dependency_use_dep_mismatch(
    emerge_binary, fixture_env
):
    """dev-libs/usedeprejectedpkg's own RDEPEND is
    "dev-libs/useflagpkg[-foo]" -- useflagpkg's own "foo" is enabled
    globally, so "-foo" isn't satisfied by default. With --autounmask-use
    on by default, the graph RESOLVES: both packages print as New on
    stdout, and the `The following USE changes are necessary to proceed:`
    block on stderr carries the real two-line dep chain (`#required by
    <parent cpv>::<repo>` then `#required by <parent atom> (argument)`).
    (--autounmask-use=n keeps the old "no visible ebuild" behavior -- see
    test_autounmask_use_dependency_suggestion_is_suppressed_by_autounmask_use_n.)"""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/usedeprejectedpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/useflagpkg-1.0  USE="-foo -missingflag"',
                                             '[ebuild  N     ] dev-libs/usedeprejectedpkg-1.0 ',
                                         ]
    assert result.stderr == (
        "\nThe following USE changes are necessary to proceed:\n"
        ' (see "package.use" in the portage(5) man page for more details)\n'
        "# required by dev-libs/usedeprejectedpkg-1.0::testrepo\n"
        "# required by dev-libs/usedeprejectedpkg (argument)\n"
        ">=dev-libs/useflagpkg-1.0 -foo\n"
    )


def test_autounmask_backward_cascade_re_resolves_an_already_resolved_slot(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/aucasctop RDEPENDs aucascmid (resolved cascade-off first,
    the bare atom) then aucasclate, whose RDEPEND is
    dev-libs/aucascmid[cascade]. The already-resolved-slot re-check sees
    `[cascade]` unsatisfied and folds `cascade -> on` into the loop's
    autounmask_use_config (real _needed_use_config_changes).

    Default (real --autounmask-backtrack off, depgraph.py:11736): the
    graph is NOT re-driven -- aucascmid's own USE line is re-rendered to
    `USE="cascade"` (real _pkg_use_enabled), but its cascade?-gated
    aucascleaf does NOT appear. The change is still reported in the
    standard "USE changes are necessary" block.

    --autounmask-backtrack=y: the loop re-runs the whole walk with the
    flip applied, so aucascleaf now appears too. Full Rust==Python."""
    base = ["--pretend", "dev-libs/aucasctop"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    py = _run(emerge_pretend_python, base, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/aucascmid-1.0  USE="cascade"',
        "[ebuild  N     ] dev-libs/aucasclate-1.0 ",
        "[ebuild  N     ] dev-libs/aucasctop-1.0 ",
    ]
    assert "aucascleaf" not in rust.stdout
    assert rust.stderr == (
        "\nThe following USE changes are necessary to proceed:\n"
        ' (see "package.use" in the portage(5) man page for more details)\n'
        "# required by dev-libs/aucasclate-1.0::testrepo\n"
        ">=dev-libs/aucascmid-1.0 cascade\n"
    )

    # --autounmask-backtrack=y: the whole graph is re-driven, aucascleaf appears
    ab = ["--pretend", "--autounmask-backtrack=y", "dev-libs/aucasctop"]
    rust_ab = _run([str(emerge_binary)], ab, fixture_env)
    py_ab = _run(emerge_pretend_python, ab, fixture_env)
    assert rust_ab.stdout == py_ab.stdout and rust_ab.stderr == py_ab.stderr
    assert rust_ab.stdout.splitlines() == [
        "[ebuild  N     ] dev-libs/aucascleaf-1.0 ",
        '[ebuild  N     ] dev-libs/aucascmid-1.0  USE="cascade"',
        "[ebuild  N     ] dev-libs/aucasclate-1.0 ",
        "[ebuild  N     ] dev-libs/aucasctop-1.0 ",
    ]

    # --json (default) carries the same change, without aucascleaf
    j = _run([str(emerge_binary)], base + ["--json"], fixture_env)
    payload = json.loads(j.stdout)
    assert payload["autounmask_use_changes"] == [
        {
            "atom": ">=dev-libs/aucascmid-1.0",
            "token": "cascade",
            "dep_chain": ["required by dev-libs/aucasclate-1.0::testrepo"],
        }
    ]
    assert {e["package"] for e in payload["entries"]} == {
        "aucasctop",
        "aucasclate",
        "aucascmid",
    }

    # --autounmask-use=n: no flip, aucascmid[cascade] is unresolvable
    n = _run(
        [str(emerge_binary)], ["--pretend", "--autounmask-use=n", "dev-libs/aucasctop"], fixture_env
    )
    npy = _run(
        emerge_pretend_python,
        ["--pretend", "--autounmask-use=n", "dev-libs/aucasctop"],
        fixture_env,
    )
    assert n.stdout == npy.stdout and n.stderr == npy.stderr
    assert "aucascleaf" not in n.stdout
    assert 'no visible ebuild for dependency "dev-libs/aucascmid"' in n.stderr


def test_autounmask_breakage_abandons_autounmask_when_a_flag_is_wanted_both_ways(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/aubreaktop pulls dev-libs/aubreaksub plain, then
    dev-libs/aubreakwant (needs aubreaksub[brk]) and dev-libs/aubreakunwant
    (needs aubreaksub[-brk]) -- the same flag wanted both ways.

    Default (real --autounmask-backtrack off): the loop is not re-driven,
    so the contradiction is never noticed -- brk is collected for
    aubreakwant, aubreaksub's USE line re-renders to "brk", and the block
    reports `>=aubreaksub-1.0 brk` (aubreakunwant's opposite need is left
    silently unmet, a pre-existing already-resolved-slot limitation).

    --autounmask-backtrack=y: the loop re-drives, spots brk wanted both
    ways, and real _autounmask_breakage (depgraph.py:12262) drops every
    autounmask change and re-resolves once with suggestion off -- NO 'USE
    changes are necessary' block, aubreaksub at its default USE="-brk",
    aubreakwant's [brk] as the ordinary non-fatal dependency warning.
    Rust==Python byte-identical both ways."""
    args = ["--pretend", "dev-libs/aubreaktop"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0 and py.returncode == 0
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/aubreaksub-1.0  USE="brk"',
        "[ebuild  N     ] dev-libs/aubreakwant-1.0 ",
        "[ebuild  N     ] dev-libs/aubreakunwant-1.0 ",
        "[ebuild  N     ] dev-libs/aubreaktop-1.0 ",
    ]
    assert rust.stderr == (
        "\nThe following USE changes are necessary to proceed:\n"
        ' (see "package.use" in the portage(5) man page for more details)\n'
        "# required by dev-libs/aubreakwant-1.0::testrepo\n"
        ">=dev-libs/aubreaksub-1.0 brk\n"
    )

    # --autounmask-backtrack=y: the contradiction is detected, autounmask abandoned
    ab = ["--pretend", "--autounmask-backtrack=y", "dev-libs/aubreaktop"]
    rust_ab = _run([str(emerge_binary)], ab, fixture_env)
    py_ab = _run(emerge_pretend_python, ab, fixture_env)
    assert rust_ab.stdout == py_ab.stdout and rust_ab.stderr == py_ab.stderr
    assert rust_ab.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/aubreaksub-1.0  USE="-brk"',
        "[ebuild  N     ] dev-libs/aubreakwant-1.0 ",
        "[ebuild  N     ] dev-libs/aubreakunwant-1.0 ",
        "[ebuild  N     ] dev-libs/aubreaktop-1.0 ",
    ]
    assert "USE changes are necessary" not in rust_ab.stderr
    assert rust_ab.stderr == (
        '!!! no visible ebuild for dependency "dev-libs/aubreaksub"\n'
    )


def test_autounmask_keyword_backward_cascade_re_resolves_a_slot_to_a_masked_version(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Slice 6: dev-libs/kwbacktop RDEPENDs dev-libs/kwbackmid (bare ->
    stable 1.0 wins the slot first) AND >=dev-libs/kwbackmid-2.0 (only 2.0
    exists there, and it is ~amd64). The slot conflict folds both atoms
    into slot_constraints; on the retry `resolve_pretend`'s *_masked_only
    fallback fires because no *is_visible* candidate satisfies the folded
    >=2.0 -- so with --autounmask, kwbackmid-2.0's keyword is autounmasked
    and the slot settles on 2.0.

    Default (keyword suggestions off): the >=2.0 dep just stays
    unresolvable (a non-fatal dependency warning), same as before.
    Rust==Python byte-identical."""
    # default: no keyword suggestions -> >=2.0 unresolvable, top still merges
    d = _run([str(emerge_binary)], ["--pretend", "dev-libs/kwbacktop"], fixture_env)
    dpy = _run(emerge_pretend_python, ["--pretend", "dev-libs/kwbacktop"], fixture_env)
    assert d.returncode == 0
    assert d.stdout == dpy.stdout and d.stderr == dpy.stderr
    assert "kwbackmid-2.0" not in d.stdout
    assert 'no visible ebuild for dependency "dev-libs/kwbackmid"' in d.stderr

    # --autounmask: the slot re-resolves to 2.0 with an implicit keyword change
    a = ["--pretend", "--autounmask", "dev-libs/kwbacktop"]
    rust = _run([str(emerge_binary)], a, fixture_env)
    py = _run(emerge_pretend_python, a, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild  N    ~] dev-libs/kwbackmid-2.0 ",
        "[ebuild  N     ] dev-libs/kwbacktop-1.0 ",
    ]
    assert rust.stderr == (
        "\nThe following keyword changes are necessary to proceed:\n"
        ' (see "package.accept_keywords" in the portage(5) man page for more details)\n'
        "# required by dev-libs/kwbacktop-1.0::testrepo\n"
        "# required by dev-libs/kwbacktop (argument)\n"
        "=dev-libs/kwbackmid-2.0 ~amd64\n"
    )


def test_autounmask_levels_unmask_two_categories_at_once_on_the_same_version(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Per-level version re-scan (real `_select_pkg_highest_available_imp`
    re-runs its highest-first match for each `_autounmask_levels` step).
    dev-libs/multimaskdep-2.0 is ~amd64 keyword-masked AND @EULA
    license-masked; multimaskdep-1.0 is only keyword-masked. Level 1
    (+license) yields nothing (both still keyword-blocked); level 2
    (+~arch +license) unmasks BOTH categories and the re-scan picks the
    higher 2.0 -- recording a keyword change AND a license change for the
    same version. Before, portuale's flat `keyword_masked_only` fallback
    dropped 2.0 (it also had a license problem) and settled on 1.0.
    Rust==Python byte-identical."""
    a = ["--pretend", "--autounmask", "dev-libs/multimaskconsumer"]
    rust = _run([str(emerge_binary)], a, fixture_env)
    py = _run(emerge_pretend_python, a, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild  N    ~] dev-libs/multimaskdep-2.0 ",
        "[ebuild  N     ] dev-libs/multimaskconsumer-1.0 ",
    ]
    assert rust.stderr == (
        "\nThe following keyword changes are necessary to proceed:\n"
        ' (see "package.accept_keywords" in the portage(5) man page for more details)\n'
        "# required by dev-libs/multimaskconsumer-1.0::testrepo\n"
        "# required by dev-libs/multimaskconsumer (argument)\n"
        "=dev-libs/multimaskdep-2.0 ~amd64\n"
        "\nThe following license changes are necessary to proceed:\n"
        ' (see "package.license" in the portage(5) man page for more details)\n'
        "# required by dev-libs/multimaskconsumer-1.0::testrepo\n"
        "# required by dev-libs/multimaskconsumer (argument)\n"
        ">=dev-libs/multimaskdep-2.0 SomeEula\n"
    )

    # default (no keyword suggestions): the dep stays unresolvable
    d = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/multimaskconsumer"], fixture_env
    )
    dpy = _run(
        emerge_pretend_python,
        ["--pretend", "dev-libs/multimaskconsumer"],
        fixture_env,
    )
    assert d.stdout == dpy.stdout and d.stderr == dpy.stderr
    assert "multimaskdep" not in d.stdout
    assert 'no visible ebuild for dependency "dev-libs/multimaskdep"' in d.stderr


def test_autounmask_levels_prefer_license_over_a_higher_keyword_masked_version(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real _autounmask_levels (depgraph.py:7446) tries relaxations least-
    to most-invasive -- +license (level 1) before +~arch (level 2) -- and
    stops at the first level yielding a candidate. dev-libs/levelpkg-1.0
    is @EULA-license-masked (stable keyword); levelpkg-2.0 is ~amd64
    keyword-masked (acceptable license). So the LOWER 1.0 wins, with a
    license change, over the higher 2.0's keyword change. Rust==Python."""
    args = ["--pretend", "--autounmask", "dev-libs/levelconsumer"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild  N     ] dev-libs/levelpkg-1.0 ",
        "[ebuild  N     ] dev-libs/levelconsumer-1.0 ",
    ]
    assert "license changes are necessary" in rust.stderr
    assert "=dev-libs/levelpkg-1.0 SomeEula" in rust.stderr
    assert "keyword changes are necessary" not in rust.stderr


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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"',
                                         ]


def test_use_dep_enforcement_negated_flag_declared_but_enabled_does_not_match(
    emerge_binary, fixture_env
):
    """Same fixture as above, but "[-foo]": "foo" IS declared, but it's
    enabled, not disabled -- genuinely unsatisfied. With --autounmask-use=n
    (autounmask-use is on by default and would otherwise resolve this via
    an implicit package.use flip -- see the resolution test below) there's
    no visible candidate for this atom at all."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/useflagpkg[-foo]"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == (
        'emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".'
    )


def test_use_dep_enforcement_plain_flag_declared_but_disabled_does_not_match(
    emerge_binary, fixture_env
):
    """"missingflag" is declared in useflagpkg's own IUSE but never
    enabled anywhere in the fixture profile chain -- "[missingflag]"
    (must be enabled) is genuinely unsatisfied. --autounmask-use=n keeps
    it that way (it would otherwise resolve via an implicit flip)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/useflagpkg[missingflag]"],
        fixture_env,
    )
    assert result.returncode == 1
    assert result.stdout == ""


def test_autounmask_use_resolves_a_top_level_use_dep_mismatch(emerge_binary, fixture_env):
    """--autounmask-use is on by default (real create_depgraph_params; no
    --autounmask-keep-keywords-style asymmetry): a top-level atom whose
    plain USE-dep a `package.use` flip would satisfy RESOLVES, applying
    the implicit change. `useflagpkg[-foo]` -> `foo` (default on) flipped
    off; `-v` shows the adjusted `USE="-foo …"`, the `The following USE
    changes are necessary to proceed:` block (real _display_autounmask's
    use_changes_msg -- `>=<cpv>` form via check_if_latest, `(see
    "package.use" …)`) goes to stderr, exit 0."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "dev-libs/useflagpkg[-foo]"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines()[0].startswith(
        "[ebuild  N     ] dev-libs/useflagpkg-1.0"
    )
    assert 'USE="-foo' in result.stdout
    assert result.stderr == (
        "\nThe following USE changes are necessary to proceed:\n"
        ' (see "package.use" in the portage(5) man page for more details)\n'
        "# required by dev-libs/useflagpkg[-foo] (argument)\n"
        ">=dev-libs/useflagpkg-1.0 -foo\n"
    )


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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"',
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"',
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
    assert result.stdout == (
                            '[ebuild  N     ] dev-libs/requireduseokpkg-1.0  USE="bar foo"\n'
                            )


def test_iuse_plus_minus_defaults_apply_when_nothing_else_says_otherwise(
    emerge_binary, fixture_env
):
    """A real, previously-undetected gap, found by comparing portuale's
    own output against the real, installed system emerge on a real
    package (media-video/ffmpeg) -- REQUIRED_USE reported violated for a
    USE combination that's actually fully satisfied once IUSE's own
    "+"/"-" markers are honored. dev-libs/iusedefaultpkg's own IUSE is
    "+enableddefault -disableddefault plainflag": before this slice,
    portuale's own effective_use_flags never consulted IUSE's own
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
        '[ebuild  N     ] dev-libs/iusedefaultpkg-1.0::testrepo  USE="enableddefault plainflag -disableddefault"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
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
    portuale's own iuse_set was built purely from a package's own literal
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
        '[ebuild  N     ] dev-libs/archiuseimplicitpkg-1.0::testrepo \n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
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
    package.use.force/.mask portuale already applies last. Before this
    slice, portuale folded global use_force/use_mask into `base` early
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
        '[ebuild  N     ] dev-libs/globalprecedencepkg-1.0::testrepo  USE="(globalforceflag) (-globalmaskflag)"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
    )


def test_profile_level_minus_flag_genuinely_cancels_an_iuse_plus_default(
    emerge_binary, fixture_env
):
    """The gap portuale's own IUSE-defaults slice originally left open,
    now closed: real regenerate() runs ONE continuous incremental walk
    (pkginternal -> defaults -> conf -> pkg), so a genuine "-flag" in
    profile/make.conf really does cancel an earlier IUSE "+flag" default
    -- not just fail to add on top of it. Before this slice, this
    portuale's own effective_use_flags union-ed the already-flattened
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
        '[ebuild  N     ] dev-libs/cancelledpkg-1.0::testrepo  USE="-cancelme"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
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
    portuale's own resolve_pretend_graph returned Err(...) immediately on
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
    --autounmask-keep-keywords defaults to False (i.e. keyword changes
    ARE applied) once --autounmask itself was explicitly given, unlike
    the ambient always-on default (see the sibling test above) --
    "explicitly asking for autounmask implies wanting its keyword changes
    too." Real --autounmask then *resolves the graph* with the implicit
    `=cpv ~arch` change applied: the package prints as a normal New entry
    on stdout, real depgraph.py::_display_autounmask's `The following
    keyword changes are necessary to proceed:` block goes to stderr
    (real _writemsg + _get_dep_chain_as_comment: the `#required by ...`
    dep chain, then `=<cpv> <kw>`), and `emerge --pretend` still exits 0
    (real actions.py:563). v1 covers the "masked by KEYWORDS alone" case
    only."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "dev-libs/autounmaskkeywordpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == "[ebuild  N    ~] dev-libs/autounmaskkeywordpkg-1.0 \n"
    assert result.stderr == (
        "\nThe following keyword changes are necessary to proceed:\n"
        ' (see "package.accept_keywords" in the portage(5) man page for more details)\n'
        "# required by dev-libs/autounmaskkeywordpkg (argument)\n"
        "=dev-libs/autounmaskkeywordpkg-1.0 ~amd64\n"
    )


def test_autounmask_continue_and_backtrack_are_inert_without_autounmask_changes(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--autounmask-continue's write-and-continue half is gated on
    `"--pretend" not in myopts` (depgraph.py:5796), so under --pretend its
    only observable is the actions.py:3772 --autounmask=n warning. Its
    OTHER half -- implying --autounmask-backtrack=y -- and
    --autounmask-backtrack itself only matter once a resolution actually
    produces autounmask changes (see the aucasctop / pfgraphparent /
    aubreaktop tests); dev-libs/newpkg has none, so all three flags are
    no-ops here."""
    plain = ["--pretend", "dev-libs/newpkg"]
    base = _run([str(emerge_binary)], plain, fixture_env)

    # No autounmask changes for newpkg -> these flags change nothing.
    for extra in (
        ["--autounmask-continue"],
        ["--autounmask-backtrack", "y"],
        ["--autounmask-backtrack=n"],
    ):
        r = _run([str(emerge_binary)], plain[:1] + extra + plain[1:], fixture_env)
        assert r.stdout == base.stdout
        assert r.stderr == base.stderr
        assert r.stdout == _run(emerge_pretend_python, plain[:1] + extra + plain[1:], fixture_env).stdout

    # --autounmask-continue + --autounmask=n -> the warning on stderr,
    # merge list unchanged on stdout.
    warn = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-continue", "--autounmask=n", "dev-libs/newpkg"],
        fixture_env,
    )
    assert warn.returncode == 0
    assert warn.stdout == base.stdout
    assert "--autounmask-continue has been disabled by --autounmask=n" in warn.stderr
    assert warn.stderr == _run(
        emerge_pretend_python,
        ["--pretend", "--autounmask-continue", "--autounmask=n", "dev-libs/newpkg"],
        fixture_env,
    ).stderr


def test_autounmask_only_suppresses_the_merge_list(emerge_binary, emerge_pretend_python, fixture_env):
    """--autounmask-only (real actions.py:456): resolve the graph, then
    `mydepgraph.display_problems(); return 0` -- the `[ebuild ...]` merge
    list is NOT printed, only the autounmask changes block (+ slot
    conflicts), and the exit code stays 0. Byte-identical Rust/Python."""
    args = ["--pretend", "--autounmask", "--autounmask-only", "dev-libs/autounmaskkeywordpkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stderr == py.stderr
    # No merge list on stdout at all.
    assert rust.stdout == ""
    # The changes block still goes to stderr.
    assert rust.stderr == (
        "\nThe following keyword changes are necessary to proceed:\n"
        ' (see "package.accept_keywords" in the portage(5) man page for more details)\n'
        "# required by dev-libs/autounmaskkeywordpkg (argument)\n"
        "=dev-libs/autounmaskkeywordpkg-1.0 ~amd64\n"
    )

    # Control: without --autounmask-only, the merge list IS printed.
    full = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "dev-libs/autounmaskkeywordpkg"],
        fixture_env,
    )
    assert full.stdout == "[ebuild  N    ~] dev-libs/autounmaskkeywordpkg-1.0 \n"


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
    assert result.stdout == '[ebuild  N     ] dev-libs/autounmaskdepconsumer-1.0 \n'
    assert result.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/autounmaskkeywordpkg"'
    )


def test_autounmask_dependency_gets_a_keyword_suggestion_once_enabled(emerge_binary, fixture_env):
    """--autounmask keyword resolution for a *dependency's* own
    keyword-masked-only candidate: dev-libs/autounmaskdepconsumer RDEPENDs
    on the keyword-masked dev-libs/autounmaskkeywordpkg. The graph now
    resolves with the implicit `=cpv ~arch` change applied, so BOTH
    packages print as normal New entries on stdout, and the `The
    following keyword changes are necessary to proceed:` block on stderr
    carries the real two-line dep chain (`#required by <parent
    cpv>::<repo>` then `#required by <parent atom> (argument)` -- real
    _get_dep_chain_as_comment)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "dev-libs/autounmaskdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        (
        '[ebuild  N    ~] dev-libs/autounmaskkeywordpkg-1.0 \n'
        '[ebuild  N     ] dev-libs/autounmaskdepconsumer-1.0 \n'
        )
    )
    assert result.stderr == (
        "\nThe following keyword changes are necessary to proceed:\n"
        ' (see "package.accept_keywords" in the portage(5) man page for more details)\n'
        "# required by dev-libs/autounmaskdepconsumer-1.0::testrepo\n"
        "# required by dev-libs/autounmaskdepconsumer (argument)\n"
        "=dev-libs/autounmaskkeywordpkg-1.0 ~amd64\n"
    )


def test_autounmask_keyword_changes_appear_in_json(emerge_binary, fixture_env):
    """--json's own mirror: with --autounmask keyword resolution, the
    keyword-masked dependency resolves as a normal "new" entry (no more
    "keyword_suggestion" field -- that was for the unresolved
    "no_visible_candidate" case), and the implicit change is exposed as a
    top-level "autounmask_keyword_changes" array
    ({"atom", "token", "dep_chain"} -- atom carries its op prefix)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "--json", "dev-libs/autounmaskdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    dep = next(e for e in payload["entries"] if e["package"] == "autounmaskkeywordpkg")
    assert dep["outcome"] == "new"
    assert payload["autounmask_keyword_changes"] == [
        {
            "atom": "=dev-libs/autounmaskkeywordpkg-1.0",
            "token": "~amd64",
            "dep_chain": [
                "required by dev-libs/autounmaskdepconsumer-1.0::testrepo",
                "required by dev-libs/autounmaskdepconsumer (argument)",
            ],
        }
    ]


def test_autounmask_license_resolves_a_eula_masked_dependency(emerge_binary, fixture_env):
    """--autounmask-license (real `_display_autounmask`'s `license_msg`):
    dev-libs/licensemaskedpkg's LICENSE="SomeEula" is in the fixture's
    @EULA group, so the default `* -@EULA` ACCEPT_LICENSE masks it, with
    no package.license unmask. dev-libs/licensemaskedconsumer RDEPENDs on
    it. `--autounmask` (which defaults `autounmask_license` to y) resolves
    the graph with the implicit accept applied -- BOTH print as normal New
    entries -- and the `The following license changes are necessary to
    proceed:` block on stderr carries the two-line dep chain and the
    `>=<cpv> <license>` line (real `check_if_latest(pkg)` -> `>=` since
    1.0 is the only version). Off without `--autounmask`."""
    off = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/licensemaskedconsumer"], fixture_env
    )
    # A *dependency's* no-visible-candidate isn't fatal (only a top-level
    # atom's is), so the consumer still resolves; the dep just isn't there.
    assert off.returncode == 0
    assert off.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/licensemaskedpkg"'
    )

    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "dev-libs/licensemaskedconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        "[ebuild  N     ] dev-libs/licensemaskedpkg-1.0 \n"
        "[ebuild  N     ] dev-libs/licensemaskedconsumer-1.0 \n"
    )
    assert result.stderr == (
        "\nThe following license changes are necessary to proceed:\n"
        ' (see "package.license" in the portage(5) man page for more details)\n'
        "# required by dev-libs/licensemaskedconsumer-1.0::testrepo\n"
        "# required by dev-libs/licensemaskedconsumer (argument)\n"
        ">=dev-libs/licensemaskedpkg-1.0 SomeEula\n"
    )

    # --autounmask-license=y alone enables it; --autounmask-license=n over
    # --autounmask suppresses it (dep stays unresolvable).
    y = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-license=y", "dev-libs/licensemaskedpkg"],
        fixture_env,
    )
    assert y.returncode == 0
    assert ">=dev-libs/licensemaskedpkg-1.0 SomeEula" in y.stderr
    n = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "--autounmask-license=n", "dev-libs/licensemaskedpkg"],
        fixture_env,
    )
    assert n.returncode == 1
    assert "license changes are necessary" not in n.stderr

    j = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask", "--json", "dev-libs/licensemaskedconsumer"],
        fixture_env,
    )
    payload = json.loads(j.stdout)
    assert payload["autounmask_license_changes"] == [
        {
            "atom": ">=dev-libs/licensemaskedpkg-1.0",
            "token": "SomeEula",
            "dep_chain": [
                "required by dev-libs/licensemaskedconsumer-1.0::testrepo",
                "required by dev-libs/licensemaskedconsumer (argument)",
            ],
        }
    ]


def test_autounmask_keep_masks_n_unmasks_a_package_mask(emerge_binary, fixture_env):
    """--autounmask-keep-masks=n (real `_display_autounmask`'s
    `p_mask_change_msg`): dev-libs/hardmaskedpkg is package.mask'd (via
    the fixture repo/profile/user package.mask chain), everything else
    visible. Real portage KEEPS masks by default -- even `--autounmask`
    alone doesn't unmask -- so only `--autounmask-keep-masks=n` resolves
    it. The `The following mask changes are necessary to proceed:` block
    has the `#required by` dep chain + a bare `=<cpv>` line (no token --
    a mask unmask has no keyword/flag). The `[ebuild N #]` bracket marker
    reflects the still-`package.mask`'d state."""
    assert (
        _run([str(emerge_binary)], ["--pretend", "dev-libs/hardmaskedpkg"], fixture_env).returncode
        == 1
    )
    assert (
        _run(
            [str(emerge_binary)],
            ["--pretend", "--autounmask", "dev-libs/hardmaskedpkg"],
            fixture_env,
        ).returncode
        == 1
    )

    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-keep-masks=n", "dev-libs/maskmaskedconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        "[ebuild  N    #] dev-libs/hardmaskedpkg-1.0 \n"
        "[ebuild  N     ] dev-libs/maskmaskedconsumer-1.0 \n"
    )
    assert result.stderr == (
        "\nThe following mask changes are necessary to proceed:\n"
        ' (see "package.unmask" in the portage(5) man page for more details)\n'
        "# required by dev-libs/maskmaskedconsumer-1.0::testrepo\n"
        "# required by dev-libs/maskmaskedconsumer (argument)\n"
        "=dev-libs/hardmaskedpkg-1.0\n"
    )

    j = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-keep-masks=n", "--json", "dev-libs/maskmaskedconsumer"],
        fixture_env,
    )
    payload = json.loads(j.stdout)
    assert payload["autounmask_mask_changes"] == [
        {
            "atom": "=dev-libs/hardmaskedpkg-1.0",
            "token": "",
            "dep_chain": [
                "required by dev-libs/maskmaskedconsumer-1.0::testrepo",
                "required by dev-libs/maskmaskedconsumer (argument)",
            ],
        }
    ]


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
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/usedeprejectedpkg-1.0 ',
    ]
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/useflagpkg"'
    )


def test_autounmask_use_changes_appear_in_json(emerge_binary, fixture_env):
    """--json's own mirror: with --autounmask-use resolution the
    USE-masked dependency resolves as a normal "new" entry (no
    "use_suggestion" field -- that was for the unresolved case), and the
    implicit change is exposed as a top-level "autounmask_use_changes"
    array ({"atom", "token", "dep_chain"} -- atom is the `>=<cpv>` form
    real check_if_latest picks for USE, bug #536392)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/usedeprejectedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    dep = next(e for e in payload["entries"] if e["package"] == "useflagpkg")
    assert dep["outcome"] == "new"
    assert payload["autounmask_keyword_changes"] == []
    assert payload["autounmask_use_changes"] == [
        {
            "atom": ">=dev-libs/useflagpkg-1.0",
            "token": "-foo",
            "dep_chain": [
                "required by dev-libs/usedeprejectedpkg-1.0::testrepo",
                "required by dev-libs/usedeprejectedpkg (argument)",
            ],
        }
    ]


def test_autounmask_use_resolves_the_opt_conditional_dependency_via_the_child_flip(
    emerge_binary, fixture_env
):
    """dev-libs/useeqparentoffpkg's own RDEPEND on dev-libs/useeqchildpkg
    "[eqflag=]" evaluates to "[-eqflag]" (parent's eqflag off); the child
    has eqflag on. With --autounmask-use on by default, real portage
    resolves this by flipping the *child's* own eqflag off (the plain
    candidate flip -- real _needed_use_config_changes[child]), so both
    packages resolve as New and a single "USE changes" block covers the
    child change. Real portage's opt=-aware *parent* flip (flipping
    useeqparentoffpkg's own eqflag on instead) is only a fallback when
    the child flip is impossible -- not exercised by this fixture, and a
    separate future increment."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/useeqparentoffpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    dep = next(e for e in payload["entries"] if e["package"] == "useeqchildpkg")
    assert dep["outcome"] == "new"
    assert payload["autounmask_use_changes"] == [
        {
            "atom": ">=dev-libs/useeqchildpkg-1.0",
            "token": "-eqflag",
            "dep_chain": [
                "required by dev-libs/useeqparentoffpkg-1.0::testrepo",
                "required by dev-libs/useeqparentoffpkg (argument)",
            ],
        }
    ]


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
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/useeqparentoffpkg-1.0  USE="-eqflag"'
    assert (
        result.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/useeqchildpkg"'
    )


def test_autounmask_use_parent_flip_resolves_when_the_child_flag_is_masked(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real --autounmask-use PART B *resolution* (_apply_parent_use_changes
    -> _show_unsatisfied_dep(collect_use_changes=True)): dev-libs/
    parentflipeqpkg (IUSE +feat) RDEPENDs parentflipchildpkg[feat=]; the
    child's own `feat` is use.mask'd, so no package.use flip on the child
    can enable it. Real portage flips the *parent's* `feat` off instead
    (dropping the conditional constraint), re-resolves, and prints
    `>=dev-libs/parentflipeqpkg-1.0 -feat` in the "necessary to proceed"
    USE block -- exit 0. The parent's own USE line reads `-feat`; the
    freed child resolves as a normal New."""
    args = ["--pretend", "dev-libs/parentflipeqpkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/parentflipchildpkg-1.0  USE="(-feat)"',
        '[ebuild  N     ] dev-libs/parentflipeqpkg-1.0  USE="-feat"',
    ]
    assert rust.stderr == (
        "\nThe following USE changes are necessary to proceed:\n"
        ' (see "package.use" in the portage(5) man page for more details)\n'
        "# required by dev-libs/parentflipeqpkg-1.0::testrepo\n"
        "# required by dev-libs/parentflipeqpkg (argument)\n"
        ">=dev-libs/parentflipeqpkg-1.0 -feat\n"
    )

    # --autounmask-use=n: the shared gate is off -> the dep stays
    # unresolvable, no change block.
    n = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/parentflipeqpkg"],
        fixture_env,
    )
    assert n.returncode == 0
    assert n.stdout.strip() == '[ebuild  N     ] dev-libs/parentflipeqpkg-1.0  USE="feat"'
    assert (
        n.stderr.strip()
        == '!!! no visible ebuild for dependency "dev-libs/parentflipchildpkg"'
    )


def test_autounmask_use_parent_flip_re_resolves_the_whole_graph(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/pfgraphparent (IUSE +pf) RDEPENDs pfgraphchild[pf=] AND
    `pf? ( dev-libs/pfgraphextra )`; pfgraphchild's `pf` is use.mask'd, so
    the parent's own `pf` is flipped off.

    Default (real --autounmask-backtrack off): the flip is applied to the
    freed child and the parent's USE line, but the graph is NOT re-driven
    -- `pf? ( pfgraphextra )` was walked with pf on, so pfgraphextra stays
    in the list (matching real).

    --autounmask-backtrack=y (Slice 4): the flip is fed back into
    _needed_use_config_changes and the WHOLE graph re-resolves, so
    `pf? ( pfgraphextra )` re-evaluates with pf OFF and pfgraphextra is
    dropped. Rust==Python byte-identical."""
    args = ["--pretend", "dev-libs/pfgraphparent"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0 and py.returncode == 0
    assert rust.stdout == py.stdout and rust.stderr == py.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/pfgraphchild-1.0  USE="(-pf)"',
        "[ebuild  N     ] dev-libs/pfgraphextra-1.0 ",
        '[ebuild  N     ] dev-libs/pfgraphparent-1.0  USE="-pf"',
    ]
    assert rust.stderr == (
        "\nThe following USE changes are necessary to proceed:\n"
        ' (see "package.use" in the portage(5) man page for more details)\n'
        "# required by dev-libs/pfgraphparent-1.0::testrepo\n"
        "# required by dev-libs/pfgraphparent (argument)\n"
        ">=dev-libs/pfgraphparent-1.0 -pf\n"
    )

    # --autounmask-backtrack=y: the whole graph re-resolves, pfgraphextra drops
    ab = ["--pretend", "--autounmask-backtrack=y", "dev-libs/pfgraphparent"]
    rust_ab = _run([str(emerge_binary)], ab, fixture_env)
    py_ab = _run(emerge_pretend_python, ab, fixture_env)
    assert rust_ab.stdout == py_ab.stdout and rust_ab.stderr == py_ab.stderr
    assert rust_ab.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/pfgraphchild-1.0  USE="(-pf)"',
        '[ebuild  N     ] dev-libs/pfgraphparent-1.0  USE="-pf"',
    ]
    assert "pfgraphextra" not in rust_ab.stdout

    # --autounmask-use=n: the shared gate is off -> pf stays on,
    # pfgraphchild[pf] is unresolvable, and pf? ( pfgraphextra ) still fires.
    n = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/pfgraphparent"],
        fixture_env,
    )
    npy = _run(
        emerge_pretend_python,
        ["--pretend", "--autounmask-use=n", "dev-libs/pfgraphparent"],
        fixture_env,
    )
    assert n.stdout == npy.stdout and n.stderr == npy.stderr
    assert "pfgraphextra" in n.stdout
    assert 'no visible ebuild for dependency "dev-libs/pfgraphchild"' in n.stderr


def test_unresolvable_dependency_is_reported_not_silently_dropped(
    emerge_binary, fixture_env
):
    """The top-level package still resolves and the graph doesn't fail,
    but the unresolvable dependency is reported on stderr, not silently
    omitted."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/missingdep"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/missingdep-1.0 ',
    ]
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
    assert result.stdout.splitlines() == [
        '[binary  N     ] dev-libs/binaryonlypkg-1.0 ',
    ]
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
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/binaryusemismatchpkg-1.0  USE="foo"',
                                         ]
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
    assert result.stdout.splitlines() == [
                                             '[binary  N     ] dev-libs/binaryusemismatchpkg-1.0  USE="foo"',
                                         ]
    assert result.stderr == ""


def test_useoldpkg_atoms_prefers_the_old_binary_over_a_newer_ebuild(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--useoldpkg-atoms ATOMS (real main.py:713 -> WildcardPackageSet;
    depgraph.py:7936 + matched_oldpkg/visible_matches): for a matching
    package, prefer an existing binary package over a newer unbuilt
    ebuild. dev-libs/useoldpkgpkg: binary 1.0 in PKGDIR, ebuild 2.0 in
    the tree. Only bites under --usepkg (no binary in the pool
    otherwise)."""
    default = _run(
        [str(emerge_binary)], ["--pretend", "--usepkg", "dev-libs/useoldpkgpkg"], fixture_env
    )
    assert default.stdout.splitlines() == ["[ebuild  N     ] dev-libs/useoldpkgpkg-2.0 "]

    old = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "--useoldpkg-atoms", "dev-libs/useoldpkgpkg", "dev-libs/useoldpkgpkg"],
        fixture_env,
    )
    assert old.stdout.splitlines() == ["[binary  N     ] dev-libs/useoldpkgpkg-1.0 "]
    assert old.stdout == _run(
        emerge_pretend_python,
        ["--pretend", "--usepkg", "--useoldpkg-atoms", "dev-libs/useoldpkgpkg", "dev-libs/useoldpkgpkg"],
        fixture_env,
    ).stdout

    # Without --usepkg the binary is never in the pool, so it's inert.
    inert = _run(
        [str(emerge_binary)],
        ["--pretend", "--useoldpkg-atoms", "dev-libs/useoldpkgpkg", "dev-libs/useoldpkgpkg"],
        fixture_env,
    )
    assert inert.stdout == default.stdout


def test_quickpkg_direct_injects_source_root_packages(
    emerge_binary, emerge_pretend_python, fixture_env, fixtures_root
):
    """--quickpkg-direct / --quickpkg-direct-root (real actions.py:150-164
    + bintree._populate_additional): when --usepkg + --quickpkg-direct=y
    and the source root differs from the target ROOT, every package
    installed in the source root joins the binary-candidate pool for the
    target build, using that root's own vdb metadata.

    fixtures/quickpkgroot has one installed package,
    dev-libs/quickpkgdirectpkg-1.0 (RDEPEND=dev-libs/newpkg), that exists
    nowhere else -- no ebuild, not in the local PKGDIR."""
    src = str(fixtures_root / "quickpkgroot")

    # Not resolvable at all without --quickpkg-direct.
    plain = _run(
        [str(emerge_binary)], ["--pretend", "--usepkg", "dev-libs/quickpkgdirectpkg"], fixture_env
    )
    assert plain.returncode == 1
    assert 'there are no ebuilds to satisfy "dev-libs/quickpkgdirectpkg"' in plain.stderr

    args = [
        "--pretend",
        "--usepkg",
        "--quickpkg-direct=y",
        f"--quickpkg-direct-root={src}",
        "dev-libs/quickpkgdirectpkg",
    ]
    got = _run([str(emerge_binary)], args, fixture_env)
    assert got.returncode == 0
    assert got.stdout == _run(emerge_pretend_python, args, fixture_env).stdout
    # The quickpkg candidate resolves as a binary, and its vdb-recorded
    # RDEPEND is walked (proving the source-root metadata is used).
    assert "[binary  N     ] dev-libs/quickpkgdirectpkg-1.0 " in got.stdout
    assert "[ebuild  N     ] dev-libs/newpkg-1.0 " in got.stdout

    # Inert when the source root == the target ROOT.
    same = _run(
        [str(emerge_binary)],
        args[:3] + [f"--quickpkg-direct-root={fixture_env['ROOT']}"] + args[4:],
        fixture_env,
    )
    assert same.returncode == 1
    assert same.stdout == _run(
        emerge_pretend_python,
        args[:3] + [f"--quickpkg-direct-root={fixture_env['ROOT']}"] + args[4:],
        fixture_env,
    ).stdout


def _binscan_configroot(tmp_path, fixtures_root, binpkg_files):
    """An ad-hoc PORTAGE_CONFIGROOT whose `PKGDIR` points at a directory
    that holds `binpkg_files` (copied from fixtures) but NO
    `Packages` index -- so `--usepkg`/`--usepkgonly` must fall back to
    the real `bintree._populate_local` `$PKGDIR` directory scan."""
    cfg = tmp_path / "cfg"
    repo = tmp_path / "repo"
    pkgdir = tmp_path / "binpkgs"
    (cfg / "etc/portage").mkdir(parents=True)
    (repo / "profiles").mkdir(parents=True)
    (repo / "profiles/repo_name").write_text("main\n")
    (repo / "profiles/make.defaults").write_text('ACCEPT_KEYWORDS="amd64"\n')
    (cfg / "etc/portage/repos.conf").write_text(
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = " + str(repo) + "\n"
    )
    (cfg / "etc/portage/make.conf").write_text('PKGDIR="' + str(pkgdir) + '"\n')
    (cfg / "etc/portage/make.profile").symlink_to(repo / "profiles")
    for name in binpkg_files:
        dest = pkgdir / "dev-libs" / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(fixtures_root / "pkgdir/dev-libs" / name, dest)
    assert not (pkgdir / "Packages").exists()
    return {"PORTAGE_CONFIGROOT": str(cfg), "ROOT": str(cfg)}


def test_pkgdir_directory_scan_resolves_a_binpkg_with_no_packages_index(
    emerge_binary, emerge_pretend_python, tmp_path, fixtures_root
):
    """Real `bintree._populate_local`'s "no trusted index" branch: a
    `$PKGDIR` holding binpkg *files* but no `Packages` is scanned, each
    file's own embedded metadata read (`portuale/src/binpkg.rs` --
    real `xpak`/`gpkg`), and the synthesized candidates resolve exactly
    as if a `Packages` entry had listed them.

    Both fixture binpkgs are genuine: `packagepkg-1.0.tbz2` was built by
    portuale's own `ebuild <file> package` (real `xpak.py`);
    `gpkgreadpkg-1.0.gpkg.tar` is a hand-built real gpkg container."""
    env = _binscan_configroot(
        tmp_path,
        fixtures_root,
        ["packagepkg-1.0.tbz2", "gpkgreadpkg-1.0.gpkg.tar"],
    )
    # Each scanned binpkg carries real dependency metadata (packagepkg's
    # xpak has `RDEPEND=dev-libs/samepkg` from real `build-info`;
    # gpkgreadpkg's gpkg metadata has `dev-libs/newpkg`), and those deps
    # are actually walked -- the ad-hoc root has neither, so each shows
    # up as an unresolvable dependency (informational, exit stays 0).
    for pkg, ver, dep in [
        ("packagepkg", "1.0", "dev-libs/samepkg"),
        ("gpkgreadpkg", "1.0", "dev-libs/newpkg"),
    ]:
        args = ["--pretend", "--usepkgonly", f"dev-libs/{pkg}"]
        rust = _run([str(emerge_binary)], args, env)
        py = _run(emerge_pretend_python, args, env)
        assert rust.returncode == 0, (pkg, rust.stdout, rust.stderr)
        assert rust.stdout == py.stdout, pkg
        assert rust.stderr == py.stderr, pkg
        # A New entry shows its USE list at plain -p now; gpkgreadpkg's
        # gpkg metadata carries IUSE=grfoo (default off).
        assert rust.stdout.splitlines()[0].startswith(
            f"[binary  N     ] dev-libs/{pkg}-{ver} "
        ), (pkg, rust.stdout.splitlines()[0])
        assert f'no visible ebuild for dependency "{dep}"' in rust.stderr, pkg

    # -pv: the scanned entry's SIZE (the file's own byte size) feeds
    # `Size of downloads:` just like a `Packages` `SIZE` field would --
    # wait, no: a local binpkg is already present, nothing to download.
    v = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--usepkgonly", "dev-libs/gpkgreadpkg"],
        env,
    )
    vp = _run(
        emerge_pretend_python,
        ["--pretend", "-v", "--usepkgonly", "dev-libs/gpkgreadpkg"],
        env,
    )
    assert v.returncode == 0
    assert v.stdout == vp.stdout


def test_pkgdir_scan_is_skipped_when_a_packages_index_is_present(
    emerge_binary, fixture_env
):
    """Regression guard: the committed fixtures/pkgdir HAS both a
    `Packages` index AND the two loose binpkg fixture files. The scan
    must NOT run there -- `--usepkg` resolution stays driven by the
    `Packages` index alone (`dev-libs/binaryonlypkg` is only in the
    index; `dev-libs/packagepkg`/`gpkgreadpkg` are only loose files and
    must stay invisible)."""
    idx = _run(
        [str(emerge_binary)], ["--pretend", "--usepkgonly", "dev-libs/binaryonlypkg"], fixture_env
    )
    assert idx.returncode == 0
    assert idx.stdout.splitlines() == ['[binary  N     ] dev-libs/binaryonlypkg-1.0 ']

    loose = _run(
        [str(emerge_binary)], ["--pretend", "--usepkgonly", "dev-libs/gpkgreadpkg"], fixture_env
    )
    assert loose.returncode == 1
    assert 'no ebuilds to satisfy "dev-libs/gpkgreadpkg"' in loose.stderr


def test_getbinpkg_makes_a_remote_binhost_binary_eligible(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/remotebinpkg exists ONLY as a binary in the binhost's own
    Packages index (fixtures/binhost/Packages, reached via
    fixtures/etc/portage/binrepos.conf's `[testbinhost] sync-uri =
    file://...`), with no ebuild and no local $PKGDIR entry. Real
    main.py's `--getbinpkg`/`-g` is what adds a binrepo's packages to the
    candidate pool (real bintree._populate_remote); `--usepkg` alone
    (local $PKGDIR only) leaves it invisible, exactly like an
    ebuild-only-package "no ebuilds to satisfy" failure. `-v` renders the
    real `g` bracket column (output.py:648 `attr_display.remote_binary =
    pkg.remote`) and the binary's own SIZE feeds `Size of downloads:`
    (real bindbapi.getfetchsizes); the REPO field in the index is
    surfaced as `::gentoo`."""
    # --usepkg alone: not eligible (local $PKGDIR only).
    up = _run(
        [str(emerge_binary)], ["--pretend", "--usepkg", "dev-libs/remotebinpkg"], fixture_env
    )
    assert up.returncode == 1
    assert 'no ebuilds to satisfy "dev-libs/remotebinpkg"' in up.stderr

    # --getbinpkg: eligible, plain -p.
    g = _run(
        [str(emerge_binary)], ["--pretend", "--getbinpkg", "dev-libs/remotebinpkg"], fixture_env
    )
    gp = _run(emerge_pretend_python, ["--pretend", "--getbinpkg", "dev-libs/remotebinpkg"], fixture_env)
    assert g.returncode == 0
    assert g.stdout == gp.stdout
    assert g.stdout.splitlines() == [
                                        '[binary  N g   ] dev-libs/remotebinpkg-1.0  USE="-rbfoo"',
                                    ]

    # --getbinpkg -v: the `g` column, the ::repo decoration, the ` N KiB`
    # per-line size suffix, and the Size of downloads: counter.
    v = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--getbinpkg", "dev-libs/remotebinpkg"],
        fixture_env,
    )
    vp = _run(
        emerge_pretend_python,
        ["--pretend", "-v", "--getbinpkg", "dev-libs/remotebinpkg"],
        fixture_env,
    )
    assert v.returncode == 0
    assert v.stdout == vp.stdout
    assert v.stdout.splitlines() == [
        '[binary  N g   ] dev-libs/remotebinpkg-1.0::gentoo  USE="-rbfoo" 560 KiB',
        '',
        'Total: 1 package (1 new, 1 binary), Size of downloads: 560 KiB',
    ]

    # -G / --getbinpkgonly resolves it the same way (binary-only).
    only = _run(
        [str(emerge_binary)], ["--pretend", "-G", "dev-libs/remotebinpkg"], fixture_env
    )
    assert only.returncode == 0
    assert only.stdout.splitlines() == [
                                           '[binary  N g   ] dev-libs/remotebinpkg-1.0  USE="-rbfoo"',
                                       ]


def test_getbinpkg_slot_repo_decoration_on_a_remote_binary_line(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/remotebinslotpkg is a binhost-only binary with SLOT=2/1 --
    verbosity-3 `:slot/sub_slot` + `::repo` decoration (real _append_slot
    / _append_repository) applies to a `[binary ... g]` line just like any
    other bracket cpv."""
    v = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--getbinpkg", "dev-libs/remotebinslotpkg"],
        fixture_env,
    )
    vp = _run(
        emerge_pretend_python,
        ["--pretend", "-v", "--getbinpkg", "dev-libs/remotebinslotpkg"],
        fixture_env,
    )
    assert v.returncode == 0
    assert v.stdout == vp.stdout
    assert v.stdout.splitlines() == [
        '[binary  N g   ] dev-libs/remotebinslotpkg-1.0:2/1::gentoo  1024 KiB',
        '',
        'Total: 1 package (1 new, 1 binary), Size of downloads: 1024 KiB',
    ]


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
        '[ebuild     UD ] dev-libs/downgradepkg-1.0 [2.0]',
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
        '[ebuild     UD ] dev-libs/keywordmaskedpkg-1.0 [2.0]',
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
    portuale used to (wrongly) print a spurious downgrade line here."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/needskeywordmasked"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/needskeywordmasked-1.0 ',
    ]
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
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/needskeywordmaskeduse-1.0 ',
    ]
    assert result.stderr == ""


def test_installed_dependency_use_dep_flag_only_in_built_use_is_kept(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/needsbuiltusediverge (New) RDEPENDs on
    dev-libs/builtusedivergedep[divergedflag]. The installed 1.0 has vdb
    USE="divergedflag" but vdb IUSE="", and the *current* ebuild has
    dropped divergedflag from its IUSE -- so nothing in the tree can
    satisfy [divergedflag] (proven by the sibling top-level case).

    Real dbapi._iuse_implicit_cnstr / _iuse_implicit_built (bug 640318):
    for a built package, every flag in its recorded USE counts as a valid
    IUSE flag, independent of the profile's / ebuild's current IUSE. So
    the installed version satisfies the atom and the dependency is kept
    exactly as installed -- no spurious "no visible ebuild"."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/needsbuiltusediverge"], fixture_env
    )
    rp = _run(emerge_pretend_python, ["--pretend", "dev-libs/needsbuiltusediverge"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == rp.stdout
    assert result.stderr == rp.stderr
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/needsbuiltusediverge-1.0 ',
    ]
    assert result.stderr == ""

    # As a top-level target the same atom still needs a *visible* ebuild
    # (the avoid-update-against-vdb path is dependency-only), so it fails.
    top = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/builtusedivergedep[divergedflag]"],
        fixture_env,
    )
    assert top.returncode == 1
    assert 'no ebuilds to satisfy "dev-libs/builtusedivergedep[divergedflag]"' in top.stderr


def test_any_of_group_falls_back_to_every_alternative_when_none_satisfiable(
    emerge_binary, fixture_env
):
    """dev-libs/anyofunresolvable's own RDEPEND is
    "|| ( dev-libs/doesnotexist-anywhere dev-libs/alsodoesnotexist-anywhere )"
    -- NEITHER alternative has a visible candidate anywhere, so real
    "||" resolution (use_reduce_flat_disjunctive, portage-use-reduce)
    falls back to keeping every alternative exactly like plain
    use_reduce(flat=True) always did, matching portuale's own
    pre-existing "never silently wrong about whether a dependency
    exists" invariant -- both get reported on stderr, neither silently
    dropped just because they're inside an unresolvable || group."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/anyofunresolvable"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/anyofunresolvable-1.0 ',
    ]
    assert result.stderr.strip().splitlines() == [
        '!!! no visible ebuild for dependency "dev-libs/doesnotexist-anywhere"',
        '!!! no visible ebuild for dependency "dev-libs/alsodoesnotexist-anywhere"',
    ]


def test_env_config_vars_override_the_profile_and_make_conf(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `config.regenerate()`'s `env` USE_ORDER layer: config vars in
    the process environment override / stack on the profile chain +
    make.conf. `ACCEPT_KEYWORDS=~amd64 emerge <~amd64-only pkg>` makes it
    visible; `USE=...`/`VIDEO_CARDS=...` flip a USE-conditional dep;
    `CFLAGS=...` shows through `emerge --info`. Byte-identical Rust/Python."""

    def run(extra_env, args):
        env = {**fixture_env, **extra_env}
        rust = _run([str(emerge_binary)], args, env)
        py = _run(emerge_pretend_python, args, env)
        assert rust.stdout == py.stdout, (args, extra_env)
        assert rust.returncode == py.returncode
        return rust

    kw = ["--pretend", "--autounmask=n", "dev-libs/autounmaskkeywordpkg"]
    assert run({}, kw).returncode == 1  # keyword-masked, fatal by default
    ok = run({"ACCEPT_KEYWORDS": "~amd64"}, kw)
    assert ok.returncode == 0
    assert "[ebuild  N     ] dev-libs/autounmaskkeywordpkg-1.0" in ok.stdout

    # VIDEO_CARDS=amdgpu -> useexpandpkg's `video_cards_amdgpu? ( hiddendep )`
    # fires and `video_cards_nvidia? ( newpkg )` (profile default) does not.
    base = run({}, ["--pretend", "dev-libs/useexpandpkg"])
    assert "dev-libs/newpkg" in base.stdout and "dev-libs/hiddendep" not in base.stdout
    flipped = run({"VIDEO_CARDS": "amdgpu"}, ["--pretend", "dev-libs/useexpandpkg"])
    assert "dev-libs/hiddendep" in flipped.stdout and "dev-libs/newpkg" not in flipped.stdout

    info = run({"CFLAGS": "-O3 -pipe", "ACCEPT_KEYWORDS": "~amd64"}, ["--info"])
    assert '\nCFLAGS="-O3 -pipe"\n' in info.stdout
    assert '\nACCEPT_KEYWORDS="amd64 ~amd64"\n' in info.stdout


def test_real_use_flags_from_profile_gate_a_dependency(emerge_binary, fixture_env):
    """Pins the profile/make.conf -> real USE follow-up end to end: the
    fixture's profile chain + make.conf (see fixtures/repo/profiles
    and portage-profile's own contract test) resolves "foo" enabled and
    "missingflag" disabled, so useflagpkg's `foo? ( dev-libs/newpkg )`
    dependency must be pulled in and its
    `missingflag? ( dev-libs/hiddendep )` must not be -- proving real
    profile-derived USE, not a hardcoded empty set, reaches use_reduce."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"',
                                         ]
    assert "hiddendep" not in result.stdout


def test_use_expand_variable_drives_a_dependency(emerge_binary, fixture_env):
    """fixtures/repo/profiles/base/make.defaults declares
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
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '[ebuild  N     ] dev-libs/useexpandpkg-1.0::testrepo  VIDEO_CARDS="nvidia -amdgpu"',
        '',
        'Total: 2 packages (2 new), Size of downloads: 0 KiB',
    ]
    assert "hiddendep" not in result.stdout


def test_package_use_expand_prefix_shorthand_drives_a_dependency(emerge_binary, fixture_env):
    """fixtures/etc/portage/package.use has "dev-libs/
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
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '[ebuild  N     ] dev-libs/packageuseexpandpkg-1.0::testrepo  PYTHON_TARGETS="python3_12"',
        '',
        'Total: 2 packages (2 new), Size of downloads: 0 KiB',
    ]


def test_use_expand_unprefixed_variable_drives_a_dependency(emerge_binary, fixture_env):
    """fixtures/repo/profiles/arch/amd64/make.defaults declares
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
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '[ebuild  N     ] dev-libs/archusepkg-1.0::testrepo  USE="amd64 -riscv"',
        '',
        'Total: 2 packages (2 new), Size of downloads: 0 KiB',
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
        '[ebuild  N     ] dev-libs/wildexpanddep-1.0::testrepo ',
        '[ebuild  N     ] dev-libs/wildexpandpkg-1.0::testrepo  LINGUAS="de (-en)"',
        '',
        'Total: 2 packages (2 new), Size of downloads: 0 KiB',
    ]
    assert "wildexpandmasked" not in result.stdout
    assert "linguas_*" not in result.stdout


def test_pv_decorates_the_cpv_with_slot_and_repo(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `emerge -pv` (verbosity 3) runs `_append_slot` +
    `_append_repository` on the bracket cpv (and `convert_myoldbest` on
    each `[old-ver]`): `::repo` is always appended (quiet_repo_display
    defaults off), `:slot` only when the slot/sub-slot is other than
    `0/0` (or `new_slot`), `/sub_slot` when it differs from `slot`. Plain
    `emerge -p` shows none of it."""
    p = _run([str(emerge_binary)], ["--pretend", "dev-libs/newpkg"], fixture_env)
    assert p.stdout == "[ebuild  N     ] dev-libs/newpkg-1.0 \n"

    v = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/subslotconsumer"], fixture_env)
    assert v.stdout == _run(
        emerge_pretend_python, ["--pretend", "-v", "dev-libs/subslotconsumer"], fixture_env
    ).stdout
    assert v.stdout.splitlines()[:2] == [
        '[ebuild  N     ] dev-libs/subslotpkg-1.0:0/2::testrepo ',
        '[ebuild  N     ] dev-libs/subslotconsumer-1.0::testrepo ',
    ]

    # An Upgrade: both the new cpv and the [old-ver] are decorated
    # (upgradepkg-1.0's vdb `repository` file is testrepo).
    up = _run([str(emerge_binary)], ["--pretend", "-v", "--update", "dev-libs/upgradepkg"], fixture_env)
    assert up.stdout == _run(
        emerge_pretend_python,
        ["--pretend", "-v", "--update", "dev-libs/upgradepkg"],
        fixture_env,
    ).stdout
    assert up.stdout.splitlines()[0] == (
        "[ebuild     U  ] dev-libs/upgradepkg-2.0::testrepo [1.0::testrepo]"
    )

    # A new-slot New: the resolved `:1` and the other-slot `[1.0:0::…]`
    # list (real `myoldbest = installed_versions`, all slots).
    ns = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/newslotpkg:1"], fixture_env)
    assert ns.stdout == _run(
        emerge_pretend_python, ["--pretend", "-v", "dev-libs/newslotpkg:1"], fixture_env
    ).stdout
    assert ns.stdout.splitlines()[0] == (
        "[ebuild  NS    ] dev-libs/newslotpkg-2.0:1::testrepo [1.0:0::testrepo]"
    )

    # A same-slot Reinstall -> no [old-ver], but the main cpv still gets
    # ::testrepo at -v.
    sp = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--newuse", "dev-libs/reinstallpkg"],
        fixture_env,
    )
    assert next(
        l for l in sp.stdout.splitlines() if "reinstallpkg-1.0" in l
    ).startswith('[ebuild   R    ] dev-libs/reinstallpkg-1.0::testrepo  USE="')


def test_pv_groups_use_by_use_expand_variable(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real output.py:_display_use / map_to_use_expand: `emerge -pv` shows
    IUSE flags split into the plain USE="..." group plus one VAR="..."
    group per USE_EXPAND variable (prefix stripped), empty groups omitted.
    dev-libs/useexpandpkg (IUSE video_cards_nvidia video_cards_amdgpu,
    VIDEO_CARDS a USE_EXPAND var) shows VIDEO_CARDS="nvidia -amdgpu" and
    no USE="" at all."""
    for pkg, expected in [
        ("useexpandpkg", 'VIDEO_CARDS="nvidia -amdgpu"'),
        ("packageuseexpandpkg", 'PYTHON_TARGETS="python3_12"'),
    ]:
        args = ["--pretend", "-v", f"dev-libs/{pkg}"]
        rust = _run([str(emerge_binary)], args, fixture_env)
        python = _run(emerge_pretend_python, args, fixture_env)
        assert rust.returncode == 0
        assert rust.stdout == python.stdout, pkg
        pkg_line = next(l for l in rust.stdout.splitlines() if f"/{pkg}-1.0" in l)
        assert pkg_line == f"[ebuild  N     ] dev-libs/{pkg}-1.0::testrepo  {expected}", pkg
        assert 'USE="' not in pkg_line, pkg


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
        "[ebuild  N     ] dev-libs/hiddenexpandpkg-1.0::testrepo ",
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
    changed) / `%` (flag newly in IUSE). `emerge -pv` always runs with
    verbosity 3 -> all_flags on, so EVERY flag is shown (plain for
    unchanged) plus a `(-flag%)` token for a flag the new ebuild dropped
    from IUSE. dev-libs/upgradeusepkg was installed at 1.0 with
    IUSE="+keep change drop" / USE="keep change"; its 2.0 ebuild has
    IUSE="+keep -change +added":

      - keep:   on, was on          -> keep (plain, unchanged)
      - change: was on, now off     -> -change*
      - added:  on, not in old IUSE -> added%*
      - drop:   removed from IUSE   -> (-drop%)  (was off)
    """
    args = ["--pretend", "-v", "--update", "dev-libs/upgradeusepkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild     U  ] dev-libs/upgradeusepkg-2.0::testrepo [1.0::testrepo] USE="added%* keep -change* (-drop%)"',
        "",
        "Total: 1 package (1 upgrade), Size of downloads: 0 KiB",
    ]
    # --alphabetical collapses the polarity split into one bare-name sort.
    alpha = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--alphabetical", "--update", "dev-libs/upgradeusepkg"],
        fixture_env,
    )
    assert alpha.stdout.splitlines()[0] == (
        '[ebuild     U  ] dev-libs/upgradeusepkg-2.0::testrepo [1.0::testrepo] USE="added%* -change* (-drop%) keep"'
    )

    # A New install has no installed side -> no markers, every flag plain.
    new = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/useflagpkg"], fixture_env
    )
    assert new.stdout.splitlines()[0] == (
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo '
    )


def test_use_expand_implicit_flag_is_valid_iuse_even_when_unlisted(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """fixtures/repo/profiles/base/make.defaults declares
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
        (
        '[ebuild  N     ] dev-libs/implicitiuseprov-1.0 \n'
        '[ebuild  N     ] dev-libs/implicitiusepkg-1.0 \n'
        )
    )

    bad = _run([str(emerge_binary)], ["--pretend", "dev-libs/implicitiusepkgmusl"], fixture_env)
    bad_py = _run(emerge_pretend_python, ["--pretend", "dev-libs/implicitiusepkgmusl"], fixture_env)
    assert bad.returncode == 0
    assert bad.stdout == bad_py.stdout
    assert bad.stderr == bad_py.stderr
    assert bad.stdout == '[ebuild  N     ] dev-libs/implicitiusepkgmusl-1.0 \n'
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
    "maskflag" back off even though fixtures/etc/portage/
    package.use enables it first -- proving package.use.stable.mask wins
    over package.use, but only for a genuinely stable candidate."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/stableusepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '[ebuild  N     ] dev-libs/stableusepkg-1.0::testrepo  USE="(stableforceflag) (-maskflag)"',
        '',
        'Total: 2 packages (2 new), Size of downloads: 0 KiB',
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
        '[ebuild  N    ~] dev-libs/unstableusepkg-1.0::testrepo  USE="maskflag -stableforceflag"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
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
    assert result.stdout.strip() == '[ebuild  N    #] dev-libs/maskedandunmaskedpkg-1.0'


def test_package_mask_minus_atom_removal_leaves_candidate_unaffected(
    emerge_binary, fixture_env
):
    """fixtures/etc/portage/package.mask masks dev-libs/samepkg and
    then immediately un-masks it again via "-dev-libs/samepkg" within the
    same file -- it must resolve completely normally (visible, matched),
    proving -atom removal actually took effect rather than the mask
    lingering. A bare top-level atom with no other flags reports a plain
    reinstall (real portage's own "selective" gap -- see resolve_pretend's
    own doc comment, portage-repo), not "already installed"."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/samepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild   R    ] dev-libs/samepkg-1.0'


def test_license_eula_style_group_is_masked_by_the_real_default_accept_license(
    emerge_binary, fixture_env
):
    """Neither the fixture profile chain nor make.conf sets
    ACCEPT_LICENSE at all -- real portage's own "* -@EULA" default
    applies, and fixtures/repo/profiles/base/license_groups
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
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/anyoflicensepkg-1.0'


def test_license_package_license_unmasks_an_otherwise_eula_masked_package(
    emerge_binary, fixture_env
):
    """fixtures/etc/portage/package.license accepts SomeEula for
    dev-libs/packagelicensepkg specifically, despite the same global
    "* -@EULA" default that masks dev-libs/eulapkg above."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/packagelicensepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/packagelicensepkg-1.0'


def test_an_overlay_own_license_groups_stacks_with_the_main_repo(
    emerge_binary, fixture_env
):
    """Real LicenseManager reads license_groups from
    LocationsManager.profile_locations -- the `profiles/` directory of
    the main repo AND every overlay (LocationsManager.py:432), NOT the
    profile-chain levels. fixtures/overlay/profiles/license_groups
    extends EULA with "CrossRepoNonfree" on top of the main repo's own
    fixtures/repo/profiles/license_groups "SomeEula" member,
    proving the two stack (main first, then overlay) rather than one
    replacing the other. dev-libs/crossrepolicensepkg's own
    LICENSE="CrossRepoNonfree" is therefore masked by the real default
    "* -@EULA"."""
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
    fixtures/etc/portage/package.use forces nonfreeflag on for
    it specifically, activating the conditional and masking it via the
    same real default that masks dev-libs/eulapkg."""
    off = _run([str(emerge_binary)], ["--pretend", "dev-libs/uselicensepkg"], fixture_env)
    assert off.returncode == 0
    assert off.stdout.strip() == '[ebuild  N     ] dev-libs/uselicensepkg-1.0  USE="-nonfreeflag"'

    forced_on = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/uselicensepkgforced"], fixture_env
    )
    assert forced_on.returncode == 1
    assert forced_on.stdout == ''
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
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/propertiespkg-1.0'


def test_package_properties_narrows_acceptance_for_one_package(emerge_binary, fixture_env):
    """fixtures/etc/portage/package.properties revokes
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
    """fixtures/etc/portage/package.accept_restrict revokes
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
    """fixtures/repo/profiles/package.mask (the main repo's own
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
    """fixtures/repo/profiles/base/package.mask (a package.mask
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
    fixtures/repo/profiles/default/package.unmask -- a
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
        result.stdout.strip() == '[ebuild  N    #] dev-libs/repomaskedthenprofileunmaskedpkg-1.0'
    )


def test_user_level_minus_atom_removes_a_repo_level_mask_entry(emerge_binary, fixture_env):
    """dev-libs/repomaskedthenuserremovedpkg is masked by the repo-level
    profiles/package.mask; fixtures/etc/portage/package.mask's
    own "-dev-libs/repomaskedthenuserremovedpkg" line removes that entry
    even though it didn't add it -- proving -atom removal now applies
    across the whole combined [repo, profile chain, user] stack (real
    MaskManager.py's stack_lists(incremental=1) semantics), not just
    within the single file that contains the "-atom" line, which is all
    portuale supported before this slice."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/repomaskedthenuserremovedpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/repomaskedthenuserremovedpkg-1.0'


def test_package_accept_keywords_wildcard_extends_visibility(emerge_binary, fixture_env):
    """dev-libs/wildcardkeywordpkg is only ~amd64 (not globally accepted),
    but fixtures/etc/portage/package.accept_keywords has a
    "*/wildcardkeywordpkg ~amd64" entry that makes it visible."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/wildcardkeywordpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N    ~] dev-libs/wildcardkeywordpkg-1.0'


def test_package_accept_keywords_double_star_accepts_no_keywords_package(
    emerge_binary, fixture_env
):
    """dev-libs/livekeywordpkg has no KEYWORDS at all (like a live/9999
    ebuild), but a "**" package.accept_keywords entry accepts it
    unconditionally."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/livekeywordpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N    *] dev-libs/livekeywordpkg-9999'


def test_package_accept_keywords_negation_revokes_a_globally_accepted_keyword(
    emerge_binary, fixture_env
):
    """dev-libs/keywordrevokedpkg is stable amd64 (globally accepted),
    but fixtures/etc/portage/package.accept_keywords has a
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
    fixtures/etc/portage/package.accept_keywords has a
    "dev-libs/starkeywordpkg *" entry, real portage's own "accept any
    stable keyword" wildcard (distinct from "**", which additionally
    accepts an empty KEYWORDS)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/starkeywordpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N    *] dev-libs/starkeywordpkg-1.0'


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
    assert result.stdout.strip() == '[ebuild  N    *] dev-libs/tildestarkeywordpkg-1.0'


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
    assert result.stdout.strip() == '[ebuild  N    ~] dev-libs/bareacceptkeywordspkg-1.0'


def test_pv_bracket_mask_marker(emerge_binary, emerge_pretend_python, fixture_env):
    """Real output.py::gen_mask_str: the [ebuild N] bracket gains a
    one-character marker for a package pulled in despite not being visible
    via the global ACCEPT_KEYWORDS alone --

      - dev-libs/bareacceptkeywordspkg (~amd64 via a bare
        package.accept_keywords entry): '~' (a testing keyword for our
        own arch, real get_keyword_mask "unstable")
      - dev-libs/tildestarkeywordpkg (~arm64 via "~*"): '*' (a different
        arch, real "missing")
      - dev-libs/maskedandunmaskedpkg (package.mask'd then unmask'd): '#'
        (real isHardMasked, ignores package.unmask, wins first)

    The marker column is real set_pkg_info's `if self.include_mask_str()`
    (verbosity > 1), and real default `emerge -p` verbosity is 2, so it
    shows at plain -p too, not only -v -- absent only under --quiet
    (verbosity 1), which portuale doesn't model. A plain stable-amd64
    package (dev-libs/newpkg) shows a bare space there regardless."""
    for pkg, marker in [
        ("bareacceptkeywordspkg", "~"),
        ("tildestarkeywordpkg", "*"),
        ("maskedandunmaskedpkg", "#"),
    ]:
        v = _run([str(emerge_binary)], ["--pretend", "-v", f"dev-libs/{pkg}"], fixture_env)
        vp = _run(emerge_pretend_python, ["--pretend", "-v", f"dev-libs/{pkg}"], fixture_env)
        assert v.returncode == 0
        assert v.stdout == vp.stdout, pkg
        assert v.stdout.splitlines()[0] == f"[ebuild  N    {marker}] dev-libs/{pkg}-1.0::testrepo ", pkg
        # Plain -p: the marker column is still there (verbosity 2).
        p = _run([str(emerge_binary)], ["--pretend", f"dev-libs/{pkg}"], fixture_env)
        pp = _run(emerge_pretend_python, ["--pretend", f"dev-libs/{pkg}"], fixture_env)
        assert p.stdout == pp.stdout, pkg
        assert p.stdout.splitlines()[0] == f"[ebuild  N    {marker}] dev-libs/{pkg}-1.0 ", pkg

    v = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/newpkg"], fixture_env)
    assert v.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '',
        'Total: 1 package (1 new), Size of downloads: 0 KiB',
    ]


def test_pv_use_flag_list_is_natural_sorted(emerge_binary, emerge_pretend_python, fixture_env):
    """Real output_helpers.py::_alnum_sort_key
    (`any_iuse.sort(key=_alnum_sort_key)` in `_create_use_string`): the
    `-pv` `USE="..."` flag list splits on digit runs and compares them as
    numbers, so `n9` sorts before `n10` (not after, as plain
    lexicographic `"n10" < "n9"` would give). dev-libs/naturalsortpkg's
    IUSE is `+n2 +n9 +n10`, all `+`-defaulted on."""
    for extra in ([], ["--alphabetical"]):
        args = ["--pretend", "-v", *extra, "dev-libs/naturalsortpkg"]
        v = _run([str(emerge_binary)], args, fixture_env)
        vp = _run(emerge_pretend_python, args, fixture_env)
        assert v.returncode == 0
        assert v.stdout == vp.stdout, extra
        assert v.stdout.splitlines()[0] == (
            '[ebuild  N     ] dev-libs/naturalsortpkg-1.0::testrepo  USE="n2 n9 n10"'
        ), extra


def test_color_y_renders_real_ansi_bracket_line(emerge_binary, emerge_pretend_python, fixture_env):
    """Increment 2 of the -pv layout + colour buildout: `emerge --color y`
    (the explicit override that wins over NO_COLOR/NOCOLOR/isatty, so the
    output is deterministic even under a captured stdout) colours the
    bracket line per real lib/portage/output.py -- the exact `\\x1b[` codes
    from the real rgb_ansi_colors/ansi_codes table:

      - the type word + `pkg.cp` via `pkgprint`: PKG_MERGE_WORLD
        (green, `\\x1b[32;01m`) for a directly-requested / world-file
        package, PKG_MERGE (darkgreen, `\\x1b[32m`) for a plain dependency,
        PKG_MERGE_SYSTEM (also darkgreen) for a `@system` member;
      - the attr-display letters: `N` green, `U` turquoise
        (`\\x1b[36;01m`), the `~` mask WARN (yellow, `\\x1b[33;01m`);
      - the `[old-ver]` column blue (`\\x1b[34;01m`).

    dev-libs/diamond is a favorite (world) but NOT `@system`; its deps
    (shared-a/-b, common) are plain PKG_MERGE."""
    R = "\x1b[39;49;00m"
    args = ["--pretend", "--color=y", "dev-libs/diamond"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stdout.splitlines() == [
        f"[\x1b[32mebuild{R}  \x1b[32;01mN{R}     ] \x1b[32mdev-libs/common-1.0{R} ",
        f"[\x1b[32mebuild{R}  \x1b[32;01mN{R}     ] \x1b[32mdev-libs/shared-a-1.0{R} ",
        f"[\x1b[32mebuild{R}  \x1b[32;01mN{R}     ] \x1b[32mdev-libs/shared-b-1.0{R} ",
        f"[\x1b[32;01mebuild{R}  \x1b[32;01mN{R}     ] \x1b[32;01mdev-libs/diamond-1.0{R} ",
    ]

    # An Upgrade: turquoise U, blue [old-ver].
    up_args = ["--pretend", "--color=y", "--update", "dev-libs/upgradepkg"]
    rup = _run([str(emerge_binary)], up_args, fixture_env)
    assert rup.stdout == _run(emerge_pretend_python, up_args, fixture_env).stdout
    assert rup.stdout == (
        f"[\x1b[32;01mebuild{R}     \x1b[36;01mU{R}  ] "
        f"\x1b[32;01mdev-libs/upgradepkg-2.0{R} \x1b[34;01m[1.0]{R}\n"
    )

    # --oneshot: the same favorite drops to PKG_MERGE (plain green
    # `\x1b[32m`) -- real `_DisplayConfig.oneshot` / `check_system_world`:
    # a --oneshot target won't be added to world, so it isn't coloured as
    # a would-be world member.
    one_args = ["--pretend", "--color=y", "--oneshot", "--update", "dev-libs/upgradepkg"]
    rone = _run([str(emerge_binary)], one_args, fixture_env)
    assert rone.stdout == _run(emerge_pretend_python, one_args, fixture_env).stdout
    assert rone.stdout == (
        f"[\x1b[32mebuild{R}     \x1b[36;01mU{R}  ] "
        f"\x1b[32mdev-libs/upgradepkg-2.0{R} \x1b[34;01m[1.0]{R}\n"
    )
    # ...and the plain-text output is byte-identical with or without it.
    assert _run([str(emerge_binary)], ["-p1", "--update", "dev-libs/upgradepkg"], fixture_env).stdout == (
        _run([str(emerge_binary)], ["--pretend", "--update", "dev-libs/upgradepkg"], fixture_env).stdout
    )

    # The -v mask column is coloured (WARN ~).
    m_args = ["--pretend", "-v", "--color=y", "dev-libs/bareacceptkeywordspkg"]
    rm = _run([str(emerge_binary)], m_args, fixture_env)
    assert rm.stdout == _run(emerge_pretend_python, m_args, fixture_env).stdout
    assert rm.stdout.splitlines()[0] == (
        f"[\x1b[32;01mebuild{R}  \x1b[32;01mN{R}    \x1b[33;01m~{R}] "
        f"\x1b[32;01mdev-libs/bareacceptkeywordspkg-1.0::testrepo{R} "
    )

    # --color=n and (default) piped stdout both stay plain.
    n_args = ["--pretend", "--color=n", "dev-libs/newpkg"]
    rn = _run([str(emerge_binary)], n_args, fixture_env)
    assert rn.stdout == "[ebuild  N     ] dev-libs/newpkg-1.0 \n"
    assert "\x1b" not in _run([str(emerge_binary)], ["--pretend", "dev-libs/newpkg"], fixture_env).stdout

    # Increment 3: the USE="..." tokens are coloured per real
    # _create_use_string -- a plain enabled flag red, a plain disabled
    # -flag blue, and only the flag core, never the */% markers or a ()
    # wrap. A New: enabled `foo` red, disabled `-missingflag` blue.
    u_args = ["--pretend", "-v", "--color=y", "dev-libs/useflagpkg"]
    ru = _run([str(emerge_binary)], u_args, fixture_env)
    assert ru.stdout == _run(emerge_pretend_python, u_args, fixture_env).stdout
    assert next(
        l for l in ru.stdout.splitlines() if "useflagpkg-1.0" in l
    ).endswith(f'USE="\x1b[31;01mfoo{R} \x1b[34;01m-missingflag{R}"')
    # An Upgrade: `added%*` -> yellow core + plain %*, `keep` red
    # (unchanged-on), `-change*` -> green core + plain *, `(-drop%)` ->
    # yellow core inside plain ( … ).
    up2_args = ["--pretend", "-v", "--color=y", "--update", "dev-libs/upgradeusepkg"]
    rup2 = _run([str(emerge_binary)], up2_args, fixture_env)
    assert rup2.stdout == _run(emerge_pretend_python, up2_args, fixture_env).stdout
    assert rup2.stdout.splitlines()[0].endswith(
        f'USE="\x1b[33;01madded{R}%* \x1b[31;01mkeep{R} \x1b[32;01m-change{R}* (\x1b[33;01m-drop{R}%)"'
    )

    # Increment 4: the counters line's `interactive` word is WARN
    # (yellow); the `-pC`/`-pc` cleanup output is coloured too.
    c_args = ["--pretend", "-v", "--color=y", "dev-libs/interactivemergepkg"]
    rc = _run([str(emerge_binary)], c_args, fixture_env)
    assert rc.stdout == _run(emerge_pretend_python, c_args, fixture_env).stdout
    assert rc.stdout.splitlines()[-1] == (
        f"Total: 1 package (1 new, 1 \x1b[33;01minteractive{R}), Size of downloads: 0 KiB"
    )

    pc_args = ["--pretend", "-C", "--color=y", "dev-libs/systempkg"]
    pc = _run([str(emerge_binary)], pc_args, fixture_env)
    pcp = _run(emerge_pretend_python, pc_args, fixture_env)
    assert pc.stdout == pcp.stdout
    assert pc.stderr == pcp.stderr
    assert pc.stdout.startswith(
        f"\x1b[32m>>> These are the packages that would be unmerged:{R}\n"
    )
    # selected version -> UNMERGE_WARN (red), the legend words coloured.
    assert f"    selected: \x1b[31;01m1.0 {R}\n" in pc.stdout
    assert f">>> \x1b[31;01m'Selected'{R} packages are slated for removal.\n" in pc.stdout
    assert f">>> \x1b[32;01m'Protected'{R} and \x1b[32;01m'omitted'{R} packages" in pc.stdout
    # the system-profile warning -> BAD / WARN, on stderr.
    assert pc.stderr.startswith(
        f"\x1b[31;01m\n\n!!! 'dev-libs/systempkg' is part of your system profile.{R}\n"
    )


def test_package_accept_keywords_profile_level_entry_extends_visibility(
    emerge_binary, fixture_env
):
    """dev-libs/profileacceptkeywordspkg is only ~amd64, made visible not
    by the user-level package.accept_keywords fixture (which has no entry
    for it) but by fixtures/repo/profiles/arch/amd64's own
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
    assert result.stdout.strip() == '[ebuild  N    ~] dev-libs/profileacceptkeywordspkg-1.0'


def test_unrelated_masked_by_keywords_package_is_still_hidden(emerge_binary, fixture_env):
    """Regression guard: the "*/wildcardkeywordpkg" package.accept_keywords
    entry is scoped to that package name only (not "dev-libs/*"), so it
    must not accidentally make dev-libs/maskedpkg visible too."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/maskedpkg"], fixture_env)
    assert result.returncode == 1


def test_package_use_wildcard_entry_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """fixtures/etc/portage/package.use has a
    "*/packageuseenablepkg pkguseflag" entry: "pkguseflag" isn't enabled by
    the profile chain or make.conf, so this proves package.use (not just
    the global USE set) reaches use_reduce."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/packageuseenablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/packageuseenablepkg-1.0  USE="pkguseflag"',
                                         ]


def test_env_use_is_the_highest_tier_and_overrides_a_package_use_flag(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `configdict["env"]` is the highest `USE_ORDER` tier -- above
    the user-level `/etc/portage/package.use`. `fixtures/etc/portage/
    package.use` enables `pkguseflag` for `dev-libs/packageuseenablepkg`
    (pulling `dev-libs/newpkg`); a process-env `USE="-pkguseflag"` now
    cancels it, so the dep is not pulled -- proving env `USE` reaches
    `effective_use_flags` at its real position (was folded into the
    weaker `conf` tier before). Rust == Python."""
    env = dict(fixture_env)
    env["USE"] = "-pkguseflag"
    args = ["--pretend", "dev-libs/packageuseenablepkg"]

    rust = _run([str(emerge_binary)], args, env)
    py = _run(emerge_pretend_python, args, env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/packageuseenablepkg-1.0  USE="-pkguseflag"',
    ]


def test_package_env_env_file_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """fixtures/etc/portage/package.env maps "dev-libs/penvpkg" to the
    env file "penv-on", whose USE="penvflag" isn't set anywhere else --
    so penvpkg's own penvflag?-gated dependency (dev-libs/newpkg) is
    pulled in only because the package.env `pkg`-layer USE reaches
    effective_use_flags, and the flag shows in the USE="..." column."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/penvpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild  N     ] dev-libs/newpkg-1.0 ",
        '[ebuild  N     ] dev-libs/penvpkg-1.0  USE="penvflag -penvother"',
    ]


def test_profile_defaults_walk_is_per_level_not_flat(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """The `defaults` USE_ORDER tier is walked one profile chain level at
    a time (real config.py::regenerate() over configdict["defaults"]):
    make.defaults then package.use, per level. fixtures/repo/profiles/
    base/package.use enables "interleaveflag" for dev-libs/interleavepkg;
    the leaf profiles/default/make.defaults (applied *after* base's
    package.use) disables it -> USE="-interleaveflag", its gated dep NOT
    pulled. A flat "all make.defaults then all package.use" model would
    leave it ON. Rust == Python."""
    base = ["--pretend", "-v", "dev-libs/interleavepkg"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    py = _run(emerge_pretend_python, base, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.splitlines()[0] == (
        '[ebuild  N     ] dev-libs/interleavepkg-1.0::testrepo  '
        'USE="-interleaveflag -other"'
    )
    assert "dev-libs/newpkg" not in rust.stdout


def test_repo_make_defaults_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """fixtures/repo/profiles/make.defaults (the main repo's top-level
    make.defaults -- real config.py's _repo_make_defaults) sets
    USE="repomakedefaultflag repo_${ARCH}". "repomakedefaultflag" is set
    nowhere else, so repomakedefaultpkg's own repomakedefaultflag?-gated
    dependency (dev-libs/newpkg) is pulled in only because this weakest
    `repo` USE_ORDER layer now reaches effective_use_flags; ${ARCH}
    expands to amd64 -> repo_amd64 also enabled. Rust == Python."""
    base = ["--pretend", "-v", "dev-libs/repomakedefaultpkg"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    py = _run(emerge_pretend_python, base, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.splitlines()[:2] == [
        "[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ",
        '[ebuild  N     ] dev-libs/repomakedefaultpkg-1.0::testrepo  '
        'USE="repo_amd64 repomakedefaultflag -other"',
    ]


def test_envd_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """`fixtures/etc/profile.env` sets `USE='envdusetestflag'` -- real
    `config.py`'s `configdict["env.d"]["USE"]` (from `/etc/env.d/*` via
    `env-update`), the LOWEST `USE_ORDER` tier. `dev-libs/envdusepkg`
    (`IUSE="envdusetestflag other"`,
    `RDEPEND="envdusetestflag? ( dev-libs/newpkg )"`) resolves with the
    flag on -- nothing higher touches it -- so `dev-libs/newpkg` is
    pulled in. Rust == Python."""
    base = ["--pretend", "-v", "dev-libs/envdusepkg"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    py = _run(emerge_pretend_python, base, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.splitlines()[:2] == [
        "[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ",
        '[ebuild  N     ] dev-libs/envdusepkg-1.0::testrepo  '
        'USE="envdusetestflag -other"',
    ]


def test_overlay_own_make_defaults_use_enables_a_flag_for_its_packages(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `_repo_make_defaults` is per-repo: `fixtures/overlay/profiles/
    make.defaults` sets `USE="omdflag"`, which reaches
    `effective_use_flags` at the head of the `repo` tier but only for a
    candidate resolved from the `overlay` repo. `dev-libs/
    overlaymakedefaultpkg` (`IUSE="omdflag other"`, overlay-only) gets
    `omdflag` on and pulls `dev-libs/newpkg`; the main repo has no such
    USE. Rust == Python."""
    base = ["--pretend", "-v", "dev-libs/overlaymakedefaultpkg"]
    rust = _run([str(emerge_binary)], base, fixture_env)
    py = _run(emerge_pretend_python, base, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.splitlines()[:2] == [
        "[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ",
        '[ebuild  N     ] dev-libs/overlaymakedefaultpkg-1.0::overlay  '
        'USE="omdflag -other"',
    ]


def test_features_test_enables_the_test_use_flag_and_pulls_test_deps(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `configdict["features"]["USE"]` (config.py ~2043): `FEATURES=test`
    appends `test` to the `features` USE_ORDER tier (between `repo` and
    `pkginternal`), so `dev-libs/featuretestpkg` (`IUSE="test other"`,
    `RDEPEND="test? ( dev-libs/newpkg )"`) resolves with `test` on and
    pulls `dev-libs/newpkg`. Without `FEATURES=test` it doesn't. Rust ==
    Python."""
    with_test = dict(fixture_env)
    with_test["FEATURES"] = "test"
    args = ["--pretend", "-v", "dev-libs/featuretestpkg"]

    rust = _run([str(emerge_binary)], args, with_test)
    py = _run(emerge_pretend_python, args, with_test)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.splitlines()[:2] == [
        "[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ",
        '[ebuild  N     ] dev-libs/featuretestpkg-1.0::testrepo  '
        'USE="test -other"',
    ]

    # No FEATURES=test -> test off, no dep.
    rust_off = _run([str(emerge_binary)], args, fixture_env)
    py_off = _run(emerge_pretend_python, args, fixture_env)
    assert rust_off.stdout == py_off.stdout
    assert rust_off.stdout.splitlines()[0] == (
        '[ebuild  N     ] dev-libs/featuretestpkg-1.0::testrepo  '
        'USE="-other -test"'
    )


def test_package_use_entry_disables_a_globally_enabled_flag_for_one_package(
    emerge_binary, fixture_env
):
    """fixtures/etc/portage/package.use has a
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
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/packageusedisablepkg-1.0  USE="-foo"',
                                         ]


def test_repo_level_package_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """fixtures/repo/profiles/package.use (the main repo's own
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/repouseenablepkg-1.0  USE="repouseflag"',
                                         ]


def test_profile_level_package_use_enables_a_flag_and_pulls_in_a_dependency(
    emerge_binary, fixture_env
):
    """fixtures/repo/profiles/default/package.use (the leaf
    profile's own package.use) has a "dev-libs/profileuseenablepkg
    profileuseflag" entry -- same proof as the repo-level case above, for
    the profile-chain source instead."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/profileuseenablepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/profileuseenablepkg-1.0  USE="profileuseflag"',
                                         ]


def test_repo_level_package_use_loses_to_the_profile_defaults_layer(
    emerge_binary, fixture_env
):
    """"Config depth" slice: repo-level package.use is real
    configdict["repo"], applied BEFORE the profile make.defaults USE
    (configdict["defaults"]). fixtures/repo/profiles/package.use
    enables "repoweakflag" for dev-libs/repouseweakpkg, but the leaf
    profile's own make.defaults carries "-repoweakflag" -- so the flag
    ends up OFF and its repoweakflag?-gated dependency is NOT pulled.
    (The old flat model applied every package.use source last/strongest,
    which would have left it ON.)"""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/repouseweakpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/repouseweakpkg-1.0::testrepo  USE="-repoweakflag"',
        '',
        'Total: 1 package (1 new), Size of downloads: 0 KiB',
    ]


def test_profile_level_package_use_loses_to_make_conf(emerge_binary, fixture_env):
    """"Config depth" slice: profile-level package.use is real
    configdict["defaults"], applied BEFORE make.conf (configdict["conf"]).
    The leaf profile's own package.use enables "profweakflag" for
    dev-libs/profileuseweakpkg, but make.conf carries "-profweakflag" --
    so the flag ends up OFF and its profweakflag?-gated dependency is NOT
    pulled."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/profileuseweakpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/profileuseweakpkg-1.0::testrepo  USE="-profweakflag"',
        '',
        'Total: 1 package (1 new), Size of downloads: 0 KiB',
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
        '[ebuild  N     ] dev-libs/pkgusemaskforcepkg-1.0::testrepo  USE="(forceflag) (-maskflag) -specflag"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
    )


def test_strong_blocker_matches_an_installed_package(emerge_binary, emerge_pretend_python, fixture_env):
    """dev-libs/blockerpkg's RDEPEND is "!!dev-libs/samepkg", and
    dev-libs/samepkg-1.0 is already installed per the fixture vdb -- a
    strong blocker match is reported (not enforced: exit code stays 0).
    Real ResolverOutput._blockers (output.py:75-123): a `[blocks B      ]`
    fixed-width bracket, the `!`-stripped (real dep_expand'd) atom, then
    `("<atom>" is hard blocking <parent cpv>)` -- `hard` for a `!!`
    blocker (real blocker.atom.blocker.overlap.forbid)."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/blockerpkg"], fixture_env)
    rp = _run(emerge_pretend_python, ["--pretend", "dev-libs/blockerpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == rp.stdout
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/blockerpkg-1.0 ',
        '[blocks B      ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerpkg-1.0)',
    ]


def test_weak_blocker_matches_another_new_package_in_the_same_graph(emerge_binary, emerge_pretend_python, fixture_env):
    """dev-libs/graphblockerparent pulls in both dev-libs/blockerpartnerpkg
    and dev-libs/weakblockerpkg (whose RDEPEND is
    "!dev-libs/blockerpartnerpkg") as New in the same run, so the weak
    blocker is matched against blockerpartnerpkg's graph-resolved version,
    not just the (empty, for this package) vdb. `soft blocking` for a `!`
    blocker; the line is printed after every `[ebuild ...]` line (real
    Display.print_blockers, called after print_messages)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/graphblockerparent"], fixture_env
    )
    rp = _run(emerge_pretend_python, ["--pretend", "dev-libs/graphblockerparent"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == rp.stdout
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/blockerpartnerpkg-1.0 ',
        '[ebuild  N     ] dev-libs/weakblockerpkg-1.0 ',
        '[ebuild  N     ] dev-libs/graphblockerparent-1.0 ',
        '[blocks B      ] dev-libs/blockerpartnerpkg ("dev-libs/blockerpartnerpkg" is soft blocking dev-libs/weakblockerpkg-1.0)',
    ]


def test_blocker_lines_print_after_every_package_line_not_inline(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/blockerorderpkg RDEPENDs "!!dev-libs/samepkg" *and*
    dev-libs/newpkg, so its blocker's owner (blockerorderpkg itself) is
    the first graph entry while a non-blocker dep follows it. Real
    Display collects blocker lines and prints them as one group after
    print_messages() -- so the `[blocks B ...]` line lands after
    dev-libs/newpkg, not interleaved right after its owner."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/blockerorderpkg"], fixture_env)
    rp = _run(emerge_pretend_python, ["--pretend", "dev-libs/blockerorderpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == rp.stdout
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/blockerorderpkg-1.0 ',
        '[blocks B      ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerorderpkg-1.0)',
    ]


def test_blocker_line_is_coloured_under_color_y(emerge_binary, emerge_pretend_python, fixture_env):
    """Real _blockers wraps `blocks`, the `B`, the resolved atom, and the
    parenthetical in colorize(PKG_BLOCKER, ...) -- style "red"
    (\\x1b[31;01m). `-v` widens the bracket by the mask column's own
    space (real empty_space_in_brackets)."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--color=y", "-v", "dev-libs/blockerpkg"], fixture_env
    )
    rp = _run(
        emerge_pretend_python, ["--pretend", "--color=y", "-v", "dev-libs/blockerpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == rp.stdout
    R = "\x1b[31;01m"
    Z = "\x1b[39;49;00m"
    assert result.stdout.splitlines()[1] == (
        f"[{R}blocks{Z} {R}B{Z}      ] {R}dev-libs/samepkg{Z}"
        f'{R} ("dev-libs/samepkg" is hard blocking dev-libs/blockerpkg-1.0){Z}'
    )


def test_unrelated_package_reports_no_blockers(emerge_binary, fixture_env):
    """Regression guard: the diamond fixture has no blockers at all, so
    resolving it must not gain a spurious [blocks] line."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/diamond"], fixture_env)
    assert result.returncode == 0
    assert "[blocks" not in result.stdout


def test_overlay_only_package_is_found(emerge_binary, fixture_env):
    """dev-libs/overlayonlypkg exists only in the fixture's overlay repo
    (see fixtures/etc/portage/repos.conf), not the main repo --
    proving the overlay is actually searched, not just present in
    repos.conf."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlayonlypkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/overlayonlypkg-1.0'


def test_best_version_wins_regardless_of_which_repo_has_it(emerge_binary, fixture_env):
    """dev-libs/overlaynewerpkg-1.0 is in the main repo, -2.0 is in the
    overlay -- the higher version wins even though it isn't in the main
    (lower-priority) repo."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlaynewerpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/overlaynewerpkg-2.0'


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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/overlaytiepkg-1.0 ',
    ]


def test_overlay_own_package_mask_hides_only_the_overlay_copy(emerge_binary, fixture_env):
    """dev-libs/overlaymaskedpkg exists in both the main repo and the
    overlay; only the overlay's own profiles/package.mask masks it, with
    no explicit "::repo" constraint on the entry -- proving real
    append_repo's own auto-scoping ("::overlay") keeps the mask from
    also hiding the identically-named main-repo package."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/overlaymaskedpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N    #] dev-libs/overlaymaskedpkg-1.0'


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
    assert result.stdout.strip() == '[ebuild  N    #] dev-libs/overlaymaskedpkg-1.0'


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
    assert result.stdout.strip() == '[ebuild  N    #] dev-libs/overlaymaskedthenunmaskedpkg-1.0'


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
    assert result.stdout.strip() == '[ebuild  N    #] dev-libs/mastermaskedthenoverlayunmaskedpkg-1.0'


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
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/independentmastermainonlypkg-1.0'


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
    assert ok.stdout.strip() == '[ebuild  N     ] dev-libs/repnamepkg-1.0'
    assert ok.stderr == ""

    by_file = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/repnamepkg::repnamefromfile"],
        fixture_env,
    )
    assert by_file.returncode == 0
    assert by_file.stdout.strip() == '[ebuild  N     ] dev-libs/repnamepkg-1.0'

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


def test_profile_parent_resolves_an_aliased_repo_name(
    emerge_binary, emerge_pretend_python, tmp_path, fixtures_root
):
    """A profile `parent` line `<alias>:some/path` where `<alias>` is a
    repo's `aliases =` (not its canonical name) resolves through the
    alias -- real `LocationsManager._expand_parent_colon` looks the token
    up via `repositories.get_location_for_name`, which is keyed on
    aliases too. The aliased-in profile level provides `USE=aliasflag`,
    which shows up at `-pv` on a package in the main repo.

    (An atom's own `cat/pkg::alias` is deliberately NOT alias-resolved --
    real `match_from_list` does a straight name comparison; the sibling
    fixture `dev-libs/repnamepkg::repnamesection` already covers that
    both sides reject it.)"""
    cfg = tmp_path / "cfg"
    main = tmp_path / "main"
    other = tmp_path / "other"
    (cfg / "etc/portage").mkdir(parents=True)
    (main / "profiles/default").mkdir(parents=True)
    (main / "metadata").mkdir()
    (main / "metadata/md5-cache/dev-libs").mkdir(parents=True)
    (other / "profiles/shared").mkdir(parents=True)
    (other / "metadata").mkdir()

    (main / "metadata/layout.conf").write_text("profile-formats = portage-2\n")
    (main / "profiles/repo_name").write_text("mainrepo\n")
    (main / "profiles/default/make.defaults").write_text(
        'ACCEPT_KEYWORDS="amd64"\nUSE=""\n'
    )
    # `otherrepo` is the canonical name; `ovl` is its alias.
    (other / "profiles/repo_name").write_text("otherrepo\n")
    (other / "metadata/layout.conf").write_text("aliases = ovl\n")
    (other / "profiles/shared/make.defaults").write_text('USE="aliasflag"\n')

    (main / "profiles/default/parent").write_text("../base\novl:shared\n")
    (main / "profiles/base").mkdir()
    (main / "profiles/base/eapi").write_text("8\n")

    (cfg / "etc/portage/repos.conf").write_text(
        "[DEFAULT]\nmain-repo = mainrepo\n\n"
        f"[mainrepo]\nlocation = {main}\n\n"
        f"[otherrepo]\nlocation = {other}\n"
    )
    (cfg / "etc/portage/make.profile").symlink_to(main / "profiles/default")

    (main / "profiles/default/eapi").write_text("8\n")
    (main / "dev-libs/aliasusepkg").mkdir(parents=True)
    (main / "dev-libs/aliasusepkg/aliasusepkg-1.0.ebuild").write_text(
        'EAPI=8\nDESCRIPTION="x"\nSLOT="0"\nKEYWORDS="amd64"\nIUSE="aliasflag"\n'
    )
    (main / "metadata/md5-cache/dev-libs/aliasusepkg-1.0").write_text(
        "DEFINED_PHASES=-\nDESCRIPTION=x\nEAPI=8\nIUSE=aliasflag\n"
        "KEYWORDS=amd64\nSLOT=0\n_md5_=0000000000000000000000000000000\n"
    )

    env = {"PORTAGE_CONFIGROOT": str(cfg), "ROOT": str(cfg)}
    args = ["--pretend", "-v", "dev-libs/aliasusepkg"]
    rust = _run([str(emerge_binary)], args, env)
    py = _run(emerge_pretend_python, args, env)
    assert rust.returncode == 0, (rust.stdout, rust.stderr)
    assert rust.stdout == py.stdout
    assert rust.stderr == py.stderr
    # `USE="aliasflag"` -> the aliased `ovl:shared` profile level was
    # actually reached.
    assert 'USE="aliasflag"' in rust.stdout.splitlines()[0], rust.stdout


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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/overlayuseenablepkg-1.0  USE="overlayuseflag"',
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/overlayuseforcepkg-1.0  USE="(overlayforceflag)"',
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
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/overlayusemaskpkg-1.0  USE="(-overlaymaskflag)"',
                                         ]


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
    # binaryonlypkg has no ebuild, so real `not cp_exists` -> the
    # --misspell-suggestions block is appended after the abort line.
    assert result.stderr.splitlines()[0] == (
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
    assert non_matching.stdout == ''

    matching = _run(
        [str(emerge_binary)],
        ["--pretend", "--usepkg", "--usepkg-include", "dev-libs/binaryonlypkg", "dev-libs/binaryonlypkg"],
        fixture_env,
    )
    assert matching.returncode == 0
    assert matching.stdout.splitlines() == [
        '[binary  N     ] dev-libs/binaryonlypkg-1.0 ',
    ]


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
        '[ebuild   R    ] dev-libs/newrepopkg-1.0 ',
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
        '[ebuild   R    ] dev-libs/samepkg-1.0 ',
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/dualdep-1.0 ',
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
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/buildpkgonlysatisfied-1.0'
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
        '[binary   R    ] dev-libs/rebuiltbinarypkg-1.0 ',
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
        too_high.stdout.strip() == 'dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do'
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
        '[binary   R    ] dev-libs/rebuiltbinarypkg-1.0 ',
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
        '[binary   R    ] dev-libs/rebuiltbinarypkg-1.0 ',
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
        == 'dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do'
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
                                             '[ebuild  N     ] dev-libs/useeqchildpkg-1.0  USE="eqflag"',
                                             '[ebuild  N     ] dev-libs/useeqparentonpkg-1.0  USE="eqflag"',
                                         ]
    assert result.stderr == ""


def test_use_dep_equal_parent_mismatches_when_parent_flag_is_disabled(emerge_binary, fixture_env):
    """Same mechanism as the sibling test above, the other half of
    "opt="'s own truth table: dev-libs/useeqparentoffpkg's own
    IUSE="eqflag" (no "+") defaults it OFF, so the identical
    "[eqflag=]" use-dep now evaluates to "[-eqflag]" (must be disabled)
    -- which does NOT match dev-libs/useeqchildpkg's own default-on
    eqflag. With --autounmask-use=n (autounmask-use is on by default and
    would resolve this via a child-flip -- see
    test_autounmask_use_resolves_the_opt_conditional_dependency_via_the_child_flip)
    the dependency is reported unresolvable, not silently dropped or
    incorrectly satisfied."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--autounmask-use=n", "dev-libs/useeqparentoffpkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.strip() == '[ebuild  N     ] dev-libs/useeqparentoffpkg-1.0  USE="-eqflag"'
    assert result.stderr.strip() == (
        '!!! no visible ebuild for dependency "dev-libs/useeqchildpkg"'
    )


def test_tree_indents_a_diamond_dependency_and_shows_it_once(emerge_binary, fixture_env):
    """--tree/-t: portuale-specific simplified indentation (real
    output_helpers.py's own _tree_display needs a genuine merge
    scheduler portuale doesn't have -- see pretend.rs's own print_tree
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
        '[ebuild  N     ] dev-libs/diamond-1.0 ',
        '[ebuild  N     ]   dev-libs/shared-a-1.0 ',
        '[ebuild  N     ]     dev-libs/common-1.0 ',
        '[ebuild  N     ]   dev-libs/shared-b-1.0 ',
    ]


def test_tree_unordered_display_preserves_discovery_order(emerge_binary, fixture_env):
    """--unordered-display (only meaningful together with --tree): real
    portage's own man page wording -- does NOT sort the tree in merging
    order. dev-libs/treeorderpkg's own RDEPEND deliberately lists its
    two children in reverse-alphabetical order
    ("dev-libs/ztreechild dev-libs/atreechild"). The default (--tree
    alone) sorts children alphabetically, portuale's own deterministic
    stand-in for real portage's genuine merge-order sort (no scheduler
    exists to be more faithful than that) -- --unordered-display instead
    preserves the RDEPEND string's own literal order, using
    already-existing BFS discovery-order data, not sorted at all."""
    ordered = _run(
        [str(emerge_binary)], ["--pretend", "--tree", "dev-libs/treeorderpkg"], fixture_env
    )
    assert ordered.returncode == 0
    assert ordered.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/treeorderpkg-1.0 ',
        '[ebuild  N     ]   dev-libs/atreechild-1.0 ',
        '[ebuild  N     ]   dev-libs/ztreechild-1.0 ',
    ]

    unordered = _run(
        [str(emerge_binary)],
        ["--pretend", "--tree", "--unordered-display", "dev-libs/treeorderpkg"],
        fixture_env,
    )
    assert unordered.returncode == 0
    assert unordered.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/treeorderpkg-1.0 ',
        '[ebuild  N     ]   dev-libs/ztreechild-1.0 ',
        '[ebuild  N     ]   dev-libs/atreechild-1.0 ',
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
        '[ebuild  N     ]   dev-libs/shared-a-1.0 ',
        '[ebuild  N     ]     dev-libs/common-1.0 ',
        '[ebuild  N     ]   dev-libs/shared-b-1.0 ',
    ]


def test_columns_right_aligns_the_version_into_a_fixed_column(emerge_binary, fixture_env):
    """--columns: real _set_root_columns's own layout (portuale's own
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
    assert result.stdout == '[ebuild  N     ] dev-libs/newpkg [1.0]  \n'


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
    assert result.stdout == '[ebuild     U  ] dev-libs/upgradepkg [2.0] [1.0]\n'


def test_columns_and_tree_together_is_a_usage_error(emerge_binary, fixture_env):
    """Real actions.py: "can't specify both of --tree and --columns" --
    portuale's own CLI-usage-error convention (exit 2, stderr) rather
    than real portage's own literal exit 1/stdout, matching every other
    CLI-usage error portuale already reports."""
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
    """An unparsable COLUMNWIDTH warns (a fixed, portuale-authored message,
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
        '[ebuild  N     ] dev-libs/newpkg                                       [1.0]                        \n'
    )


def test_solvable_slot_conflict_is_reconciled_by_backtracking(emerge_binary, fixture_env):
    """dev-libs/slotconflictparent pulls in slotconflictnewconsumer (bare
    RDEPEND on slotconflicttarget, resolves the best version, 2.0, first)
    and slotconflictoldconsumer (RDEPEND "<dev-libs/slotconflicttarget-2.0",
    which 2.0 itself does NOT satisfy). The first pass hits a slot conflict
    on slotconflicttarget:0, but 1.0 satisfies *both* atoms, so the
    backtracking retry (real _emerge/resolver/backtracking.py driven by
    _process_slot_conflicts) re-resolves the whole graph with both
    constraints enforced together: slotconflicttarget settles on 1.0 and
    no [slot conflict] line is printed."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/slotconflictparent"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/slotconflicttarget-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictnewconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictoldconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictparent-1.0 ',
    ]


def test_backtrack_zero_disables_slot_conflict_reconciliation(emerge_binary, fixture_env):
    """`--backtrack=0` (real: disable backtracking) turns the retry loop
    off, so the otherwise-solvable dev-libs/slotconflictparent conflict is
    reported instead of reconciled -- the pre-backtracking behavior, on
    demand. `--backtrack=1` is enough to reconcile a one-step conflict."""
    reconciled = [
        '[ebuild  N     ] dev-libs/slotconflicttarget-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictnewconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictoldconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictparent-1.0 ',
    ]
    r0 = _run(
        [str(emerge_binary)],
        ["--pretend", "--backtrack=0", "dev-libs/slotconflictparent"],
        fixture_env,
    )
    assert r0.returncode == 0
    assert r0.stdout.splitlines()[:4] == [
        '[ebuild  N     ] dev-libs/slotconflicttarget-2.0 ',
        '[ebuild  N     ] dev-libs/slotconflictnewconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictoldconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictparent-1.0 ',
    ]
    _assert_slot_collision_block(
        r0.stdout,
        "dev-libs/slotconflicttarget:0",
        [
            (
                "dev-libs/slotconflicttarget-1.0:0/0::testrepo",
                [("dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo", "<dev-libs/slotconflicttarget-2.0")],
            ),
        ],
        backtrack_hint=False,
    )

    r1 = _run(
        [str(emerge_binary)],
        ["--pretend", "--backtrack", "1", "dev-libs/slotconflictparent"],
        fixture_env,
    )
    assert r1.returncode == 0
    assert r1.stdout.splitlines() == reconciled


def test_unsolvable_slot_conflict_resolved_by_masking_a_puller_version(
    emerge_binary, fixture_env
):
    """dev-libs/btparent -> btconsumer (resolves -2.0, RDEPEND
    >=bttarget-2.0) + btpin (RDEPEND <bttarget-2.0). No bttarget version
    satisfies both, so slice 1's solvability check fails. Slice 3's real
    runtime_pkg_mask trial hides bttarget-2.0 AND btconsumer-2.0 (which
    has a lower -1.0 whose RDEPEND is only a bare bttarget); the retry
    falls back to btconsumer-1.0 + bttarget-1.0 and every constraint is
    met, with no [slot conflict] line. `--backtrack=0` turns the trial
    off, so the conflict is reported instead."""
    r = _run([str(emerge_binary)], ["--pretend", "dev-libs/btparent"], fixture_env)
    assert r.returncode == 0
    assert r.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/bttarget-1.0 ',
        '[ebuild  N     ] dev-libs/btconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/btpin-1.0 ',
        '[ebuild  N     ] dev-libs/btparent-1.0 ',
    ]

    r0 = _run(
        [str(emerge_binary)],
        ["--pretend", "--backtrack=0", "dev-libs/btparent"],
        fixture_env,
    )
    assert r0.returncode == 0
    assert r0.stdout.splitlines()[:4] == [
        '[ebuild  N     ] dev-libs/bttarget-2.0 ',
        '[ebuild  N     ] dev-libs/btconsumer-2.0 ',
        '[ebuild  N     ] dev-libs/btpin-1.0 ',
        '[ebuild  N     ] dev-libs/btparent-1.0 ',
    ]
    _assert_slot_collision_block(
        r0.stdout,
        "dev-libs/bttarget:0",
        [
            (
                "dev-libs/bttarget-2.0:0/0::testrepo",
                [("dev-libs/btconsumer-2.0:0/0::testrepo", ">=dev-libs/bttarget-2.0")],
            ),
            (
                "dev-libs/bttarget-1.0:0/0::testrepo",
                [("dev-libs/btpin-1.0:0/0::testrepo", "<dev-libs/bttarget-2.0")],
            ),
        ],
        backtrack_hint=False,
    )


def test_unsolvable_slot_conflict_survives_backtracking_and_is_reported(
    emerge_binary, fixture_env
):
    """dev-libs/slotconflictunsolvable pulls in slotconflictnewpin (RDEPEND
    ">=dev-libs/slotconflicttarget-2.0", resolves 2.0 first) and
    slotconflictoldpin (RDEPEND "<dev-libs/slotconflicttarget-2.0"). No
    single version of slotconflicttarget satisfies both, so the
    backtracking solvability pre-check fails, the runtime_pkg_mask trial
    is reverted, and the slot-collision block is reported -- purely
    informational, exit code unchanged."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/slotconflictunsolvable"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines()[:4] == [
        '[ebuild  N     ] dev-libs/slotconflicttarget-2.0 ',
        '[ebuild  N     ] dev-libs/slotconflictnewpin-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictoldpin-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictunsolvable-1.0 ',
    ]
    _assert_slot_collision_block(
        result.stdout,
        "dev-libs/slotconflicttarget:0",
        [
            (
                "dev-libs/slotconflicttarget-2.0:0/0::testrepo",
                [("dev-libs/slotconflictnewpin-1.0:0/0::testrepo", ">=dev-libs/slotconflicttarget-2.0")],
            ),
            (
                "dev-libs/slotconflicttarget-1.0:0/0::testrepo",
                [("dev-libs/slotconflictoldpin-1.0:0/0::testrepo", "<dev-libs/slotconflicttarget-2.0")],
            ),
        ],
    )


def test_slot_conflict_groups_same_reason_parents_and_offers_verbose_conflicts(
    emerge_binary, fixture_env
):
    """dev-libs/slotconfgroup pulls slotconfgroupnew (>=slotconflicttarget-2.0,
    reached first -> slot 0 resolves to 2.0) plus slotconfgroupa/b/c (each
    <slotconflicttarget-2.0). The 1.0 instance has three parents sharing
    one collision reason ("version", "le"): real
    _prepare_conflict_msg_and_check_for_specificity keeps one
    representative and appends "(and 2 more with the same problem)", then a
    single "NOTE: Use the '--verbose-conflicts' option ..." footer.
    --verbose-conflicts shows all three and drops both trailers."""
    collapsed = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/slotconfgroup"], fixture_env
    )
    assert collapsed.returncode == 0
    out = collapsed.stdout
    # the >=2.0 parent of the 2.0 instance: carets under ">=" and "2.0"
    assert (
        '  (dev-libs/slotconflicttarget-2.0:0/0::testrepo, ebuild scheduled for merge) USE="" pulled in by\n'
        '    >=dev-libs/slotconflicttarget-2.0 required by (dev-libs/slotconfgroupnew-1.0:0/0::testrepo, ebuild scheduled for merge) USE=""\n'
        "    ^^                            ^^^"
    ) in out
    # exactly one of a/b/c is shown, then the omission tail + NOTE
    shown = [p for p in ("a", "b", "c") if f"slotconfgroup{p}-1.0:0/0::testrepo, ebuild scheduled for merge) USE=\"\"" in out]
    assert shown == ["a"]
    assert "    (and 2 more with the same problem)\n" in out
    assert (
        "\nNOTE: Use the '--verbose-conflicts' option to display parents omitted above\n"
        in out
    )

    verbose = _run(
        [str(emerge_binary)],
        ["--pretend", "--verbose-conflicts", "dev-libs/slotconfgroup"],
        fixture_env,
    )
    assert verbose.returncode == 0
    vout = verbose.stdout
    for p in ("a", "b", "c"):
        assert (
            f"    <dev-libs/slotconflicttarget-2.0 required by (dev-libs/slotconfgroup{p}-1.0:0/0::testrepo, ebuild scheduled for merge) USE=\"\"\n"
            in vout
        )
    assert "with the same problem" not in vout
    assert "--verbose-conflicts' option to display parents omitted" not in vout


def test_different_slots_of_the_same_package_coexist_without_conflict(emerge_binary, fixture_env):
    """dev-libs/multislotparent RDEPENDs on both dev-libs/multislotpkg:0
    and dev-libs/multislotpkg:1 -- real, different slots of the same
    package are normal coexistence (like dev-lang/python:3.11 and
    :3.12), not a conflict: both must appear as independent [ebuild N]
    lines, with no [slot conflict] line at all."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/multislotparent"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/multislotpkg-1.0 ',
        '[ebuild  N     ] dev-libs/multislotpkg-2.0 ',
        '[ebuild  N     ] dev-libs/multislotparent-1.0 ',
    ]
    assert _SLOT_COLLISION_PREAMBLE not in result.stdout

    # --tree: *both* slots nest under the parent that pulled them in.
    # (Regression: a destructive required_by merge used to hand the owner
    # to only the first slot's entry, so the second fell through to the
    # flush-left safety net.)
    tree = _run(
        [str(emerge_binary)], ["--pretend", "--tree", "dev-libs/multislotparent"], fixture_env
    )
    assert tree.returncode == 0
    assert tree.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/multislotparent-1.0 ',
        '[ebuild  N     ]   dev-libs/multislotpkg-1.0 ',
        '[ebuild  N     ]   dev-libs/multislotpkg-2.0 ',
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
            "[ebuild  NS    ] dev-libs/newslotpkg-2.0 [1.0]",
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
        '[ebuild IN     ] dev-libs/interactivemergepkg-1.0 ',
    ]

    # An installed interactive package reinstalls as [ebuild Ir].
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/interactiveinstalledpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild I R    ] dev-libs/interactiveinstalledpkg-1.0 ',
    ]

    # `gtk? ( interactive )` with gtk disabled -> the conditional gates
    # the interactive token out, no I.
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/interactivecondpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/interactivecondpkg-1.0  USE="-gtk"',
                                         ]


def test_fetch_restrict_bracket_column(emerge_binary, fixture_env):
    """Real output.py:633: for a merge-bound ebuild whose evaluated
    RESTRICT contains `fetch`, attr_display.fetch_restrict; then
    fetch_restrict_satisfied if `not getfetchsizes(only_restricted=True)`
    -- every SRC_URI distfile already in DISTDIR at its Manifest size.
    __str__ renders green `f` (satisfied) / red `F` (missing), after the
    S column. fixture_env points DISTDIR at fixtures/distfiles/,
    which holds frs-1.0.tar.gz (matching frs's Manifest) but not
    frm-1.0.tar.gz."""
    ok = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/fetchrestrictsatisfiedpkg"], fixture_env
    )
    assert ok.returncode == 0
    assert ok.stdout.splitlines() == [
        '[ebuild  N f   ] dev-libs/fetchrestrictsatisfiedpkg-1.0 ',
    ]

    missing = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/fetchrestrictmissingpkg"], fixture_env
    )
    assert missing.returncode == 0
    assert missing.stdout.splitlines() == [
        '[ebuild  N F   ] dev-libs/fetchrestrictmissingpkg-1.0 ',
    ]

    # Point DISTDIR somewhere empty -> even the pre-seeded one is now F.
    empty_env = {**fixture_env, "DISTDIR": "/var/empty/no-such-distdir"}
    both_missing = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/fetchrestrictsatisfiedpkg"], empty_env
    )
    assert both_missing.stdout.splitlines() == [
        '[ebuild  N F   ] dev-libs/fetchrestrictsatisfiedpkg-1.0 ',
    ]

    # A package with no RESTRICT=fetch has no f/F column at all.
    plain = _run([str(emerge_binary)], ["--pretend", "dev-libs/newpkg"], fixture_env)
    assert plain.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
    ]


def test_pv_totals_summary_line(emerge_binary, fixture_env):
    """Real output.py::display: `if self.conf.verbosity == 3:
    self.print_verbose(...)` -> `writemsg_stdout(f"\\n{self.counters}\\n")`
    -- the trailing `Total: N packages (...)` line, only under `-v`,
    including `, Size of downloads: N KiB` (real `_calc_size` /
    `localized_size`, KiB-only, no locale grouping) and the `Fetch
    Restriction: N package[s][ (M unsatisfied)]` line. Ported minus the
    `Conflict:` line's `(N unsatisfied)`/`(all satisfied)` suffix (this
    portuale resolves no blocker)."""
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
        '[ebuild  N     ] dev-libs/common-1.0 ',
        '[ebuild  N     ] dev-libs/shared-a-1.0 ',
        '[ebuild  N     ] dev-libs/shared-b-1.0 ',
    ]


def test_multiple_top_level_atoms_reconcile_a_solvable_slot_conflict_between_targets(
    emerge_binary, fixture_env
):
    """Same solvable fixture pair as
    test_solvable_slot_conflict_is_reconciled_by_backtracking, but
    requested directly as two top-level atoms instead of reached through a
    shared parent -- backtracking reconciles a slot conflict between two
    *targets* (not just between two dependencies of one target) too, since
    resolve_pretend_graph seeds all top-level atoms into the same
    BFS/slot_want bookkeeping the retry loop reads."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/slotconflictnewconsumer", "dev-libs/slotconflictoldconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/slotconflicttarget-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictnewconsumer-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictoldconsumer-1.0 ',
    ]


def test_multiple_top_level_atoms_report_an_unsolvable_slot_conflict_between_targets(
    emerge_binary, fixture_env
):
    """dev-libs/slotconflictnewpin (">=dev-libs/slotconflicttarget-2.0")
    and dev-libs/slotconflictoldpin ("<dev-libs/slotconflicttarget-2.0")
    as two top-level atoms: no common satisfying version, so backtracking
    leaves the slot-collision block in place."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/slotconflictnewpin", "dev-libs/slotconflictoldpin"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines()[:3] == [
        '[ebuild  N     ] dev-libs/slotconflicttarget-2.0 ',
        '[ebuild  N     ] dev-libs/slotconflictnewpin-1.0 ',
        '[ebuild  N     ] dev-libs/slotconflictoldpin-1.0 ',
    ]
    _assert_slot_collision_block(
        result.stdout,
        "dev-libs/slotconflicttarget:0",
        [
            (
                "dev-libs/slotconflicttarget-2.0:0/0::testrepo",
                [("dev-libs/slotconflictnewpin-1.0:0/0::testrepo", ">=dev-libs/slotconflicttarget-2.0")],
            ),
            (
                "dev-libs/slotconflicttarget-1.0:0/0::testrepo",
                [("dev-libs/slotconflictoldpin-1.0:0/0::testrepo", "<dev-libs/slotconflicttarget-2.0")],
            ),
        ],
    )


def test_multiple_top_level_atoms_dedupe_a_literal_duplicate(emerge_binary, fixture_env):
    """emerge --pretend foo foo: the second occurrence dedupes silently
    via the existing visited-atoms set, same as a dependency cycle does
    -- exactly one [ebuild N] line, not two."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/newpkg", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
    ]


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
    # The first line is the abort; --misspell-suggestions (default on)
    # appends its own block for a genuinely-missing cp.
    assert result.stderr.splitlines()[0] == (
        'emerge: there are no ebuilds to satisfy "dev-libs/does-not-exist".'
    )
    assert "emerge: searching for similar names..." in result.stderr


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
        assert result.stdout.splitlines() == ["[ebuild  N     ] dev-libs/newpkg-1.0 "], atom


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
        == 'emerge: "!!dev-libs/newpkg" is a blocker, not a valid emerge target'
    )


def test_verbose_shows_use_flags_gated_by_profile_and_make_conf(emerge_binary, fixture_env):
    """dev-libs/useflagpkg declares IUSE="foo missingflag"; the fixture
    profile chain resolves "foo" enabled and "missingflag" disabled (see
    portage-profile's own fixture test) -- the USE= line shows both,
    enabled plain and disabled "-"-prefixed, alphabetically ordered. Real
    `print_use_string = verbosity != 1` and default `emerge -p` verbosity
    is 2, so a New package's full USE list shows at plain -p too; only the
    `::repo` cpv decoration and the counters line are -v-only."""
    verbose = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/useflagpkg"], fixture_env
    )
    assert verbose.returncode == 0
    assert verbose.stdout.splitlines()[1] == (
        '[ebuild  N     ] dev-libs/useflagpkg-1.0::testrepo  USE="foo -missingflag"'
    )

    quiet = _run([str(emerge_binary)], ["--pretend", "dev-libs/useflagpkg"], fixture_env)
    assert quiet.returncode == 0
    assert quiet.stdout.splitlines()[1] == (
        '[ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"'
    )
    # -v-only: the ::repo decoration and the counters line.
    assert "::testrepo" not in quiet.stdout
    assert "Total:" not in quiet.stdout


def test_use_line_at_p_is_full_for_a_new_pkg_and_changed_only_for_a_reinstall(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `print_use_string = verbosity != 1` (not -v-gated); it's
    `all_flags = verbosity == 3` that changes *which* flags render. A New
    package's `is_new` branch renders every IUSE flag regardless, so its
    USE list is identical at -p and -pv (the -pv-only additions are the
    `::repo` cpv decoration and the counters line). A Reinstall/Upgrade
    at plain -p shows only the *changed* flags (`_create_use_string`
    leaves an unchanged flag omitted when `all_flags` is off), where -pv
    shows the whole diff plus the `(-flag%)` removed list."""
    for pkg, use in [
        ("useflagpkg", 'USE="foo -missingflag"'),
        ("useexpandpkg", 'VIDEO_CARDS="nvidia -amdgpu"'),
        ("iusedefaultpkg", 'USE="enableddefault plainflag -disableddefault"'),
    ]:
        p = _run([str(emerge_binary)], ["--pretend", f"dev-libs/{pkg}"], fixture_env)
        pv = _run([str(emerge_binary)], ["--pretend", "-v", f"dev-libs/{pkg}"], fixture_env)
        assert p.returncode == 0
        assert p.stdout == _run(
            emerge_pretend_python, ["--pretend", f"dev-libs/{pkg}"], fixture_env
        ).stdout, pkg
        p_line = next(l for l in p.stdout.splitlines() if f"/{pkg}-1.0" in l)
        pv_line = next(l for l in pv.stdout.splitlines() if f"/{pkg}-1.0" in l)
        assert p_line == f"[ebuild  N     ] dev-libs/{pkg}-1.0  {use}", pkg
        assert pv_line == f"[ebuild  N     ] dev-libs/{pkg}-1.0::testrepo  {use}", pkg

    # An Upgrade with a real USE diff: -p shows only the changes,
    # -pv shows everything (unchanged `keep`, removed `(-drop%)`).
    up = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "dev-libs/upgradeusepkg"],
        fixture_env,
    )
    upv = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--update", "dev-libs/upgradeusepkg"],
        fixture_env,
    )
    assert up.returncode == 0
    assert up.stdout == _run(
        emerge_pretend_python,
        ["--pretend", "--update", "dev-libs/upgradeusepkg"],
        fixture_env,
    ).stdout
    assert (
        next(l for l in up.stdout.splitlines() if "upgradeusepkg-2.0" in l)
        == '[ebuild     U  ] dev-libs/upgradeusepkg-2.0 [1.0] USE="added%* -change*"'
    )
    assert (
        next(l for l in upv.stdout.splitlines() if "upgradeusepkg-2.0" in l)
        == '[ebuild     U  ] dev-libs/upgradeusepkg-2.0::testrepo [1.0::testrepo]'
        ' USE="added%* keep -change* (-drop%)"'
    )


def test_reinst_flags_force_show_a_dropped_iuse_trigger_flag_at_plain_p(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `_create_use_string`'s `reinst_flag` (`reinst_flags_map`, the
    Reinstall's own `_reinstall_for_flags` trigger set): a flag is shown
    even when otherwise-unchanged if it triggered the reinstall. The one
    case this changes at plain -p: `dev-libs/reinstdropiusepkg`'s vdb has
    IUSE="gone keep" with `gone` enabled, but the current ebuild dropped
    `gone` from IUSE -- so `--newuse`/`--changed-use` reinstall it, with
    `{gone}` as the trigger set, and `gone` now shows in the `(-flag%)`
    removed list at -p (previously -pv-only). `keep` (unchanged-disabled,
    not a trigger) stays omitted at -p."""
    for flag in ("--newuse", "--changed-use"):
        p = _run(
            [str(emerge_binary)],
            ["--pretend", flag, "dev-libs/reinstdropiusepkg"],
            fixture_env,
        )
        assert p.returncode == 0
        assert p.stdout == _run(
            emerge_pretend_python,
            ["--pretend", flag, "dev-libs/reinstdropiusepkg"],
            fixture_env,
        ).stdout, flag
        assert p.stdout.strip() == (
            '[ebuild   R    ] dev-libs/reinstdropiusepkg-1.0  USE="(-gone%*)"'
        ), flag

    pv = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--newuse", "dev-libs/reinstdropiusepkg"],
        fixture_env,
    )
    assert pv.stdout == _run(
        emerge_pretend_python,
        ["--pretend", "-v", "--newuse", "dev-libs/reinstdropiusepkg"],
        fixture_env,
    ).stdout
    # -pv already showed every flag: unchanged `-keep` plus the removed one.
    assert next(
        l for l in pv.stdout.splitlines() if "reinstdropiusepkg-1.0" in l
    ) == '[ebuild   R    ] dev-libs/reinstdropiusepkg-1.0::testrepo  USE="-keep (-gone%*)"'


def test_verbose_use_order_is_enabled_first_and_alphabetical_flips_it(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real output_helpers.py::_create_use_string joins `enabled +
    disabled` -- enabled flags first, then disabled, each alphabetical --
    unless --alphabetical, which joins one combined bare-name-sorted
    list. dev-libs/iusedefaultpkg resolves enableddefault + plainflag on,
    disableddefault off; the disabled flag sorts first alphabetically, so
    the two orderings differ. Applies to USE_EXPAND groups too
    (dev-libs/useexpandpkg's VIDEO_CARDS)."""
    default = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "dev-libs/iusedefaultpkg", "dev-libs/useexpandpkg"],
        fixture_env,
    )
    assert default.returncode == 0
    lines = default.stdout.splitlines()
    assert lines[0] == (
        '[ebuild  N     ] dev-libs/iusedefaultpkg-1.0::testrepo  USE="enableddefault plainflag -disableddefault"'
    )
    assert any(
        ln == '[ebuild  N     ] dev-libs/useexpandpkg-1.0::testrepo  VIDEO_CARDS="nvidia -amdgpu"'
        for ln in lines
    )

    alpha = _run(
        [str(emerge_binary)],
        ["--pretend", "-v", "--alphabetical", "dev-libs/iusedefaultpkg", "dev-libs/useexpandpkg"],
        fixture_env,
    )
    assert alpha.returncode == 0
    alines = alpha.stdout.splitlines()
    assert alines[0] == (
        '[ebuild  N     ] dev-libs/iusedefaultpkg-1.0::testrepo  USE="-disableddefault enableddefault plainflag"'
    )
    assert any(
        ln == '[ebuild  N     ] dev-libs/useexpandpkg-1.0::testrepo  VIDEO_CARDS="-amdgpu nvidia"'
        for ln in alines
    )

    # Both implementations agree, both forms.
    py_default = _run(
        emerge_pretend_python,
        ["--pretend", "-v", "dev-libs/iusedefaultpkg", "dev-libs/useexpandpkg"],
        fixture_env,
    )
    py_alpha = _run(
        emerge_pretend_python,
        ["--pretend", "-v", "--alphabetical", "dev-libs/iusedefaultpkg", "dev-libs/useexpandpkg"],
        fixture_env,
    )
    assert default.stdout == py_default.stdout
    assert alpha.stdout == py_alpha.stdout


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
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo '
    )

    disable = _run(
        [str(emerge_binary)], ["--pretend", "-v", "dev-libs/packageusedisablepkg"], fixture_env
    )
    assert disable.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/packageusedisablepkg-1.0::testrepo  USE="-foo"',
        '',
        'Total: 1 package (1 new), Size of downloads: 0 KiB',
    ]


def test_verbose_on_a_package_with_no_iuse_shows_no_use_line(emerge_binary, fixture_env):
    """dev-libs/newpkg declares no IUSE at all -- -v must not print an
    empty USE="" line, matching real portage's own "nothing to show"
    behavior (_create_use_string returns "" when there's nothing to
    join)."""
    result = _run([str(emerge_binary)], ["--pretend", "-v", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '',
        'Total: 1 package (1 new), Size of downloads: 0 KiB',
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
    assert disabled.stdout.splitlines()[0] == '[ebuild  N     ] dev-libs/newpkg-1.0 '
    # ::repo decoration + the counters line are the -v-only signals (a
    # New package's USE list shows at plain -p regardless).
    assert "::testrepo" not in disabled.stdout
    assert "Total:" not in disabled.stdout

    enabled = _run(
        [str(emerge_binary)], ["--pretend", "-v", "y", "dev-libs/useflagpkg"], fixture_env
    )
    assert enabled.returncode == 0
    assert enabled.stdout.splitlines()[0] == '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo '


def test_verbose_inline_equals_form_consumes_y_or_n(emerge_binary, fixture_env):
    """--verbose=y / --verbose=n (argparse's own native "=" syntax, a
    separate mechanism from insert_optional_args's next-token lookahead)
    must be honored the same way."""
    disabled = _run(
        [str(emerge_binary)], ["--pretend", "--verbose=n", "dev-libs/useflagpkg"], fixture_env
    )
    assert disabled.returncode == 0
    assert "::testrepo" not in disabled.stdout
    assert "Total:" not in disabled.stdout

    enabled = _run(
        [str(emerge_binary)], ["--pretend", "--verbose=y", "dev-libs/useflagpkg"], fixture_env
    )
    assert enabled.returncode == 0
    assert 'dev-libs/useflagpkg-1.0::testrepo  USE="foo -missingflag"' in enabled.stdout


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
            == '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo '
        ), bundle


def test_short_flag_bundle_reports_the_first_out_of_scope_character(
    emerge_binary, fixture_env
):
    """-pf (pretend + real-but-unimplemented -f/--fetchonly) and -pz
    (pretend + a genuinely unrecognized "-z") each decompose left to
    right, processing "-p" silently and then reporting on the next
    character exactly as a standalone occurrence of it would -- same
    messages, same exit code."""
    unimplemented = _run(
        [str(emerge_binary)], ["-pf", "dev-libs/useflagpkg"], fixture_env
    )
    assert unimplemented.returncode == 2
    assert (
        unimplemented.stderr.strip()
        == 'emerge: option "--fetchonly" is a real emerge option, but is not yet '
        'implemented in portuale -- run "emerge --help" for the options '
        "and actions that are."
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
    treat "n" as a positional target, not as -v's value -- proven here
    by "n" being category-qualified as a (nonexistent) bare package name
    (real dep_expand), not by "n" silently disabling verbose."""
    result = _run([str(emerge_binary)], ["-pv", "n"], fixture_env)
    assert result.returncode == 1
    assert result.stderr.strip() == 'emerge: there are no ebuilds to satisfy "n".'


def test_help_prints_a_pilot_specific_summary_not_real_emerges_own(
    emerge_binary, fixture_env
):
    """--help/-h is real and implemented, but the text is a grouped tour
    of what portuale actually does -- not a port of real emerge's own
    _emerge/help.py (157 lines of colorized usage syntax for its full
    ~130-flag surface). Pinned in full since it's portuale's own
    content, byte-identical to emerge_pretend_reference.py's _HELP_TEXT."""
    result = _run([str(emerge_binary)], ["--help"], fixture_env)
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout == r"""emerge: command-line interface to the Portuale package manager

Portuale is a drop-in Rust reimplementation of Portage: same behaviour,
verified against the Python original by a shared test suite. Any real
emerge option or action not listed below is recognized by name
(lib/_emerge/main.py) -- using one reports that it is not yet implemented
in portuale, rather than a generic error.

Usage:
  emerge [options] <target> ...            build and merge the targets, resolving dependencies
  emerge --pretend [options] <target> ...  show what would be merged; change nothing
  emerge --unmerge <atom> ...              remove matching packages
  emerge <action> [options]                run one of the actions listed below
  emerge --help
  A <target> is an atom, an @set, or an installed file / ebuild / tbz2 / gpkg.

Actions (with none of these, the targets are built and merged):
  -C, --unmerge              remove matching packages with no dependency check (CLEAN_DELAY countdown)
      --rage-clean           like --unmerge, with CLEAN_DELAY=0
  -c, --depclean             remove packages that nothing explicitly installed still needs
  -P, --prune                remove all but the highest installed version of a package
      --clean                remove all but the most recently installed version in each slot
      --config               run pkg_config for an installed package
  -W, --deselect[=y|n]       drop atoms / sets from the world favourites (implied by the removals above)
  -s, --search               search package names; -S / --searchdesc also matches DESCRIPTION
      --info                 print the configuration / build-environment block for bug reports
      --list-sets            list the available package sets
      --check-news           report how many unread GLEP 42 news items there are
      --regen                regenerate every repo's metadata/md5-cache (runs each depend phase)
      --metadata             no-op here (md5-cache is read directly); prints the real header only
  -r, --resume [--skipfirst] replay the merge list saved by the last failed run (--skipfirst drops entry 1)
  -h, --help                 show this message and exit

Dependency and target selection:
  -u, --update               pull a newer visible version even if the installed one satisfies the atom
  -D, --deep[=N]             also recurse through installed packages' dependencies (optionally N levels)
  -N, --newuse               reinstall an installed package whose USE settings changed
  -U, --changed-use          like -N, but ignore flags newly added to or removed from IUSE
  -e, --emptytree            rebuild the whole dependency tree, treating nothing as installed
  -n, --noreplace            leave a directly named, still satisfied installed atom alone
      --selective[=y|n]      same as --noreplace; =n cancels it
  -1, --oneshot              merge without recording the target in world / world_sets
  -o, --onlydeps             merge the targets' dependencies but not the targets themselves
  -O, --nodeps               ignore dependencies entirely
  -X, --exclude ATOMS        never act on a matching package (repeatable, space separated)
      --newrepo              reinstall if the package would now come from a different repo
      --changed-deps[=y|n], --changed-deps-report[=y|n]  react to a *DEPEND differing from the vdb record
      --changed-slot[=y|n]  reinstall if the ebuild's SLOT differs from the vdb record
      --with-bdeps <y|n>, --with-bdeps-auto <y|n>  keep DEPEND/BDEPEND when --deep walks installed packages
      --with-test-deps[=y|n]  also pull a target's test?-gated dependencies
      --root-deps[=rdeps|True]  resolve build dependencies against the running root
      --reinstall-atoms ATOMS  force-reinstall matching installed packages
      --rebuild-if-unbuilt, -new-rev, -new-ver, -new-slot  rebuild an installed package when a build dep is merged
      --rebuild-exclude ATOMS, --rebuild-ignore ATOMS  keep packages out of the rebuild triggers
      --complete-graph[=y|n], --complete-graph-if-new-use, --complete-graph-if-new-ver  force a full deep graph walk
      --dynamic-deps[=y|n]  walk the ebuild (y, default) or the vdb snapshot (n) during --deep
      --backtrack N         maximum resolver backtracking passes (default 10; 0 disables)
      --package-moves[=y|n]  apply profiles/updates/ package moves (default y)
      --misspell-suggestions[=y|n]  suggest close names for a missing cat/pkg

Autounmask (read-only: prints the required changes and stops -- never writes config):
      --autounmask[=y|n], --autounmask-use[=y|n], --autounmask-keep-keywords[=y|n]
      --autounmask-license[=y|n], --autounmask-keep-masks[=y|n]
      --autounmask-only[=y|n]  resolve, print only the change block, and exit 0
      --autounmask-backtrack<y|n>  keep re-resolving after autounmask changes (off by default)
      --autounmask-continue[=y|n]  recognized; implies --autounmask-backtrack=y

Binary packages:
  -b, --buildpkg[=y|n]       also build a binary package for each merged package
  -B, --buildpkgonly         build binary packages only; never merge
      --buildpkg-exclude ATOMS  skip the binary package for matching packages
  -k, --usepkg / -K, --usepkgonly  use a usable binary package when one exists (-K: only, never build)
  -g, --getbinpkg / -G, --getbinpkgonly  also fetch binary packages from a remote binhost (-G: only)
      --usepkg-exclude ATOMS, --usepkg-include ATOMS  narrow which packages may come from a binary
      --binpkg-respect-use[=y|n]  reject a binary package built with the wrong USE
      --rebuilt-binaries[=y|n], --rebuilt-binaries-timestamp N  prefer / bound rebuilt binary packages
      --useoldpkg-atoms ATOMS  prefer an existing binary package for matching atoms
      --quickpkg-direct[=y|n], --quickpkg-direct-root DIR  reuse another root's installed packages as binaries

Build scheduling:
  -j, --jobs[=N]             run up to N package builds in parallel
  -l, --load-average N       hold new builds while the load average exceeds N
  -a, --ask[=y|n]            prompt for confirmation before a real merge or removal
      --keep-going           on a build failure, drop that package's dependents and carry on
      --quiet-build[=y|n]    redirect a build's phase output to ${T}/build.log (implied by -j >1 and -q)

Output:
  -p, --pretend             resolve and print the merge list; do nothing
  -v, --verbose[=y|n]       add the USE="..." column to each [ebuild ...] line
  -q, --quiet[=y|n]         verbosity level 1: drop the mask column and the USE line
  -t, --tree                show the merge list as a dependency tree
      --columns             lay the merge list out in columns (not together with --tree)
      --unordered-display, --alphabetical  merge-list ordering variants
      --color <y|n>         force colour output on or off
      --verbose-slot-rebuilds[=y|n]  show the atoms forcing a slot-operator rebuild
      --verbose-conflicts   list every parent of a slot conflict, not one per collision reason
      --ignore-built-slot-operator-deps[=y|n]  ignore recorded := slot-operator dependencies
      --depclean-lib-check[=y|n]  with --depclean/--prune: scan for soname breakage (default y)
  -d, --debug               run ebuild phases under `set -x` (PORTAGE_DEBUG=1); no effect under --pretend

Portuale extensions (not real emerge options):
      --json                dump the resolved graph as one JSON line instead of the display
      --shell <bash|brush>  which real shell runs a merge / unmerge / --config phase chain (default bash)

emerge --sync is a permanent non-goal: it prints
"Functionality has moved to `emaint sync`." and exits 1.
See README.md and emerge(1) for the full picture.
"""


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
        ["--accept-properties", "--help"],
        ["-ph"],
        ["--help", "dev-libs/newpkg"],
    ):
        result = _run([str(emerge_binary)], args, fixture_env)
        assert result.returncode == 0, args
        assert result.stdout.startswith(
            "emerge: command-line interface to the Portuale package manager"
        ), args


def test_world_expands_to_the_fixture_world_files_own_atoms(emerge_binary, fixture_env):
    """fixtures/var/lib/portage/world (real portage's own
    WORLD_FILE, <ROOT>/var/lib/portage/world) lists dev-libs/newpkg and
    dev-libs/withdeps (which itself recurses into newpkg again -- deduped
    -- and upgradepkg), plus a "@some-nested-set-reference" line that
    must be silently skipped, not mishandled (a "@"-prefixed line in the
    plain world FILE itself really does fail real portage's own
    validation too -- see _read_world_atoms's own docstring; nested sets
    live in the separate world_sets file, exercised below in this same
    test) -- proving @world expansion feeds the exact same multi-atom/
    recursion machinery every other invocation already uses, not a
    separate code path. fixtures/var/lib/portage/world_sets
    lists "@nestedtestset" (fixtures/etc/portage/sets/
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/nestedsetpkg-1.0 is already installed; nothing to do',
        '[ebuild  N     ] dev-libs/innernestedsetpkg-1.0 ',
        'dev-libs/dualslotpkg-2.0 is already installed; nothing to do',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
        '[ebuild  N     ] dev-libs/withdeps-1.0 ',
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
        'dev-libs/samepkg-1.0 is already installed; nothing to do',
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/nestedsetpkg-1.0 is already installed; nothing to do',
        '[ebuild  N     ] dev-libs/innernestedsetpkg-1.0 ',
        'dev-libs/dualslotpkg-2.0 is already installed; nothing to do',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
        '[ebuild  N     ] dev-libs/withdeps-1.0 ',
    ]


def test_custom_set_as_a_top_level_target_expands_to_its_members(
    emerge_binary, fixture_env
):
    """`emerge @nestedtestset` (a user-defined set given directly, not via
    @world) expands through the same resolve_custom_set machinery the
    --unmerge/--depclean/--deselect paths use: dev-libs/nestedsetpkg
    (installed -> [ebuild R], non-selective) + dev-libs/innernestedsetpkg
    (from the nested @innernestedset reference; the cycle back to
    @nestedtestset contributes nothing). Also works alongside an explicit
    atom, expanding in place."""
    result = _run([str(emerge_binary)], ["--pretend", "@nestedtestset"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild   R    ] dev-libs/nestedsetpkg-1.0 ",
        "[ebuild  N     ] dev-libs/innernestedsetpkg-1.0 ",
    ]
    combined = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/samepkg", "@nestedtestset"],
        fixture_env,
    )
    assert combined.returncode == 0
    assert combined.stdout.splitlines() == [
        "[ebuild   R    ] dev-libs/samepkg-1.0 ",
        "[ebuild   R    ] dev-libs/nestedsetpkg-1.0 ",
        "[ebuild  N     ] dev-libs/innernestedsetpkg-1.0 ",
    ]


def test_selected_set_expands_the_same_as_world(emerge_binary, fixture_env):
    """Real cnf/sets/portage.conf: @world = @profile @selected @system,
    and @selected = WorldSelectedSet (the world file's atoms + world_sets'
    nested sets). Portuale's @world already IS that (the @profile /
    @system union is a pre-existing simplification), so @selected is the
    exact same expansion."""
    a = _run([str(emerge_binary)], ["--pretend", "--update", "@selected"], fixture_env)
    b = _run([str(emerge_binary)], ["--pretend", "--update", "@world"], fixture_env)
    assert a.returncode == 0
    assert a.stdout == b.stdout
    assert a.stdout != ""


def test_installed_set_expands_to_a_slot_atom_per_vdb_package(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """Real @installed (EverythingSet): a `cat/pkg:slot` atom for every
    package under var/db/pkg -- always slot-qualified, even for a lone
    slot (bug #338959). A test-local ROOT with dev-libs/dualslotpkg-1.0
    (SLOT=1) and dev-libs/nestedsetpkg-1.0 (SLOT=0) installed; the slot
    atom `dev-libs/dualslotpkg:1` pins slot 1, so 1.0 reinstalls rather
    than upgrading to the repo's 2.0 (SLOT=2)."""
    for name, slot in (("dualslotpkg-1.0", "1"), ("nestedsetpkg-1.0", "0")):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / name
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text(f"{slot}\n")
        (d / "repository").write_text("testrepo\n")
    env = dict(fixture_env)
    env["ROOT"] = str(tmp_path)
    args = ["--pretend", "@installed"]
    result = _run([str(emerge_binary)], args, env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "[ebuild   R    ] dev-libs/dualslotpkg-1.0 ",
        "[ebuild   R    ] dev-libs/nestedsetpkg-1.0 ",
    ]
    assert _run(emerge_pretend_python, args, env).stdout == result.stdout


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
        == "emerge: no package atoms to resolve (the target list, "
        "after expanding any @world/@selected/@system/@installed/@<set>, is empty)"
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
    tests stay fully isolated from the shared fixtures tree
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


def test_depclean_args_deselect_n_keeps_a_world_member(emerge_binary, fixture_env, tmp_path):
    """Real action_depclean's `deselect = myopts.get("--deselect") !=
    "n"` (default True): `-pc <atom>` in args mode empties the world
    "selected" set so a named world member still gets removed
    (actions.py:1037). `-pc <atom> --deselect=n` keeps the world set as a
    protection root, so a world member named as an arg is KEPT. Also
    proves `--depclean --deselect` no longer wrongly routes to the
    standalone deselect action (real: `myaction` is already `depclean`)."""
    env = _depclean_env(fixture_env, tmp_path)
    # default: dcworld (in @world, nothing needs it) -> removed
    default = _run([str(emerge_binary)], ["--pretend", "-c", "dev-libs/dcworld"], env)
    assert default.returncode == 0
    assert " dev-libs/dcworld" in default.stdout.splitlines()
    assert default.stdout.splitlines()[-1] == "Number to remove:     1"
    # --deselect=n: dcworld stays world-protected -> nothing to remove
    for flag in (["--deselect=n"], ["--deselect", "n"]):
        kept = _run(
            [str(emerge_binary)], ["--pretend", "-c", "dev-libs/dcworld", *flag], env
        )
        assert kept.returncode == 0, flag
        assert ">>> No packages selected for removal by depclean" in kept.stdout, flag
        assert kept.stdout.splitlines()[-1] == "Number to remove:     0", flag


def test_depclean_without_pretend_is_no_longer_gated(emerge_binary, fixture_env):
    """`emerge --depclean` WITHOUT `--pretend` really removes the cleanlist
    now (pretend.rs's execute_unmerge). Not a Rust-vs-Python contract
    case: the Python reference has no ebuild-execution machinery and just
    returns 0. The real removal is covered in test_portuale.py. Here we
    only assert the old `requires --pretend` exit-2 gate is gone, using a
    non-matching atom so nothing is removed from the read-only fixture."""
    result = _run(
        [str(emerge_binary)], ["--depclean", "dev-libs/nonexistent"], fixture_env
    )
    assert "requires --pretend" not in result.stderr


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
        ["--pretend", "-c", "dev-libs/dcworld", "--deselect=n"],
        ["--pretend", "-c", "dev-libs/dcworld", "--deselect", "n"],
        ["--pretend", "-c", "dev-libs/dcorphan", "--deselect=n"],
        ["--pretend", "-c", "dcorphan"],
        ["--pretend", "-c", "dev-libs/dcorphan", "dev-libs/nope"],
        ["--pretend", "-c", "dev-libs/nope"],
        ["--pretend", "-c", "dev-libs/dcbuilddep"],
        ["--pretend", "-c", "dev-libs/dcbdep"],
        ["--pretend", "--depclean", "--verbose"],
        ["--pretend", "-c", "-v", "dev-libs/dcdep"],
        ["--pretend", "-c", "-v", "dev-libs/dcorphan"],
    ):
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def _libcheck_root(tmp_path):
    """A ROOT for --depclean-lib-check: dcconsumer (kept via @world ->
    dcworld -> dcconsumer) links `libdclib.so.1` at the ELF level
    (NEEDED.ELF.2) but has NO package dependency on its provider. That
    provider, dev-libs/dclib, is otherwise an orphan -- nothing in the
    dependency graph needs it -- so the plain cleanlist is
    {dclib, dclibdep (dclib's own RDEPEND), dcplainorphan}. The
    `--depclean-lib-check` soname scan keeps dclib (and, via the
    re-closure, dclibdep); only dcplainorphan is actually removed."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("dev-libs/dcworld\n")

    def install(package, rdepend="", needed_elf=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")
        if needed_elf:
            (d / "NEEDED.ELF.2").write_text(needed_elf + "\n")

    install("dcworld", rdepend="dev-libs/dcconsumer")
    install(
        "dcconsumer",
        needed_elf="X86_64;/usr/bin/dcconsumer;;;libdclib.so.1",
    )
    install(
        "dclib",
        rdepend="dev-libs/dclibdep",
        needed_elf="X86_64;/usr/lib/libdclib.so.1;libdclib.so.1;;libc.so.6",
    )
    install("dclibdep")
    install("dcplainorphan")
    return tmp_path


def _libcheck_env(fixture_env, tmp_path):
    env = dict(fixture_env)
    env["ROOT"] = str(_libcheck_root(tmp_path))
    return env


def test_depclean_lib_check_keeps_a_link_level_only_provider(
    emerge_binary, fixture_env, tmp_path
):
    """emerge -pc: the default --depclean-lib-check soname scan keeps
    dev-libs/dclib (a surviving package links its lib with no package
    dep), and -- via the graph re-closure -- its own RDEPEND dclibdep.
    Only dcplainorphan is removed. The WARNING names the provider and
    the link-level consumer."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-c"], _libcheck_env(fixture_env, tmp_path)
    )
    assert result.returncode == 0
    cleaned = [ln for ln in result.stdout.splitlines() if ln.startswith(" dev-libs/")]
    assert cleaned == [" dev-libs/dcplainorphan"]
    assert result.stdout.splitlines()[-1] == "Number to remove:     1"

    err = result.stderr.splitlines()
    assert ">>> Checking for lib consumers..." in err
    assert ">>> Adding lib providers to graph..." in err
    assert (
        " * In order to avoid breakage of link level dependencies, one or more"
        in err
    )
    assert " *   dev-libs/dclib-1.0 pulled in by:" in err
    assert " *     dev-libs/dcconsumer-1.0 needs libdclib.so.1" in err


def test_depclean_lib_check_disabled_removes_the_provider_and_warns(
    emerge_binary, fixture_env, tmp_path
):
    """emerge -pc --depclean-lib-check=n: the soname scan is skipped, so
    dclib + dclibdep + dcplainorphan are all removed, and the no-args
    advisory gains the `Depclean may break link level dependencies`
    paragraph."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "-c", "--depclean-lib-check=n"],
        _libcheck_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    cleaned = [ln for ln in result.stdout.splitlines() if ln.startswith(" dev-libs/")]
    # Topological removal order: roots (dcplainorphan, dclib) cpv-desc,
    # then dclibdep (dclib's RDEPEND, unmerged after it).
    assert cleaned == [
        " dev-libs/dcplainorphan",
        " dev-libs/dclib",
        " dev-libs/dclibdep",
    ]
    assert " * Depclean may break link level dependencies. Thus, it is" in result.stdout
    assert ">>> Checking for lib consumers..." not in result.stderr


def test_depclean_lib_check_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = _libcheck_env(fixture_env, tmp_path)
    for args in (
        ["--pretend", "-c"],
        ["--pretend", "-c", "-v"],
        ["--pretend", "-c", "--depclean-lib-check=n"],
        ["--pretend", "-c", "--depclean-lib-check", "n"],
        ["--pretend", "-c", "--depclean-lib-check=y"],
        ["--pretend", "-c", "dev-libs/dclib"],
        ["--pretend", "-c", "dev-libs/dcplainorphan"],
        ["--pretend", "--prune"],
        ["--pretend", "--prune", "--depclean-lib-check=n"],
        ["--pretend", "-c", "--color=y"],
    ):
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def _unresolved_root(tmp_path):
    """A ROOT where a *kept* package has a hard runtime dependency no
    installed package satisfies. Real _calc_depclean's unresolved_deps()
    (actions.py:1137-1248) refuses to remove anything in that state.

    uworld (world) -> ukept, whose RDEPEND names dev-libs/umissing (not
    installed). uorphan would be the cleanlist, but the halt fires first.
    ukept also DEPENDs dev-libs/ubuildmissing (a SOFT buildtime dep --
    never trips the halt) and RDEPENDs `|| ( dev-libs/uany dev-libs/ualtmissing )`
    (uany installed, so the || group is satisfied and never flagged)."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("dev-libs/uworld\n")

    def install(package, rdepend="", depend="", pdepend=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")
        if depend:
            (d / "DEPEND").write_text(depend + "\n")
        if pdepend:
            (d / "PDEPEND").write_text(pdepend + "\n")

    install("uworld", rdepend="dev-libs/ukept")
    install(
        "ukept",
        rdepend="dev-libs/umissing || ( dev-libs/uany dev-libs/ualtmissing )",
        depend="dev-libs/ubuildmissing",
    )
    install("uany")
    install("uorphan")
    return tmp_path


def _unresolved_env(fixture_env, tmp_path):
    env = dict(fixture_env)
    env["ROOT"] = str(_unresolved_root(tmp_path))
    return env


def test_depclean_halts_on_an_unresolvable_runtime_dep(
    emerge_binary, fixture_env, tmp_path
):
    """emerge -pc: a kept package's unsatisfiable RDEPEND makes depclean
    print the `* Dependencies could not be completely resolved ...`
    block and exit 1 without removing anything (real actions.py:1247).
    The SOFT buildtime dep and the satisfied `||` group are not flagged."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "-c"], _unresolved_env(fixture_env, tmp_path)
    )
    assert result.returncode == 1
    assert ">>> Calculating removal order..." not in result.stdout
    assert " dev-libs/uorphan" not in result.stdout
    err = result.stderr
    assert " * Dependencies could not be completely resolved due to" in err
    assert " *   dev-libs/umissing pulled in by:\n *     dev-libs/ukept-1.0\n" in err
    assert (
        " *   emerge --update --newuse --deep --with-bdeps=y @world" in err
    )
    # SOFT buildtime dep + the satisfied `||` alternative are not flagged.
    assert "ubuildmissing" not in err
    assert "ualtmissing" not in err


def test_prune_halts_on_an_unresolvable_runtime_dep_with_the_nodeps_hint(
    emerge_binary, fixture_env, tmp_path
):
    """emerge -pP hits the same halt (real _calc_depclean serves
    action in ('depclean', 'prune')) and adds the prune-only
    `If you would like to ignore dependencies then use --nodeps.` line."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--prune"],
        _unresolved_env(fixture_env, tmp_path),
    )
    assert result.returncode == 1
    assert " * Dependencies could not be completely resolved due to" in result.stderr
    assert (
        " * If you would like to ignore dependencies then use --nodeps." in result.stderr
    )


def test_prune_nodeps_ignores_the_unresolvable_dep(emerge_binary, fixture_env, tmp_path):
    """--prune --nodeps routes around _calc_depclean entirely -- no dep
    check, so the halt never fires (there just happens to be nothing
    multi-version to prune here, so it exits 1 with the standard 'no
    packages' message, not the resolution-failure block)."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--prune", "--nodeps"],
        _unresolved_env(fixture_env, tmp_path),
    )
    assert "Dependencies could not be completely resolved" not in result.stderr


def test_depclean_unresolved_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = _unresolved_env(fixture_env, tmp_path)
    for args in (
        ["--pretend", "-c"],
        ["--pretend", "-c", "-v"],
        ["--pretend", "-c", "--color=y"],
        ["--pretend", "--prune"],
        ["--pretend", "--prune", "--color=y"],
        ["--pretend", "-c", "dev-libs/uorphan"],
    ):
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def _depclean_revdep_root(tmp_path):
    """A ROOT where the kept closure has a shared dependency (dcshared,
    pulled in by two parents) and a world member (dcworld). dcorphan +
    dcorphandep are the cleanlist."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("dev-libs/dcworld\n")

    def install(package, rdepend=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")

    install("dcworld", rdepend="dev-libs/dcdep dev-libs/dcshared")
    install("dcdep", rdepend="dev-libs/dcsub dev-libs/dcshared")
    install("dcsub")
    install("dcshared")
    install("dcorphan", rdepend="dev-libs/dcorphandep")
    install("dcorphandep")
    return tmp_path


def test_depclean_pretend_verbose_shows_reverse_deps(emerge_binary, fixture_env, tmp_path):
    """emerge -pc --verbose: real create_cleanlist's `elif "--verbose":
    show_parents(pkg)` -- for every KEPT installed package (cpv-sorted):
    '  <cpv> pulled in by:\\n    <parent> requires <atom>'. A world-file
    member's parent is @selected; parent lines are sorted; the blocks
    come after the ` * ` advisory and before '>>> Calculating removal
    order...'."""
    env = dict(fixture_env)
    env["ROOT"] = str(_depclean_revdep_root(tmp_path))
    result = _run([str(emerge_binary)], ["--pretend", "--depclean", "--verbose"], env)
    assert result.returncode == 0
    out = result.stdout
    # dcshared is pulled in by both dcdep and dcworld, lines sorted.
    assert (
        "  dev-libs/dcshared-1.0 pulled in by:\n"
        "    dev-libs/dcdep-1.0 requires dev-libs/dcshared\n"
        "    dev-libs/dcworld-1.0 requires dev-libs/dcshared\n"
    ) in out
    assert "  dev-libs/dcworld-1.0 pulled in by:\n    @selected requires dev-libs/dcworld\n" in out
    # The reverse-dep blocks precede the removal-order line.
    assert out.index("pulled in by:") < out.index(">>> Calculating removal order...")
    # dcorphan / dcorphandep are the cleanlist, not reverse-dep'd.
    assert "  dev-libs/dcorphan-1.0 pulled in by:" not in out

    # --verbose suppresses the "To see reverse dependencies" hint even
    # when there's nothing to remove.
    (tmp_path / "var" / "lib" / "portage" / "world").write_text(
        "dev-libs/dcworld\ndev-libs/dcorphan\n"
    )
    nothing = _run([str(emerge_binary)], ["--pretend", "--depclean", "--verbose"], env)
    assert nothing.returncode == 0
    assert ">>> No packages selected for removal by depclean" in nothing.stdout
    assert ">>> To see reverse dependencies" not in nothing.stdout


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
    # order there is not a meaningful spec; both portuale sides sort it).
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


def _depclean_cycle_root(tmp_path):
    """An orphan cleanlist whose members form a genuine dependency cycle,
    so real _calc_depclean's `ignore_priority_range` fallback
    (actions.py:1713-1727) kicks in: with no true root, it ignores the
    lowest edge-priority level and pops ONE node (cpv-max) to break the
    cycle. dev-libs/cyclicdepa DEPENDs dev-libs/cyclicdepb (buildtime,
    priority -4) and dev-libs/cyclicdepb RDEPENDs dev-libs/cyclicdepa
    (runtime, -2). At ignore_priority -4 only cyclicdepb qualifies (its
    single incoming edge is the -4 one) -> popped first, then cyclicdepa
    falls out as a plain root: [cyclicdepb, cyclicdepa]. dev-libs/keeper
    is world-listed so the world isn't empty."""
    portage_dir = tmp_path / "var" / "lib" / "portage"
    portage_dir.mkdir(parents=True)
    (portage_dir / "world").write_text("dev-libs/keeper\n")

    def install(package, rdepend="", depend=""):
        d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / f"{package}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
        if rdepend:
            (d / "RDEPEND").write_text(rdepend + "\n")
        if depend:
            (d / "DEPEND").write_text(depend + "\n")

    install("cyclicdepa", depend="dev-libs/cyclicdepb")
    install("cyclicdepb", rdepend="dev-libs/cyclicdepa")
    install("keeper")
    return tmp_path


def test_depclean_breaks_a_dependency_cycle_by_popping_one_node(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = dict(fixture_env)
    env["ROOT"] = str(_depclean_cycle_root(tmp_path))
    args = ["--pretend", "--depclean"]
    result = _run([str(emerge_binary)], args, env)
    assert result.returncode == 0
    blocks = [
        ln.strip() for ln in result.stdout.splitlines() if ln.startswith(" dev-libs/")
    ]
    # The -4 (DEPEND) edge into cyclicdepb is dropped first; the -2
    # (RDEPEND) edge into cyclicdepa is preserved as long as possible.
    assert blocks == ["dev-libs/cyclicdepb", "dev-libs/cyclicdepa"]
    python = _run(emerge_pretend_python, args, env)
    assert result.stdout == python.stdout
    assert result.stderr == python.stderr


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


def test_prune_without_pretend_is_no_longer_gated(emerge_binary, fixture_env):
    """`emerge --prune` WITHOUT `--pretend` really removes now
    (pretend.rs's execute_unmerge). Not a Rust-vs-Python contract case
    (the Python reference just returns 0). Real removal is covered in
    test_portuale.py; here we only assert the exit-2 gate is gone, using a
    non-matching atom so nothing is removed from the read-only fixture."""
    result = _run(
        [str(emerge_binary)], ["--prune", "dev-libs/nonexistent"], fixture_env
    )
    assert "requires --pretend" not in result.stderr


def test_prune_nodeps_pretend_prunes_every_old_version(emerge_binary, fixture_env, tmp_path):
    """emerge -pP --nodeps (actions.py:2684): --nodeps bypasses
    _calc_depclean entirely and routes to _unmerge_display's prune branch
    -- NO dependency check, so EVERY non-highest version is selected even
    one something needs (in _prune_root, keeper pins =dev-libs/mm-2.0,
    but --nodeps prunes it anyway). The best version is `protected`,
    `omitted` is always `none`, and there's no ">>> Calculating removal
    order..." line."""
    env = dict(fixture_env)
    env["ROOT"] = str(_prune_root(tmp_path))
    result = _run([str(emerge_binary)], ["--pretend", "--prune", "--nodeps"], env)
    assert result.returncode == 0
    out = result.stdout
    assert ">>> Calculating removal order..." not in out
    assert (
        "\n dev-libs/mm\n"
        "    selected: 1.0 2.0 \n"
        "   protected: 3.0 \n"
        "     omitted: none \n"
    ) in out
    assert (
        "All selected packages: =dev-libs/aa-1.0 =dev-libs/mm-1.0 "
        "=dev-libs/mm-2.0 =dev-libs/zz-1.0" in out
    )
    # --verbose is inert here (show_parents lives on the _calc_depclean path).
    v = _run([str(emerge_binary)], ["--pretend", "--prune", "--nodeps", "-v"], env)
    assert "pulled in by:" not in v.stdout


def test_prune_nodeps_pretend_nothing_outdated(emerge_binary, fixture_env, tmp_path):
    """No multi-version cp -> real `global_unmerge and not numselected`
    prints ">>> No outdated packages were found on your system." and
    exits 1 (unlike plain --prune's exit 0). With an arg it's the
    ordinary "No packages selected" message instead."""
    root = tmp_path
    for n in ("one", "two"):
        d = root / "var" / "db" / "pkg" / "dev-libs" / f"{n}-1.0"
        d.mkdir(parents=True)
        (d / "CATEGORY").write_text("dev-libs\n")
        (d / "SLOT").write_text("0\n")
    env = dict(fixture_env)
    env["ROOT"] = str(root)
    noargs = _run([str(emerge_binary)], ["--pretend", "--prune", "--nodeps"], env)
    assert noargs.returncode == 1
    assert ">>> No outdated packages were found on your system." in noargs.stdout
    witharg = _run([str(emerge_binary)], ["--pretend", "--prune", "--nodeps", "dev-libs/one"], env)
    assert witharg.returncode == 1
    assert ">>> No packages selected for removal by prune" in witharg.stdout


def test_prune_nodeps_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = dict(fixture_env)
    env["ROOT"] = str(_prune_root(tmp_path))
    for args in (
        ["--pretend", "--prune", "--nodeps"],
        ["-pPO"],
        ["--pretend", "--prune", "--nodeps", "-v"],
        ["--pretend", "--prune", "--nodeps", "dev-libs/mm"],
        ["--pretend", "--prune", "--nodeps", "mm"],
        ["--pretend", "--prune", "--nodeps", "dev-libs/single"],
        ["--pretend", "--prune", "--nodeps", "dev-libs/nope"],
    ):
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, args
        assert rust.stdout == python.stdout, args
        assert rust.stderr == python.stderr, args


def test_prune_pretend_verbose_shows_reverse_deps(emerge_binary, fixture_env, tmp_path):
    """emerge -pP --verbose: real create_cleanlist's prune branch
    (actions.py:1339) also calls show_parents(pkg) -- but only for an
    args_set-matched KEPT version with a real Package parent. In
    _prune_root, dev-libs/keeper pins =dev-libs/mm-2.0, so mm-2.0 (kept,
    non-highest) is the one block; the highest versions (protected by the
    bare-cp seed, which show_parents filters) get no block. The
    ">>> To see reverse dependencies" hint is suppressed."""
    env = dict(fixture_env)
    env["ROOT"] = str(_prune_root(tmp_path))
    result = _run([str(emerge_binary)], ["--pretend", "--prune", "--verbose"], env)
    assert result.returncode == 0
    out = result.stdout
    assert (
        "  dev-libs/mm-2.0 pulled in by:\n"
        "    dev-libs/keeper-1.0 requires =dev-libs/mm-2.0\n"
    ) in out
    assert "  dev-libs/mm-3.0 pulled in by:" not in out
    assert "  dev-libs/aa-2.0 pulled in by:" not in out
    assert out.index("pulled in by:") < out.index(">>> Calculating removal order...")


def test_prune_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    env = dict(fixture_env)
    env["ROOT"] = str(_prune_root(tmp_path))
    for args in (
        ["--pretend", "--prune"],
        ["--pretend", "-P"],
        ["--pretend", "--prune", "--verbose"],
        ["--pretend", "-pvP"],
        ["--pretend", "--prune", "dev-libs/mm"],
        ["--pretend", "--prune", "-v", "dev-libs/mm"],
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


def _vdb_path_root(tmp_path):
    """A ROOT with one installed package (dev-libs/foo-1.0) whose vdb
    entry has a CONTENTS file and a copied <pf>.ebuild, so a literal path
    into it can be given to `emerge -C`."""
    d = tmp_path / "var" / "db" / "pkg" / "dev-libs" / "foo-1.0"
    d.mkdir(parents=True)
    (d / "CATEGORY").write_text("dev-libs\n")
    (d / "SLOT").write_text("0\n")
    (d / "CONTENTS").write_text("obj /usr/bin/foo 0000 1700000000\n")
    (d / "foo-1.0.ebuild").write_text("")
    return tmp_path, d


def test_unmerge_pretend_accepts_a_literal_vdb_path(emerge_binary, fixture_env, tmp_path):
    """Real unmerge.py:137-182: an `-C` argument that starts with `.`/`/`
    or ends `.ebuild` is a path into the vdb -- validated, echoed as the
    derived `=cat/pkg-ver`, and selected. A `.ebuild` suffix is stripped
    first."""
    root, pkgdir = _vdb_path_root(tmp_path)
    env = dict(fixture_env)
    env["ROOT"] = str(root)

    for target in (str(pkgdir), str(pkgdir / "foo-1.0.ebuild")):
        result = _run([str(emerge_binary)], ["--pretend", "-C", target], env)
        assert result.returncode == 0, target
        out = result.stdout.splitlines()
        assert out[0] == "=dev-libs/foo-1.0", target
        assert out[1] == ">>> These are the packages that would be unmerged:", target
        assert "    selected: 1.0 " in out, target
        assert "All selected packages: =dev-libs/foo-1.0" in out, target

    # A path that doesn't exist.
    missing = _run(
        [str(emerge_binary)], ["--pretend", "-C", str(root / "var/db/pkg/dev-libs/nope-9")], env
    )
    assert missing.returncode == 1
    assert missing.stdout.startswith("\n!!! The path '")
    assert "doesn't exist." in missing.stdout

    # An existing dir with no CONTENTS.
    bad = tmp_path / "notadb"
    bad.mkdir()
    nocont = _run([str(emerge_binary)], ["--pretend", "-C", str(bad)], env)
    assert nocont.returncode == 1
    assert nocont.stdout.rstrip() == f"!!! Not a valid db dir: {bad}"


def test_unmerge_pretend_vdb_path_matches_between_implementations(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    root, pkgdir = _vdb_path_root(tmp_path)
    env = dict(fixture_env)
    env["ROOT"] = str(root)
    (tmp_path / "notadb").mkdir()
    for target in (
        str(pkgdir),
        str(pkgdir / "foo-1.0.ebuild"),
        str(root / "var/db/pkg/dev-libs/nope-9"),
        str(tmp_path / "notadb"),
    ):
        args = ["--pretend", "-C", target]
        rust = _run([str(emerge_binary)], args, env)
        python = _run(emerge_pretend_python, args, env)
        assert rust.returncode == python.returncode, target
        assert rust.stdout == python.stdout, target
        assert rust.stderr == python.stderr, target


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


def test_unmerge_without_pretend_is_no_longer_gated(emerge_binary, fixture_env):
    """`emerge -C <atom>` WITHOUT `--pretend` is a real removal now
    (pretend.rs's execute_unmerge) -- the old `requires --pretend` exit-2
    gate is gone. Not a Rust-vs-Python contract case: the Python
    reference has no ebuild-execution machinery and just returns 0. The
    real removal (files gone, vdb entry gone, world deselected, the
    `>>> Unmerging (N of M)` lines) is covered in test_portuale.py. Here
    we only assert the gate no longer fires against the read-only shared
    fixture ROOT (nothing installed to match -> nothing removed)."""
    result = _run([str(emerge_binary)], ["--unmerge", "dev-libs/nonexistent"], fixture_env)
    assert "requires --pretend" not in result.stderr


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


def test_unmerge_pretend_set_warning_higher_slot_refinement(emerge_binary, fixture_env):
    """Real unmerge.py:421-441's `higher_slot`: the "still listed in the
    following package sets" warning is suppressed for a set when an
    installed *newer* version of the same cp *in a different slot* also
    matches the set atom -- removing this version leaves that set
    satisfied. dev-libs/dualslotpkg is installed in slot 1 (1.0) and
    slot 2 (2.0); etc/portage/sets/dualslotset lists the bare
    `dev-libs/dualslotpkg`, selected via world_sets."""
    # Unmerging the slot-1 version: slot 2 (higher) still matches the bare
    # set atom -> NO warning.
    low = _run(
        [str(emerge_binary)], ["--pretend", "-C", "dev-libs/dualslotpkg:1"], fixture_env
    )
    assert low.returncode == 0
    assert "still listed in the following package sets" not in low.stdout
    # Unmerging the slot-2 version: nothing higher -> warning shown, naming
    # the set.
    high = _run(
        [str(emerge_binary)], ["--pretend", "-C", "dev-libs/dualslotpkg:2"], fixture_env
    )
    assert high.returncode == 0
    assert high.stdout.splitlines()[1:4] == [
        "Package dev-libs/dualslotpkg-2.0 is going to be unmerged,",
        "but still listed in the following package sets:",
        "    dualslotset",
    ]


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
    version of portuale got this backwards -- see run_deselect's own
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
    "@" < "d" and canonical str.sort() ordering, which portuale's own
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


def test_deselect_without_pretend_is_no_longer_gated(emerge_binary, fixture_env, tmp_path):
    """`emerge --deselect` WITHOUT `--pretend` really rewrites the world /
    world_sets files now (run_deselect, real `world_set.replace`). Not a
    Rust-vs-Python contract case: the reference has no execution machinery
    (it prints `Removing` but doesn't write). The real write is covered in
    test_portuale.py. Here we only assert the old `requires --pretend`
    exit-2 gate is gone, using a non-matching atom so nothing changes."""
    result = _run(
        [str(emerge_binary)],
        ["--deselect", "dev-libs/nonexistent"],
        _deselect_env(fixture_env, tmp_path),
    )
    assert result.returncode == 0
    assert "requires --pretend" not in result.stderr


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
    assert result.stdout == '[ebuild   R    ] dev-libs/foo-1.0 \n'


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
    """fixtures/repo/profiles/base/packages contributes
    dev-libs/newpkg (plus a non-"*"-prefixed "hint" line that must never
    contribute an atom of its own), and fixtures/repo/profiles/
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/systempkg-1.0 is already installed; nothing to do',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
        '[ebuild  N     ] dev-libs/withdeps-1.0 ',
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
        'dev-libs/samepkg-1.0 is already installed; nothing to do',
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/systempkg-1.0 is already installed; nothing to do',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
        '[ebuild  N     ] dev-libs/withdeps-1.0 ',
    ]


def test_unknown_set_name_as_a_top_level_atom_is_a_real_error(emerge_binary, fixture_env):
    """`@world`/`@system` expand to their own atom lists; any other
    `@name` is a user-defined file-based set, resolved via
    etc/portage/sets/<name> (real StaticFileSet). A `@name` with no
    matching set file is a real, immediate configuration error (real
    PackageSetNotFound), not a silent no-op -- same error the
    world_sets code path already produces for an unresolvable name."""
    result = _run([str(emerge_binary)], ["--pretend", "@some-other-set"], fixture_env)
    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == "emerge: set 'some-other-set' not found"


def test_newuse_reinstalls_a_package_whose_use_changed(emerge_binary, fixture_env):
    """fixtures/var/db/pkg/dev-libs/reinstallpkg-1.0 is installed
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild   R    ] dev-libs/reinstallpkg-1.0  USE="foo*"',
                                         ]


def test_newuse_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-N is --newuse's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pN") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pN", "dev-libs/reinstallpkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild   R    ] dev-libs/reinstallpkg-1.0  USE="foo*"',
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
        '[ebuild  N     ] dev-libs/newpkg-1.0::testrepo ',
        '[ebuild   R    ] dev-libs/reinstallpkg-1.0::testrepo  USE="foo*"',
        '',
        'Total: 2 packages (1 new, 1 reinstall), Size of downloads: 0 KiB',
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
    flag fixtures/repo/profiles/base/use.mask masks off, so it
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
        (
        '[ebuild   R    ] dev-libs/changedusepkg-1.0  USE="-brandnewflag%"\n'
        )
    )

    changed_use_result = _run(
        [str(emerge_binary)],
        ["--pretend", "--changed-use", "dev-libs/changedusepkg"],
        fixture_env,
    )
    assert changed_use_result.returncode == 0
    assert changed_use_result.stdout == (
        'dev-libs/changedusepkg-1.0 is already installed; nothing to do\n'
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild   R    ] dev-libs/reinstallpkg-1.0  USE="foo*"',
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
    portuale reports a reinstall and recurses into the CURRENT ebuild's own
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild   R    ] dev-libs/changeddepspkg-1.0 ',
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
    key, which portuale now mirrors: the move registers as changed."""
    args = ["--pretend", "--changed-deps", "dev-libs/movedkeydepspkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert python.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild   R    ] dev-libs/movedkeydepspkg-1.0 ",
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
    everywhere, redundant brackets collapsed. Portuale's
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
            f"[ebuild   R    ] dev-libs/{pkg}-1.0 ",
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
    --changed-slot is given, portuale reports a reinstall. Without
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild   R    ] dev-libs/changedslotpkg-1.0 ',
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild   R    ] dev-libs/changedslotpkg-1.0 ',
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/withtestdeppkg-1.0  USE="-test"',
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
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/testonlydep-1.0 ',
                                             '[ebuild  N     ] dev-libs/withtestdeppkg-1.0  USE="-test"',
                                         ]


def test_with_test_deps_n_explicitly_disables_it(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--with-test-deps", "n", "dev-libs/withtestdeppkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/withtestdeppkg-1.0  USE="-test"',
                                         ]


def test_with_test_deps_does_not_apply_beyond_a_top_level_atom(emerge_binary, fixture_env):
    """dev-libs/withtestdepconsumer RDEPENDs on dev-libs/withtestdeppkg,
    reaching it at depth 1, not depth 0 -- real depgraph.py's own
    "pkg.depth == 0" gate (portuale's own equivalent) means
    dev-libs/testonlydep must NOT be pulled in even with --with-test-deps
    given, since withtestdeppkg itself isn't the top-level atom here."""
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--with-test-deps", "dev-libs/withtestdepconsumer"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
                                             '[ebuild  N     ] dev-libs/newpkg-1.0 ',
                                             '[ebuild  N     ] dev-libs/withtestdeppkg-1.0  USE="-test"',
                                             '[ebuild  N     ] dev-libs/withtestdepconsumer-1.0 ',
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
    assert result.stdout == '[ebuild  N     ] dev-libs/withdeps-1.0 \n'


def test_nodeps_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-O is --nodeps's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pO") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pO", "dev-libs/withdeps"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == '[ebuild  N     ] dev-libs/withdeps-1.0 \n'


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
        '[ebuild  N     ] dev-libs/useflagpkg-1.0::testrepo  USE="foo -missingflag"\n\nTotal: 1 package (1 new), Size of downloads: 0 KiB\n'
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
    ]


def test_onlydeps_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-o is --onlydeps's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-po") it must behave identically to
    the long-flag invocation above. -u (--update) is bundled in too, for
    the same reason the long-flag invocation above needs --update."""
    result = _run([str(emerge_binary)], ["-pou", "dev-libs/withdeps"], fixture_env)
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
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
    of portuale's own test suite asserted here, before this real
    behavior was discovered."""
    result = _run([str(emerge_binary)], ["--pretend", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]\n'


def test_noreplace_restores_the_real_avoid_update_shortcut(emerge_binary, fixture_env):
    """The mirror case: with `selective` restored via --noreplace, the
    installed version (1.0) IS still a matched candidate (real
    `want_reinstall` no longer forces it out), so real `avoid_update`'s
    own shortcut fires normally and 2.0 is never even considered --
    matching portuale's own pre-existing behavior for every case
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
    assert with_update_alone.stdout == 'dev-libs/samepkg-1.0 is already installed; nothing to do\n'

    with_selective_cancelled = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--selective=n", "dev-libs/samepkg"],
        fixture_env,
    )
    assert with_selective_cancelled.returncode == 0
    assert with_selective_cancelled.stdout == '[ebuild   R    ] dev-libs/samepkg-1.0 \n'


def test_update_upgrades_to_the_newer_visible_version(emerge_binary, fixture_env):
    """Same fixture as above, but with --update: now a real Upgrade,
    matching real depgraph.py's own `dont_miss_updates` branch."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--update", "dev-libs/upgradepkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout == '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]\n'


def test_update_short_alias_bundled_with_pretend(emerge_binary, fixture_env):
    """-u is --update's real short alias (see lib/_emerge/main.py's
    shortmapping); bundled with -p ("-pu") it must behave identically to
    the long-flag invocation above."""
    result = _run([str(emerge_binary)], ["-pu", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 0
    assert result.stdout == '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]\n'


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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]',
        '[ebuild  N     ] dev-libs/withdeps-1.0 ',
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/deeppkg-1.0 is already installed; nothing to do',
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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/deeppkg-1.0 is already installed; nothing to do',
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
    assert bounded_one.stdout == 'dev-libs/deeppkg-1.0 is already installed; nothing to do\n'

    bounded_two = _run(
        [str(emerge_binary)],
        ["--pretend", "--noreplace", "--deep=2", "dev-libs/deeppkg"],
        fixture_env,
    )
    assert bounded_two.returncode == 0
    assert bounded_two.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        'dev-libs/deeppkg-1.0 is already installed; nothing to do',
    ]


def test_package_provided_drops_the_dep_and_warns_on_a_direct_target(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """profiles/default/package.provided lists dev-libs/providedpkg-1.0
    and dev-libs/providedpkg2-1.0 (both have ebuilds in the fixture repo).
    Real config.py:970-1027 builds pprovideddict; a dependency atom
    matching one is silently dropped from the dep walk
    (dep_check.py:1052), and a directly-requested one is not resolved and
    triggers real depgraph.py:11192-11235's `WARNING: … listed in
    package.provided:` block (to stderr, exit 0). No SetArg tracking here,
    so the ref is always `'args'`."""
    # dev-libs/needsprovided RDEPENDs providedpkg (dropped) + newpkg (New).
    dep = _run([str(emerge_binary)], ["--pretend", "dev-libs/needsprovided"], fixture_env)
    dep_py = _run(emerge_pretend_python, ["--pretend", "dev-libs/needsprovided"], fixture_env)
    assert dep.returncode == 0
    assert dep.stdout == dep_py.stdout
    assert dep.stderr == dep_py.stderr
    assert dep.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/needsprovided-1.0 ',
    ]
    assert dep.stderr == ""

    # A direct target -> no merge-list line, the singular WARNING block.
    one = _run([str(emerge_binary)], ["--pretend", "dev-libs/providedpkg"], fixture_env)
    one_py = _run(emerge_pretend_python, ["--pretend", "dev-libs/providedpkg"], fixture_env)
    assert one.returncode == 0
    assert one.stdout == "" and one.stdout == one_py.stdout
    assert one.stderr == one_py.stderr
    assert one.stderr == (
        "\nWARNING: A requested package will not be merged because it is listed in\n"
        "package.provided:\n"
        "\n"
        "  dev-libs/providedpkg pulled in by 'args'\n"
        "\n"
    )

    # Two direct targets -> the plural phrasing.
    two = _run(
        [str(emerge_binary)],
        ["--pretend", "dev-libs/providedpkg", "dev-libs/providedpkg2"],
        fixture_env,
    )
    two_py = _run(
        emerge_pretend_python,
        ["--pretend", "dev-libs/providedpkg", "dev-libs/providedpkg2"],
        fixture_env,
    )
    assert two.stderr == two_py.stderr
    assert two.stderr == (
        "\nWARNING: Requested packages will not be merged because they are listed in\n"
        "package.provided:\n"
        "\n"
        "  dev-libs/providedpkg pulled in by 'args'\n"
        "  dev-libs/providedpkg2 pulled in by 'args'\n"
        "\n"
    )

    # --color=y: `WARNING: ` is BAD (red), the atom is INFORM (darkgreen).
    R = "\x1b[39;49;00m"
    col = _run([str(emerge_binary)], ["--pretend", "--color=y", "dev-libs/providedpkg"], fixture_env)
    assert col.stderr == _run(
        emerge_pretend_python, ["--pretend", "--color=y", "dev-libs/providedpkg"], fixture_env
    ).stderr
    assert col.stderr.startswith(f"\x1b[31;01m\nWARNING: {R}A requested package")
    assert f"  \x1b[32mdev-libs/providedpkg{R} pulled in by 'args'\n" in col.stderr


def test_emptytree_reinstalls_the_whole_deep_dependency_tree(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--emptytree/-e (real create_depgraph_params.py:176-179 --
    myparams["empty"] = True; myparams["deep"] = True;
    myparams.pop("selective")): every atom in the deep dependency tree is
    (re)merged as though nothing is installed. Unlike a plain `emerge -p
    dev-libs/deeppkg` (which shows only deeppkg and never walks its
    installed RDEPEND chain), `-e` forces `deep` on AND turns each
    already-installed dependency into a bare `[ebuild   R   ]` reinstall
    (no `[oldver]`, no reason -- real portage's `attr_display.replace`
    from `vardb.cpv_exists`). deeppkg + deeppkg2 are installed ->
    Reinstall; newpkg is not -> New. Useful for byte-for-byte comparison
    against real portage and for debugging resolution."""
    args = ["--pretend", "--emptytree", "dev-libs/deeppkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild   R    ] dev-libs/deeppkg2-1.0 ',
        '[ebuild   R    ] dev-libs/deeppkg-1.0 ',
    ]

    # -e alone reinstalls; the counters line counts the reinstalls.
    v = _run([str(emerge_binary)], ["--pretend", "-v", "--emptytree", "dev-libs/deeppkg"], fixture_env)
    assert v.stdout == _run(
        emerge_pretend_python, ["--pretend", "-v", "--emptytree", "dev-libs/deeppkg"], fixture_env
    ).stdout
    assert v.stdout.splitlines()[-1] == (
        "Total: 3 packages (1 new, 2 reinstalls), Size of downloads: 0 KiB"
    )

    # `-e -u` still upgrades a dependency where a newer version exists
    # (real `emerge -e` reinstalls what you have; `-e -u` upgrades).
    eu = _run(
        [str(emerge_binary)],
        ["--pretend", "--emptytree", "--update", "dev-libs/withdeps"],
        fixture_env,
    )
    assert eu.stdout == _run(
        emerge_pretend_python,
        ["--pretend", "--emptytree", "--update", "dev-libs/withdeps"],
        fixture_env,
    ).stdout
    assert "[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]" in eu.stdout

    # -e without -p is still refused (portuale never really merges).
    assert _run([str(emerge_binary)], ["-e", "dev-libs/deeppkg"], fixture_env).returncode != 0


def test_reinstall_atoms_forces_one_deep_dependency_to_reinstall(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--reinstall-atoms ATOMS (real main.py `action: "append"` ->
    depgraph.py:363 WildcardPackageSet): an already-installed package
    matching one of the atoms is treated as if not installed, forcing a
    re-merge (real depgraph.py drops it from every inst_pkgs list). It is
    a scoped --emptytree: only the matched atom flips to `[ebuild R]`,
    everything else keeps its ordinary outcome. deeppkg RDEPENDs deeppkg2
    (both installed) RDEPENDs newpkg (New)."""
    plain = _run([str(emerge_binary)], ["--pretend", "--deep", "dev-libs/deeppkg"], fixture_env)
    # Without the flag, deeppkg2 is AlreadyInstalled -> not in the list.
    assert "deeppkg2" not in plain.stdout

    args = ["--pretend", "--deep", "--reinstall-atoms", "dev-libs/deeppkg2", "dev-libs/deeppkg"]
    rust = _run([str(emerge_binary)], args, fixture_env)
    python = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild  N     ] dev-libs/newpkg-1.0 ",
        "[ebuild   R    ] dev-libs/deeppkg2-1.0 ",
        # deeppkg itself is a directly-named installed atom -> real
        # portage re-merges it by default (only --noreplace keeps it).
        "[ebuild   R    ] dev-libs/deeppkg-1.0 ",
    ]

    # A wildcard atom is accepted (WildcardPackageSet); a repeated flag
    # and a multi-atom value both accumulate, same as --exclude.
    multi = ["--pretend", "--deep",
             "--reinstall-atoms", "dev-libs/deeppkg2 dev-libs/none",
             "--reinstall-atoms=dev-libs/deeppkg",
             "dev-libs/deeppkg"]
    r = _run([str(emerge_binary)], multi, fixture_env)
    assert r.stdout == _run(emerge_pretend_python, multi, fixture_env).stdout
    assert "[ebuild   R    ] dev-libs/deeppkg-1.0 " in r.stdout
    assert "[ebuild   R    ] dev-libs/deeppkg2-1.0 " in r.stdout

    # No value -> usage error, exit 2, matching --exclude.
    err = _run([str(emerge_binary)], ["--pretend", "dev-libs/deeppkg", "--reinstall-atoms"], fixture_env)
    assert err.returncode == 2
    assert err.stderr == _run(
        emerge_pretend_python, ["--pretend", "dev-libs/deeppkg", "--reinstall-atoms"], fixture_env
    ).stderr


def test_rebuild_if_star_rebuilds_an_installed_consumer_of_a_merged_build_dep(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--rebuild-if-unbuilt / --rebuild-if-new-rev / --rebuild-if-new-ver
    (real `_rebuild_config.trigger_rebuilds`): an installed package whose
    vdb DEPEND/BDEPEND is satisfied by a package being merged this run
    gets its own `[ebuild R]` rebuild entry -- it isn't otherwise in the
    graph. `dev-libs/rebuildconsumer` (installed) DEPENDs
    `dev-libs/rebuildtrigger` (installed 1.0, tree has 2.0)."""
    up = ["--pretend", "-u", "--rebuild-if-unbuilt", "dev-libs/rebuildtrigger"]
    rust = _run([str(emerge_binary)], up, fixture_env)
    python = _run(emerge_pretend_python, up, fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == python.stdout
    assert rust.stderr == python.stderr
    assert rust.stdout.splitlines() == [
        "[ebuild     U  ] dev-libs/rebuildtrigger-2.0 [1.0]",
        "[ebuild   R    ] dev-libs/rebuildconsumer-1.0 ",
    ]

    # --rebuild-if-new-ver vs --rebuild-if-unbuilt only diverge for a
    # same-version re-merge: rebuildnochange's best tree version == the
    # installed one.
    nv = _run([str(emerge_binary)], ["--pretend", "--rebuild-if-new-ver", "dev-libs/rebuildnochange"], fixture_env)
    assert nv.stdout == _run(
        emerge_pretend_python, ["--pretend", "--rebuild-if-new-ver", "dev-libs/rebuildnochange"], fixture_env
    ).stdout
    assert "rebuildnochangeconsumer" not in nv.stdout
    ub = _run([str(emerge_binary)], ["--pretend", "--rebuild-if-unbuilt", "dev-libs/rebuildnochange"], fixture_env)
    assert "[ebuild   R    ] dev-libs/rebuildnochangeconsumer-1.0 " in ub.stdout

    # --rebuild-exclude (parent) / --rebuild-ignore (dep) both suppress it.
    for extra in (
        ["--rebuild-exclude", "dev-libs/rebuildconsumer"],
        ["--rebuild-ignore", "dev-libs/rebuildtrigger"],
    ):
        args = ["--pretend", "-u", "--rebuild-if-unbuilt", *extra, "dev-libs/rebuildtrigger"]
        r = _run([str(emerge_binary)], args, fixture_env)
        assert r.stdout == _run(emerge_pretend_python, args, fixture_env).stdout
        assert "rebuildconsumer" not in r.stdout


def test_dynamic_deps_chooses_ebuild_vs_vdb_deps_for_an_installed_deep_dep(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--dynamic-deps (real create_depgraph_params.py, ON by default for a
    source install): an AlreadyInstalled package's --deep dependency walk
    uses its CURRENT ebuild metadata (portuale's own long-standing
    behaviour). --dynamic-deps=n uses the vdb-recorded *DEPEND snapshot.
    dev-libs/changeddepspkg's current ebuild RDEPENDs dev-libs/newpkg
    (New) but its vdb RDEPEND is dev-libs/samepkg (installed)."""
    base = ["--pretend", "-D", "--noreplace", "dev-libs/changeddepspkg"]
    dyn = _run([str(emerge_binary)], base, fixture_env)
    assert dyn.stdout == _run(emerge_pretend_python, base, fixture_env).stdout
    assert "[ebuild  N     ] dev-libs/newpkg-1.0 " in dyn.stdout

    static = _run([str(emerge_binary)], base[:3] + ["--dynamic-deps=n"] + base[3:], fixture_env)
    assert static.stdout == _run(
        emerge_pretend_python, base[:3] + ["--dynamic-deps=n"] + base[3:], fixture_env
    ).stdout
    assert "newpkg" not in static.stdout


def test_complete_graph_forces_the_deep_walk(emerge_binary, emerge_pretend_python, fixture_env):
    """--complete-graph (real create_depgraph_params.py:169-175 +
    depgraph.py::_complete_graph 8668-8670): "completely account for all
    known dependencies" -> myparams["deep"] = True. In this --pretend
    portuale that forced deep walk is the whole observable delta (see
    resolve_pretend_graph's `complete` param). deeppkg (installed) ->
    deeppkg2 (installed) -> newpkg (New): plain `emerge -p deeppkg` never
    walks deeppkg's deps; --complete-graph does, byte-identical to -D."""
    plain = _run([str(emerge_binary)], ["--pretend", "dev-libs/deeppkg"], fixture_env)
    assert "newpkg" not in plain.stdout

    cg = _run([str(emerge_binary)], ["--pretend", "--complete-graph", "dev-libs/deeppkg"], fixture_env)
    deep = _run([str(emerge_binary)], ["--pretend", "-D", "dev-libs/deeppkg"], fixture_env)
    assert cg.stdout == deep.stdout
    assert cg.stdout == _run(
        emerge_pretend_python, ["--pretend", "--complete-graph", "dev-libs/deeppkg"], fixture_env
    ).stdout
    assert "[ebuild  N     ] dev-libs/newpkg-1.0 " in cg.stdout

    off = _run(
        [str(emerge_binary)], ["--pretend", "--complete-graph=n", "dev-libs/deeppkg"], fixture_env
    )
    assert off.stdout == plain.stdout


def test_complete_graph_if_new_ver_auto_enables_on_an_upgrade(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real depgraph.py::_complete_graph 8581-8648: --complete-graph-if-new-ver
    defaults ON, so complete mode (the forced deep walk) auto-enables when
    a run would change an installed package's version -- even without
    --complete-graph. completegraphpkg 1.0 installed, 2.0 in the tree,
    RDEPEND dev-libs/deeppkg -> deeppkg2 -> newpkg (New). `emerge -pu
    completegraphpkg` is an Upgrade, so newpkg is pulled in; =n opts out."""
    base = ["--pretend", "--update", "dev-libs/completegraphpkg"]
    auto = _run([str(emerge_binary)], base, fixture_env)
    assert auto.stdout == _run(emerge_pretend_python, base, fixture_env).stdout
    assert "[ebuild     U  ] dev-libs/completegraphpkg-2.0 [1.0]" in auto.stdout
    assert "[ebuild  N     ] dev-libs/newpkg-1.0 " in auto.stdout

    off = _run([str(emerge_binary)], base[:2] + ["--complete-graph-if-new-ver=n"] + base[2:], fixture_env)
    assert off.stdout == _run(
        emerge_pretend_python, base[:2] + ["--complete-graph-if-new-ver=n"] + base[2:], fixture_env
    ).stdout
    assert "newpkg" not in off.stdout
    assert "[ebuild     U  ] dev-libs/completegraphpkg-2.0 [1.0]" in off.stdout


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
    assert result.stdout == '[ebuild     U  ] dev-libs/upgradepkg-2.0 [1.0]\n'


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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] dev-libs/withdeps-1.0 ',
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
    portuale deliberately doesn't support bundling it -- a specific message
    instead of a misleading generic one."""
    result = _run([str(emerge_binary)], ["-pX", "dev-libs/upgradepkg"], fixture_env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert result.stderr.strip() == (
        "emerge: -X (--exclude) requires an argument and can't be bundled with "
        "other short flags in portuale"
    )


def test_json_is_not_a_real_emerge_option(emerge_binary, fixture_env):
    """--json is a portuale-specific addition (real portage has no
    structured-output mode for --pretend at all) -- pinned in full since
    it's portuale's own content, not derived from any real emerge
    output, unlike every other flag's own contract test."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout == (
        (
        '{"entries":[{"category":"dev-libs","package":"newpkg","merge_order":0,"outcome":"new","version":"1.0","new_slot":false,"interactive":false,"fetch_restrict":false,"fetch_restrict_satisfied":false,"slot":"0","source":"ebuild","provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},"requested":true,"required_by":[],"builds_against_running_root":null,"blockers":[]}],"slot_conflicts":[],"changed_deps_report":[],"autounmask_keyword_changes":[],"autounmask_use_changes":[],"autounmask_license_changes":[],"autounmask_mask_changes":[],"abi_rebuilds":[]}\n'
        )
    )


def test_json_upgrade_includes_from_version(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)],
        ["--pretend", "--update", "--json", "dev-libs/upgradepkg"],
        fixture_env,
    )
    assert result.returncode == 0
    assert result.stdout == (
        (
        '{"entries":[{"category":"dev-libs","package":"upgradepkg","merge_order":0,"outcome":"upgrade","version":"2.0","from_version":"1.0","interactive":false,"fetch_restrict":false,"fetch_restrict_satisfied":false,"slot":"0","source":"ebuild","provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},"requested":true,"required_by":[],"builds_against_running_root":null,"blockers":[]}],"slot_conflicts":[],"changed_deps_report":[],"autounmask_keyword_changes":[],"autounmask_use_changes":[],"autounmask_license_changes":[],"autounmask_mask_changes":[],"abi_rebuilds":[]}\n'
        )
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
        [str(emerge_binary)],
        ["--pretend", "--json", "dev-libs/slotconflictunsolvable"],
        fixture_env,
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    assert len(payload["slot_conflicts"]) == 1
    conflict = payload["slot_conflicts"][0]
    assert conflict["category"] == "dev-libs"
    assert conflict["package"] == "slotconflicttarget"


def test_json_solvable_slot_conflict_is_reconciled_to_no_conflict(emerge_binary, fixture_env):
    result = _run(
        [str(emerge_binary)], ["--pretend", "--json", "dev-libs/slotconflictparent"], fixture_env
    )
    assert result.returncode == 0
    payload = json.loads(result.stdout)
    assert payload["slot_conflicts"] == []
    target = next(
        e for e in payload["entries"] if e["package"] == "slotconflicttarget"
    )
    assert target["outcome"] == "new"
    assert target["version"] == "1.0"


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
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] virtual/texteditor-0 ',
    ]


def test_virtual_is_resolved_as_a_dependency(emerge_binary, fixture_env):
    """dev-libs/virtualconsumerpkg RDEPENDs on virtual/texteditor --
    proving a virtual/ atom extracted from another package's own
    DEPEND/RDEPEND resolves identically to the top-level case above,
    with no virtual-specific code path anywhere in portuale."""
    result = _run(
        [str(emerge_binary)], ["--pretend", "dev-libs/virtualconsumerpkg"], fixture_env
    )
    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        '[ebuild  N     ] dev-libs/newpkg-1.0 ',
        '[ebuild  N     ] virtual/texteditor-0 ',
        '[ebuild  N     ] dev-libs/virtualconsumerpkg-1.0 ',
    ]


def test_real_option_not_implemented_message_names_the_option(emerge_binary, fixture_env):
    """--accept-properties is a real emerge option (see lib/_emerge/main.py's
    argument_options) portuale doesn't implement -- the error must
    name it specifically and say "option", distinct from both a
    genuinely unrecognized flag and an unimplemented action."""
    result = _run([str(emerge_binary)], ["--accept-properties", "dev-libs/newpkg"], fixture_env)
    assert result.returncode == 2
    assert result.stdout == ""
    assert (
        result.stderr.strip()
        == 'emerge: option "--accept-properties" is a real emerge option, but is '
        'not yet implemented in portuale -- run "emerge --help" for the '
        "options and actions that are."
    )


def test_real_option_inline_equals_form_is_still_recognized(emerge_binary, fixture_env):
    """--accept-properties=* (the "--opt=value" form argparse also accepts)
    must resolve to the same canonical "--accept-properties" option as
    "--accept-properties *" would, not be treated as one unrecognized token."""
    result = _run(
        [str(emerge_binary)], ["--accept-properties=*", "--pretend", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 2
    assert (
        result.stderr.strip()
        == 'emerge: option "--accept-properties" is a real emerge option, but is '
        'not yet implemented in portuale -- run "emerge --help" for the '
        "options and actions that are."
    )


def test_sync_points_at_emaint(emerge_binary, emerge_pretend_python, fixture_env):
    """`emerge --sync` is a permanent non-goal in portuale -- repo syncing
    belongs to `emaint sync` (real portage's own long-standing split).
    The exact message, with or without --pretend, exit 1, byte-identical
    Rust/Python."""
    for args in (["--sync"], ["--pretend", "--sync"]):
        rust = _run([str(emerge_binary)], args, fixture_env)
        py = _run(emerge_pretend_python, args, fixture_env)
        assert rust.returncode == 1
        assert rust.stdout == ""
        assert rust.stderr.strip() == "Functionality has moved to `emaint sync`."
        assert rust.stderr == py.stderr
        assert rust.returncode == py.returncode


def test_real_action_not_implemented_message_says_action_not_option(emerge_binary, fixture_env):
    """--moo is a real emerge action (see main.py's actions frozenset),
    not an option -- the error must say "action". (--search/--depclean/
    --unmerge/--regen used to be the example here; all implemented now.
    --sync has its own dedicated "moved to `emaint sync`" message.)"""
    result = _run([str(emerge_binary)], ["--moo"], fixture_env)
    assert result.returncode == 2
    expected = (
        'emerge: action "--moo" is a real emerge action, but is not yet '
        'implemented in portuale -- run "emerge --help" for the options '
        "and actions that are."
    )
    assert result.stderr.strip() == expected


def test_list_sets_prints_the_defined_set_names(emerge_binary, emerge_pretend_python, fixture_env):
    """emerge --list-sets (real _emerge/actions.py:3839): every defined
    package-set name, sorted, one per line -- the cnf/sets/portage.conf
    built-ins plus the fixture's own user set files. Rust == Python."""
    rust = _run([str(emerge_binary)], ["--list-sets"], fixture_env)
    py = _run(emerge_pretend_python, ["--list-sets"], fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    lines = rust.stdout.splitlines()
    assert lines == sorted(lines)
    assert "world" in lines and "system" in lines and "selected" in lines
    # user sets from fixtures/etc/portage/sets/
    assert "dualslotset" in lines
    # the [usersets] multiset generator section is NOT a set name
    assert "usersets" not in lines


@pytest.mark.parametrize(
    "args",
    [
        ["--search", "newpkg"],
        ["-s", "useflagpkg"],
        ["-s", "nomatchanywhere"],
        ["-sv", "useflagpkg"],
        ["--searchdesc", "fixture"],
        ["-S", "overlay"],
        ["-s", "dev-libs/newpkg"],
        # --fuzzy-search (default on): a misspelling still resolves.
        ["-s", "useflgpkg"],
        ["-s", "newpgk"],
        ["-s", "diamnod"],
        ["-s", "dev-libz/newpkg"],  # category half scored independently
        ["-s", "--fuzzy-search=n", "useflgpkg"],
        ["-s", "--search-similarity=100", "useflgpkg"],
        ["-s", "--search-similarity=40", "newpgk"],
        # --regex-search-auto (default on) + explicit % force.
        ["-s", "%^dev-libs/newpkg$"],
        ["-s", "new.+pkg"],
        ["-s", "--regex-search-auto=n", "a.+b"],
        ["-sv", "useflgpkg"],
        # usage errors on --search-similarity.
        ["-s", "--search-similarity=xyz", "foo"],
        ["-s", "--search-similarity=150", "foo"],
    ],
)
def test_search_matches_rust_and_python(emerge_binary, emerge_pretend_python, fixture_env, args):
    """emerge --search/-s (--searchdesc/-S also matches DESCRIPTION;
    --fuzzy-search / --regex-search-auto / --search-similarity modifiers):
    real action_search / search.output() shape. Rust == Python (stdout,
    stderr, and exit code)."""
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.returncode == py.returncode
    assert rust.stdout == py.stdout
    assert rust.stderr == py.stderr


def test_fuzzy_and_regex_search_change_the_result_set(
    emerge_binary, fixture_env
):
    """--fuzzy-search (default on) and --regex-search-auto (default on)
    are not no-ops: a misspelled key still finds the package, and turning
    fuzzy off drops it; a %-forced regex key matches by pattern."""
    def apps(args):
        out = _run([str(emerge_binary)], args, fixture_env).stdout
        # "[ Applications found : N ]"
        return int(out.rsplit(":", 1)[1].split("]")[0].strip())

    assert apps(["-s", "useflgpkg"]) >= 1  # fuzzy hit
    assert apps(["-s", "--fuzzy-search=n", "useflgpkg"]) == 0  # exact only
    assert apps(["-s", "--search-similarity=100", "useflgpkg"]) == 0
    assert apps(["-s", "%^dev-libs/newpkg$"]) == 1  # regex, anchored


def test_misspell_suggestions_for_a_missing_package_name(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """--misspell-suggestions (real depgraph.py:7037 + _similar_name_search,
    default on): a top-level `cat/pkg` that doesn't exist gets
    `difflib.get_close_matches` suggestions after the "there are no
    ebuilds to satisfy" line. `--misspell-suggestions=n` drops them; an
    existing-but-masked cp gets no name suggestions (real `not
    cp_exists`)."""
    r = _run([str(emerge_binary)], ["--pretend", "dev-libs/newpgk"], fixture_env)
    p = _run(emerge_pretend_python, ["--pretend", "dev-libs/newpgk"], fixture_env)
    assert r.returncode == 1
    assert r.stderr == p.stderr
    assert 'there are no ebuilds to satisfy "dev-libs/newpgk".' in r.stderr
    assert "emerge: searching for similar names..." in r.stderr
    assert "dev-libs/newpkg" in r.stderr  # the close match

    off = _run(
        [str(emerge_binary)],
        ["--pretend", "--misspell-suggestions=n", "dev-libs/newpgk"],
        fixture_env,
    )
    assert off.stderr == _run(
        emerge_pretend_python,
        ["--pretend", "--misspell-suggestions=n", "dev-libs/newpgk"],
        fixture_env,
    ).stderr
    assert "searching for similar names" not in off.stderr

    # dev-libs/autounmaskkeywordpkg exists (keyword-masked) -> the
    # autounmask note path, never the name-suggestion path.
    masked = _run([str(emerge_binary)], ["--pretend", "dev-libs/autounmaskkeywordpkg"], fixture_env)
    assert "searching for similar names" not in masked.stderr


def test_bare_command_line_name_is_category_qualified(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `dep_expand()` / `cpv_expand()` (`lib/portage/dbapi/`): a
    command-line target with no category is qualified against the repo
    tree. `dev-libs/newpkg` is the only `newpkg` anywhere, so
    `emerge newpkg` resolves it. `virtprefpkg` exists as both
    `dev-libs/virtprefpkg` and `virtual/virtprefpkg` -> the non-virtual
    wins silently (real "assume that the non-virtual is desired"). Rust
    == Python throughout."""
    for target, expected in (
        ("newpkg", "[ebuild  N     ] dev-libs/newpkg-1.0"),
        ("virtprefpkg", "[ebuild  N     ] dev-libs/virtprefpkg-1.0"),
    ):
        r = _run([str(emerge_binary)], ["--pretend", target], fixture_env)
        p = _run(emerge_pretend_python, ["--pretend", target], fixture_env)
        assert r.returncode == 0, r.stderr
        assert r.stdout == p.stdout
        assert r.stdout.splitlines()[0].rstrip() == expected


def test_bare_command_line_name_with_version_or_slot_is_category_qualified(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real `dep_expand` (lib/portage/dbapi/dep_expand.py): a no-category
    target that carries an operator, a bare version, or a slot is still
    qualified -- `null/` is inserted before the first word char, the
    result is parsed (retrying with a leading `=` for the missing-`=`
    backward-compat shape), the package name is pulled back out and
    `cpv_expand`ed, and the category is spliced into the original string.
    `dev-libs/newpkg` is the only `newpkg` anywhere. Rust == Python."""
    for target, first_line, exit_code in (
        ("newpkg-1.0", "[ebuild  N     ] dev-libs/newpkg-1.0", 0),
        (">=newpkg-1.0", "[ebuild  N     ] dev-libs/newpkg-1.0", 0),
        ("newpkg:0", "[ebuild  N     ] dev-libs/newpkg-1.0", 0),
    ):
        r = _run([str(emerge_binary)], ["--pretend", target], fixture_env)
        p = _run(emerge_pretend_python, ["--pretend", target], fixture_env)
        assert r.returncode == exit_code, r.stderr
        assert r.stdout == p.stdout and r.stderr == p.stderr
        assert r.stdout.splitlines()[0].rstrip() == first_line

    # a bare version with no matching ebuild: the message quotes the
    # dep_expand'd atom (real cpv_expand splices the category, then
    # resolution fails)
    r = _run([str(emerge_binary)], ["--pretend", "newpkg-9.9"], fixture_env)
    p = _run(emerge_pretend_python, ["--pretend", "newpkg-9.9"], fixture_env)
    assert r.returncode == 1 and r.stderr == p.stderr
    assert r.stderr.strip() == 'emerge: there are no ebuilds to satisfy "=dev-libs/newpkg-9.9".'

    # ambiguous survives version stripping
    r = _run([str(emerge_binary)], ["--pretend", "ambigpkg-1.0"], fixture_env)
    p = _run(emerge_pretend_python, ["--pretend", "ambigpkg-1.0"], fixture_env)
    assert r.returncode == 1 and r.stdout == p.stdout and r.stderr == p.stderr
    assert '!!! The short ebuild name "ambigpkg-1.0" is ambiguous.' in r.stderr
    assert r.stdout.split() == ["app-misc/ambigpkg", "dev-libs/ambigpkg"]


def test_bare_name_ambiguous_across_categories_is_rejected(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """`ambigpkg` exists as both `app-misc/ambigpkg` and
    `dev-libs/ambigpkg` (both non-virtual) -> real
    `ambiguous_package_name` (its `--quiet` form: the two `!!!` lines +
    the sorted fully-qualified list, exit 1). Rust == Python."""
    r = _run([str(emerge_binary)], ["--pretend", "ambigpkg"], fixture_env)
    p = _run(emerge_pretend_python, ["--pretend", "ambigpkg"], fixture_env)
    assert r.returncode == 1
    assert r.stdout == p.stdout
    assert r.stderr == p.stderr
    assert '!!! The short ebuild name "ambigpkg" is ambiguous.' in r.stderr
    assert "!!! one of the following fully-qualified ebuild names instead:" in r.stderr
    assert r.stdout.split() == ["app-misc/ambigpkg", "dev-libs/ambigpkg"]


def test_bare_name_with_no_match_reports_no_ebuilds(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """A bare name matching no package anywhere -> `emerge: there are no
    ebuilds to satisfy "<name>".`, exit 1 (real `cpv_expand` returns
    `null/<name>` and resolution then fails; portuale short-circuits
    with the message). Rust == Python."""
    r = _run([str(emerge_binary)], ["--pretend", "nosuchpkgname"], fixture_env)
    p = _run(emerge_pretend_python, ["--pretend", "nosuchpkgname"], fixture_env)
    assert r.returncode == 1
    assert r.stderr == p.stderr
    assert r.stderr.strip() == 'emerge: there are no ebuilds to satisfy "nosuchpkgname".'


def test_check_news_counts_unread_relevant_items(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """emerge --check-news (real actions.py:3844 -> count_unread_news):
    the fixture testrepo has three GLEP 42 news items -- one unrestricted,
    one Display-If-Installed: dev-libs/samepkg (in the vdb), one
    Display-If-Installed on an uninstalled package. Only the first two are
    relevant, so the count is 2. Rust == Python."""
    rust = _run([str(emerge_binary)], ["--check-news"], fixture_env)
    py = _run(emerge_pretend_python, ["--check-news"], fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert "2 news items need reading for repository 'testrepo'." in rust.stdout
    assert "eselect news read" in rust.stdout


@pytest.mark.parametrize(
    "args",
    [
        ["-p", "--clean", "dev-libs/unmergepkg"],
        ["-p", "--clean", "dev-libs/dualslotpkg"],
        ["-p", "--clean"],
        ["-p", "--rage-clean", "dev-libs/unmergepkg"],
        ["-p", "--rage-clean", "dev-libs/nope"],
        ["-p", "--rage-clean"],
    ],
)
def test_clean_and_rage_clean_pretend_match_rust_and_python(
    emerge_binary, emerge_pretend_python, fixture_env, args
):
    """emerge -p --clean / -p --rage-clean (real action_uninstall ->
    unmerge). --clean keeps only the newest version per slot
    (dev-libs/unmergepkg 1.0+2.0 in slot 0 -> remove 1.0;
    dev-libs/dualslotpkg 1.0/slot1 + 2.0/slot2 -> nothing). --rage-clean
    removes every matched version (a fast --unmerge). Rust == Python."""
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.stdout == py.stdout
    assert rust.returncode == py.returncode


def test_color_map_overrides_the_ansi_codes(
    emerge_binary, emerge_pretend_python, fixture_env, fixtures_root, tmp_path
):
    """Real /etc/portage/color.map (output.py::_parse_color_map): a
    `KEY = VALUE` line overrides the ANSI code for a `_styles` key or a
    `codes` colour-name (VALUE = a raw code like `31m`, or a
    space-separated list of colour-names). Here `GOOD = darkred` recolours
    `emerge --search`'s `*` marker from green to darkred, and
    `PKG_MERGE = 34;01m` recolours a `[ebuild N ]` cpv. Rust == Python."""
    cfg = tmp_path / "cfg"
    shutil.copytree(fixtures_root / "etc", cfg / "etc", symlinks=True)
    # The fixture make.profile is a relative symlink into ../../repo; make
    # it absolute so it still resolves from the tmp config root.
    prof = cfg / "etc" / "portage" / "make.profile"
    prof.unlink()
    prof.symlink_to(fixtures_root / "repo" / "profiles" / "default")
    # repos.conf uses locations relative to the config root; rewrite them
    # to absolute paths so the repos still resolve from the tmp tree.
    rc = cfg / "etc" / "portage" / "repos.conf" / "repos.conf"
    rc.write_text(
        "\n".join(
            (
                line
                if not line.strip().startswith("location")
                else "location = %s" % (fixtures_root / line.split("=", 1)[1].strip())
            )
            for line in rc.read_text().splitlines()
        )
        + "\n"
    )
    (cfg / "etc" / "portage" / "color.map").write_text(
        "GOOD = darkred\n"
        "# a comment\n"
        "PKG_MERGE = 34;01m\n"
        'BAD = "red bold"\n'
    )
    env = dict(fixture_env)
    env["PORTAGE_CONFIGROOT"] = str(cfg)

    for args in (["--color=y", "-s", "newpkg"], ["-pv", "--color=y", "dev-libs/newpkg"]):
        rust = _run([str(emerge_binary)], args, env)
        py = _run(emerge_pretend_python, args, env)
        assert rust.stdout == py.stdout, args
        assert rust.returncode == py.returncode

    # `*` is GOOD -> darkred (\x1b[31m), not the default green (\x1b[32;01m).
    search = _run([str(emerge_binary)], ["--color=y", "-s", "newpkg"], env)
    assert "\x1b[31m*\x1b[39;49;00m" in search.stdout
    # and without the color.map it is green.
    plain = _run([str(emerge_binary)], ["--color=y", "-s", "newpkg"], fixture_env)
    assert "\x1b[32;01m*\x1b[39;49;00m" in plain.stdout


@pytest.mark.parametrize(
    "args",
    [
        ["-pq", "dev-libs/useflagpkg"],
        ["-pvq", "dev-libs/useflagpkg"],
        ["-pq", "--tree", "dev-libs/useflagpkg"],
        ["-q", "-s", "useflagpkg"],
        ["-q", "--check-news"],
    ],
)
def test_quiet_verbosity_level_1_matches_rust_and_python(
    emerge_binary, emerge_pretend_python, fixture_env, args
):
    """emerge --quiet/-q (real _DisplayConfig verbosity 1): the mask
    column disappears from the [ebuild ...] bracket, the USE="..." line
    is suppressed (unless -v is also given), the ::repo cpv decoration
    and the Total: line never show, and --search drops its verbose
    block. Rust == Python."""
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.stdout == py.stdout, args
    assert rust.returncode == py.returncode


def test_quiet_drops_the_mask_column_and_the_use_line(emerge_binary, fixture_env):
    """The concrete -pq shape: a merge line's fixed-width attr field is
    6 columns (not the default 7 -- real include_mask_str() is
    verbosity > 1), and no USE="..." suffix. -pvq keeps the USE line
    (print_use_string = verbosity != 1 or --verbose) but still drops the
    mask column and the Total: line."""
    pq = _run([str(emerge_binary)], ["-pq", "dev-libs/useflagpkg"], fixture_env)
    assert "[ebuild  N    ] dev-libs/useflagpkg-1.0 \n" in pq.stdout
    assert "USE=" not in pq.stdout
    assert "Total:" not in pq.stdout

    pvq = _run([str(emerge_binary)], ["-pvq", "dev-libs/useflagpkg"], fixture_env)
    assert '[ebuild  N    ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"\n' in pvq.stdout
    assert "::testrepo" not in pvq.stdout
    assert "Total:" not in pvq.stdout

    # plain -p keeps the 7-column field.
    p = _run([str(emerge_binary)], ["-p", "dev-libs/useflagpkg"], fixture_env)
    assert "[ebuild  N     ] dev-libs/useflagpkg-1.0 " in p.stdout


@pytest.mark.parametrize(
    "args",
    [
        # A `move dev-libs/oldmovepkg dev-libs/newmovepkg` in the fixture
        # repo's profiles/updates/2Q-2024, with the vdb still holding
        # dev-libs/oldmovepkg-1.0 under its pre-move dir.
        ["-pv", "dev-libs/newmovepkg"],
        ["-pv", "dev-libs/oldmovepkg"],
        ["-pv", "--deep", "dev-libs/movedepconsumer"],
        ["-pv", "dev-libs/slotmovepkg"],
        ["-p", "--tree", "--deep", "dev-libs/movedepconsumer"],
        # --package-moves=n: no move applied anywhere.
        ["-pv", "--package-moves=n", "dev-libs/newmovepkg"],
        ["-pv", "--package-moves=n", "dev-libs/oldmovepkg"],
        ["-pv", "--package-moves=n", "dev-libs/slotmovepkg"],
        ["-pv", "--package-moves=n", "--deep", "dev-libs/movedepconsumer"],
        ["-pv", "--package-moves", "y", "dev-libs/newmovepkg"],
    ],
)
def test_profiles_updates_package_moves_match_rust_and_python(
    emerge_binary, emerge_pretend_python, fixture_env, args
):
    """Real profiles/updates/ package moves (portage.update /
    _do_global_updates): `move`/`slotmove` directives, applied at read
    time (portuale never syncs), rewrite command-line atoms, `*DEPEND`
    strings and an installed package's identity. Rust == Python."""
    rust = _run([str(emerge_binary)], args, fixture_env)
    py = _run(emerge_pretend_python, args, fixture_env)
    assert rust.stdout == py.stdout, args
    assert rust.returncode == py.returncode


def test_profiles_updates_move_makes_the_renamed_package_already_installed(
    emerge_binary, fixture_env
):
    """`move dev-libs/oldmovepkg dev-libs/newmovepkg` + vdb
    dev-libs/oldmovepkg-1.0 => `emerge -p dev-libs/newmovepkg` resolves
    the *installed* package (a bare `R`), not a fresh `N`. The
    command-line atom `dev-libs/oldmovepkg` is itself rewritten too.
    `slotmove dev-libs/slotmovepkg 0 1` makes the SLOT-0 vdb entry read
    as slot 1, matching the ebuild's SLOT=1."""
    for atom in ("dev-libs/newmovepkg", "dev-libs/oldmovepkg"):
        r = _run([str(emerge_binary)], ["-p", atom], fixture_env)
        assert "[ebuild   R    ] dev-libs/newmovepkg-1.0" in r.stdout, atom
        assert "dev-libs/oldmovepkg" not in r.stdout, atom

    sm = _run([str(emerge_binary)], ["-pv", "dev-libs/slotmovepkg"], fixture_env)
    assert "[ebuild   R    ] dev-libs/slotmovepkg-1.0:1::testrepo" in sm.stdout


def test_package_moves_n_disables_profiles_updates(emerge_binary, emerge_pretend_python, fixture_env):
    """--package-moves (real y_or_n, default y): --package-moves=n turns
    every profiles/updates/ move/slotmove into a no-op. `move
    dev-libs/oldmovepkg dev-libs/newmovepkg` + vdb dev-libs/oldmovepkg-1.0:
    with the move applied (default) `emerge -p dev-libs/newmovepkg` is a
    bare `R` of the installed package; with `=n` there's no installed
    match, so it's a fresh `N`, and the pre-move name has no ebuild at
    all."""
    default = _run([str(emerge_binary)], ["-p", "dev-libs/newmovepkg"], fixture_env)
    assert "[ebuild   R    ] dev-libs/newmovepkg-1.0 " in default.stdout

    off = _run([str(emerge_binary)], ["-p", "--package-moves=n", "dev-libs/newmovepkg"], fixture_env)
    assert off.stdout == _run(
        emerge_pretend_python, ["-p", "--package-moves=n", "dev-libs/newmovepkg"], fixture_env
    ).stdout
    assert "[ebuild  N     ] dev-libs/newmovepkg-1.0 " in off.stdout

    # The pre-move name has only a vdb entry, no ebuild -> unsatisfiable.
    old = _run([str(emerge_binary)], ["-p", "--package-moves=n", "dev-libs/oldmovepkg"], fixture_env)
    assert old.returncode == 1
    assert 'there are no ebuilds to satisfy "dev-libs/oldmovepkg".' in old.stderr


def test_info_prints_the_deterministic_config_block(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """emerge --info (real action_info), narrowed to its deterministic
    config/repository block: the `Repositories:` list, `Binary
    Repositories:`, `Installed sets:`, the sorted VAR="value" dump, the
    `Unset:` line. The host-state half of real --info (Portage version
    header, uname/mem, tool version probes, info_pkgs, timestamps) is a
    documented cut. Rust == Python."""
    rust = _run([str(emerge_binary)], ["--info"], fixture_env)
    py = _run(emerge_pretend_python, ["--info"], fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.startswith("Repositories:\n")
    assert "\ntestrepo\n    location: " in rust.stdout
    assert "\nBinary Repositories:\n" in rust.stdout
    assert '\nACCEPT_KEYWORDS="amd64"\n' in rust.stdout
    assert "\nUnset:  " in rust.stdout


def test_info_atom_that_does_not_exist_errors_with_misspell_suggestions(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real action_info's `myfiles` loop: a target whose cat/pkg has no
    ebuild anywhere aborts before the config block with `emerge: there
    are no ebuilds to satisfy "<atom>".` + `--misspell-suggestions`,
    exit 1. Rust == Python."""
    rust = _run([str(emerge_binary)], ["--info", "dev-libs/newpgk"], fixture_env)
    py = _run(emerge_pretend_python, ["--info", "dev-libs/newpgk"], fixture_env)
    assert rust.returncode == 1
    assert py.returncode == 1
    assert rust.stdout == "" == py.stdout
    assert rust.stderr == py.stderr
    assert 'there are no ebuilds to satisfy "dev-libs/newpgk"' in rust.stderr
    assert "emerge: Maybe you meant" in rust.stderr


def test_info_atom_prints_package_settings_for_a_pkg_info_package(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """dev-libs/pkginfopkg's ebuild defines pkg_info() (DEFINED_PHASES=
    info), so real action_info appends the `Package Settings` section
    with a `<cpv>::<repo> would be built with the following:` + USE line.
    An ordinary package (dev-libs/newpkg, no pkg_info) gets no such
    block. Rust == Python."""
    rust = _run([str(emerge_binary)], ["--info", "dev-libs/pkginfopkg"], fixture_env)
    py = _run(emerge_pretend_python, ["--info", "dev-libs/pkginfopkg"], fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.endswith(
        "=================================================================\n"
        "                        Package Settings\n"
        "=================================================================\n"
        "\n"
        "\n"
        "dev-libs/pkginfopkg-1.0::testrepo would be built with the following:\n"
        'USE="alpha -beta"\n'
        "\n"
        "\n"
    )

    plain = _run([str(emerge_binary)], ["--info", "dev-libs/newpkg"], fixture_env)
    assert "Package Settings" not in plain.stdout
    assert plain.stdout == _run(
        emerge_pretend_python, ["--info", "dev-libs/newpkg"], fixture_env
    ).stdout


def test_info_atom_prints_the_installed_package_block(
    emerge_binary, emerge_pretend_python, fixture_env
):
    """Real action_info checks the vdb first: an installed match
    short-circuits the ebuild lookup and prints `<cpv>::<repo> was built
    with the following:` + the vdb USE line + the `mydesiredvars`
    (CHOST/CFLAGS/CXXFLAGS/FEATURES/LDFLAGS) whose stored value differs
    from the current config, then an `Unset:` line for the ones with no
    stored value. `dev-libs/infoinstpkg` is installed with
    IUSE="alpha beta" USE="alpha", CFLAGS/CHOST recorded, the make.conf
    setting neither. Rust == Python."""
    rust = _run([str(emerge_binary)], ["--info", "dev-libs/infoinstpkg"], fixture_env)
    py = _run(emerge_pretend_python, ["--info", "dev-libs/infoinstpkg"], fixture_env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.endswith(
        "dev-libs/infoinstpkg-1.0::testrepo was built with the following:\n"
        'USE="alpha -beta"\n'
        'CHOST="x86_64-pc-linux-gnu"\n'
        'CFLAGS="-O2 -march=native"\n'
        "Unset: CXXFLAGS, FEATURES, LDFLAGS\n"
        "\n"
        "\n"
    )


def test_check_news_reports_none_when_all_items_are_read(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """A news item id listed in
    <eroot>/var/lib/gentoo/news/news-<repo>.read (what `eselect news
    read` writes) is not counted. With all three fixture items marked
    read, `--check-news` prints ` * No news items were found.`"""
    read_dir = tmp_path / "var" / "lib" / "gentoo" / "news"
    read_dir.mkdir(parents=True)
    (read_dir / "news-testrepo.read").write_text(
        "2026-09-01-portuale-general\n"
        "2026-09-02-portuale-samepkg\n"
        "2026-09-03-portuale-irrelevant\n"
    )
    # ROOT at tmp (for the .read file + an empty vdb) but CONFIGROOT still
    # the fixtures (for repos.conf / the news items themselves).
    env = dict(fixture_env)
    env["ROOT"] = str(tmp_path)
    rust = _run([str(emerge_binary)], ["--check-news"], env)
    py = _run(emerge_pretend_python, ["--check-news"], env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.strip() == "* No news items were found."


def test_check_news_skip_file_excludes_items_like_the_read_file(
    emerge_binary, emerge_pretend_python, fixture_env, tmp_path
):
    """An id in <eroot>/var/lib/gentoo/news/news-<repo>.skip (real
    NewsManager.updateItems' permanent per-item skip list) is not
    counted, exactly like a `.read` id. With ROOT at an empty tmp tree
    only 2026-09-01-portuale-general (unrestricted) is relevant; a `.skip`
    listing it -- and no `.read` at all -- drops the count to 0."""
    news_dir = tmp_path / "var" / "lib" / "gentoo" / "news"
    news_dir.mkdir(parents=True)
    (news_dir / "news-testrepo.skip").write_text("2026-09-01-portuale-general\n")
    env = dict(fixture_env)
    env["ROOT"] = str(tmp_path)
    rust = _run([str(emerge_binary)], ["--check-news"], env)
    py = _run(emerge_pretend_python, ["--check-news"], env)
    assert rust.returncode == 0
    assert rust.stdout == py.stdout
    assert rust.stdout.strip() == "* No news items were found."


def test_genuinely_unrecognized_option_gets_a_distinct_message(emerge_binary, fixture_env):
    """A flag that isn't in real emerge's own option surface at all must
    be reported differently from a real-but-unimplemented one, so users
    can tell a typo apart from a portuale scope gap."""
    result = _run(
        [str(emerge_binary)], ["--totally-fake-option", "dev-libs/newpkg"], fixture_env
    )
    assert result.returncode == 2
    assert result.stderr.strip() == 'emerge: unrecognized option "--totally-fake-option"'
