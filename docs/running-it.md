# Running it

> Runnable, live-verified examples for every shipped slice, moved out of the
> root `README.md` verbatim. Build the binaries first (see the root
> [`README.md`](../README.md) quickstart).

---

Build both Rust binaries:

```sh
cd rust && cargo build --release
```

Try the harnesses directly:

```sh
# Python
python3 python/versions_harness.py vercmp 1.0-r1 1.0

# Rust
rust/target/release/versions-harness vercmp 1.0-r1 1.0

# batch mode (benchmark-oriented: many ops, one process)
printf 'vercmp 1.0 1.0\nververify 1.0_pre2\n' | rust/target/release/versions-harness batch
```

Try the atom-matching harness:

```sh
# Python
python3 python/atom_harness.py parse ">=dev-libs/foo-1.2.3-r1:2"

# Rust
rust/target/release/atom-harness parse ">=dev-libs/foo-1.2.3-r1:2"

# match_from_list-equivalent: prints the matching candidates, comma-joined
rust/target/release/atom-harness match ">=dev-libs/foo-1.2.3" \
    dev-libs/foo-1.0 dev-libs/foo-2.0

# slot operators: ":=" (no explicit slot) matches regardless of slot,
# ":slot=" filters to that slot exactly like a plain ":slot" atom would
rust/target/release/atom-harness match "dev-libs/foo:=" \
    dev-libs/foo-1.0:0 dev-libs/foo-2.0:1
# dev-libs/foo-1.0:0,dev-libs/foo-2.0:1
rust/target/release/atom-harness match "dev-libs/foo:1=" \
    dev-libs/foo-1.0:0 dev-libs/foo-2.0:1
# dev-libs/foo-2.0:1

# USE deps: parsed, but never enforced by matching -- "[bar]" and
# "[-bar]" return the identical match set, same as real match_from_list
# already does for these same plain-string candidates
rust/target/release/atom-harness match "dev-libs/foo[bar]" \
    dev-libs/foo-1.0 dev-libs/foo-2.0
# dev-libs/foo-1.0,dev-libs/foo-2.0
rust/target/release/atom-harness match "dev-libs/foo[-bar]" \
    dev-libs/foo-1.0 dev-libs/foo-2.0
# dev-libs/foo-1.0,dev-libs/foo-2.0

# "=*" glob version operator: component-boundary aware, not a naive
# string prefix -- "1*" matches "1.2" (real boundary: ".") but not "10"
# (both digits, no real boundary -- bug 560466)
rust/target/release/atom-harness match "=dev-libs/foo-1*" \
    dev-libs/foo-1.2 dev-libs/foo-10
# dev-libs/foo-1.2

# "::reponame" repo constraint: rejects a candidate only if it carries a
# KNOWN, different repo -- the repo-less candidate always passes too
rust/target/release/atom-harness match "dev-libs/foo::gentoo" \
    dev-libs/foo-1.0 dev-libs/foo-1.0::gentoo dev-libs/foo-1.0::other
# dev-libs/foo-1.0,dev-libs/foo-1.0::gentoo
```

Try the use_reduce harness:

```sh
# Python
python3 python/use_reduce_harness.py reduce normal bar \
    dev-libs/foo bar? "(" dev-libs/baz ")" "!bar?" "(" dev-libs/qux ")"

# Rust
rust/target/release/use-reduce-harness reduce normal bar \
    dev-libs/foo bar? "(" dev-libs/baz ")" "!bar?" "(" dev-libs/qux ")"

# REQUIRED_USE ("^^ ( a b )", exactly-one-of, with only "a" enabled --
# satisfied): Python then Rust, same output either way
python3 python/required_use_harness.py check a a,b "^^" "(" a b ")"
rust/target/release/required-use-harness check a a,b "^^" "(" a b ")"
# true
```

Try `emerge --pretend` against the fixture tree:

```sh
ln -sf "$(realpath rust/target/release/portuale)" /tmp/emerge
FX="$(realpath fixtures)"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg              # -> [ebuild  N    ] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg # -> [ebuild     U ] ...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg             # -> already installed

# real PkgAttrDisplay bracket layout (increment 1 of the -pv real-output.py
# layout + colour buildout): the fixed-width [I][N/r][S/R][f/F/g][U][D]
# field, and [old-ver] in place of the "(upgrade from X)" prose
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/downgradepkg
# [ebuild     UD] dev-libs/downgradepkg-1.0 [2.0]
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/changeddepspkg
# [ebuild   R   ] dev-libs/changeddepspkg-1.0     <- a plain reinstall: R, no inline reason (real -pv)
# [ebuild  N    ] dev-libs/newpkg-1.0

# ANSI colour (increment 2): --color y|n overrides the NO_COLOR/isatty
# gate. diamond is a favorite (PKG_MERGE_WORLD, green); its deps are
# plain PKG_MERGE (darkgreen); N is green, [old-ver] blue, ~ mask yellow.
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --color y dev-libs/diamond | cat -v
# [^[[32;01mebuild^[[39;49;00m  ^[[32;01mN^[[39;49;00m    ] ^[[32;01mdev-libs/diamond-1.0^[[39;49;00m
# [^[[32mebuild^[[39;49;00m  ^[[32;01mN^[[39;49;00m    ] ^[[32mdev-libs/shared-a-1.0^[[39;49;00m
#   ... (deps darkgreen)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" NO_COLOR=1 /tmp/emerge --pretend --color y dev-libs/diamond | cat -v
#   -- still coloured: an explicit --color y wins over NO_COLOR

# dependency recursion: diamond dependency, deduped (see fixtures)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/diamond
# [ebuild  N    ] dev-libs/diamond-1.0
# [ebuild  N    ] dev-libs/shared-a-1.0
# [ebuild  N    ] dev-libs/shared-b-1.0
# [ebuild  N    ] dev-libs/common-1.0

# --tree: the same diamond, indented -- common nests under shared-a
# only (first alphabetically), never repeated under shared-b
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree dev-libs/diamond
# [ebuild  N    ] dev-libs/diamond-1.0
# [ebuild  N    ]   dev-libs/shared-a-1.0
# [ebuild  N    ]     dev-libs/common-1.0
# [ebuild  N    ]   dev-libs/shared-b-1.0

# --unordered-display (only meaningful with --tree): preserves RDEPEND's
# own literal order instead of sorting alphabetically -- treeorderpkg's
# own RDEPEND deliberately lists its children reverse-alphabetically
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree dev-libs/treeorderpkg
# [ebuild  N    ] dev-libs/treeorderpkg-1.0
# [ebuild  N    ]   dev-libs/atreechild-1.0
# [ebuild  N    ]   dev-libs/ztreechild-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree --unordered-display dev-libs/treeorderpkg
# [ebuild  N    ] dev-libs/treeorderpkg-1.0
# [ebuild  N    ]   dev-libs/ztreechild-1.0
# [ebuild  N    ]   dev-libs/atreechild-1.0

# --columns: the version moves out of the inline "-1.0" suffix into its
# own right-aligned column instead (COLUMNWIDTH here trimmed to 70 for a
# readable example -- the real default is 130)
COLUMNWIDTH=70 PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --columns dev-libs/newpkg
# [ebuild  N    ] dev-libs/newpkg [1.0]
# an Upgrade's own old version appears in its own trailing column too --
# the same information the default format's own "(upgrade from X)"
# parenthetical carries, just repositioned
COLUMNWIDTH=70 PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --columns dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg [2.0] [1.0]
# --tree and --columns can't be combined, matching real portage
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --tree --columns dev-libs/newpkg
# emerge: can't specify both of "--tree" and "--columns".  (exit 2)

# BDEPEND/PDEPEND/IDEPEND are walked too, not just DEPEND/RDEPEND -- v1
# makes no distinction between any of the five real dependency-string
# keys (no real merge ordering exists yet for the distinction to matter)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/bdependpkg
# [ebuild  N    ] dev-libs/bdependpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# real slot-operator dependency atoms (":=" and ":1=") are resolved, not
# silently dropped -- ":1=" specifically resolves multislotpkg's SLOT=1
# version (2.0), not its SLOT=0 version (1.0)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/slotoperatorpkg
# [ebuild  N    ] dev-libs/slotoperatorpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/multislotpkg-2.0

# a real sub-slot restriction (":0/2", PMS 8.3.3, not a slot-operator) now
# actually matches -- dev-libs/subslotpkg's own SLOT is "0/2"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/subslotconsumer
# [ebuild  N    ] dev-libs/subslotconsumer-1.0
# [ebuild  N    ] dev-libs/subslotpkg-1.0

# ...and a genuine sub-slot mismatch (":0/3" against the same "0/2"
# candidate) is genuinely rejected, not just always accepted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/subslotmismatchconsumer
# [ebuild  N    ] dev-libs/subslotmismatchconsumer-1.0
# !!! no visible ebuild for dependency "dev-libs/subslotpkg"

# real USE-dep dependency atoms are resolved AND enforced now: both
# "[bar(+)]"/"[baz(+)?]" are (+)-defaulted flags missing from their own
# target's IUSE, so both are genuinely, trivially satisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeppkg
# [ebuild  N    ] dev-libs/usedeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/multislotpkg-2.0

# USE-dep enforcement, top-level: useflagpkg's own IUSE="foo missingflag",
# "foo" enabled globally -- "[foo]" (declared, enabled) is satisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[foo]'
# [ebuild  N    ] dev-libs/useflagpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# "[-foo]" (declared, but enabled, not disabled) is genuinely unsatisfied
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[-foo]'
# emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".  (exit 1)
# a flag not declared in IUSE at all, with no (+)/(-) default, never
# matches -- real _use_dep.required, checked before enabled/disabled at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[nonexistentflag]'
# emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[nonexistentflag]".  (exit 1)
# ...but a "(+)" default rescues a flag missing from IUSE, standing in
# for "as if enabled"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/useflagpkg[nonexistentflag(+)]'
# [ebuild  N    ] dev-libs/useflagpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# USE-dep enforcement, dependency level: usedeprejectedpkg's own RDEPEND
# is "dev-libs/useflagpkg[-foo]", genuinely unsatisfiable -- the parent
# still resolves, the rejected dependency is reported, not silently
# dropped or accepted (same "report, don't fail" spirit as an
# unresolvable dependency)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeprejectedpkg
# [ebuild  N    ] dev-libs/usedeprejectedpkg-1.0
# !!! no visible ebuild for dependency "dev-libs/useflagpkg"  (stderr)

# opt= conditional USE-dep (PMS 8.3.4): useeqparentonpkg's own
# IUSE="+eqflag" defaults it ON, so its RDEPEND's "[eqflag=]" evaluates
# to "[eqflag]" -- matches useeqchildpkg's own default-on eqflag
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useeqparentonpkg
# [ebuild  N    ] dev-libs/useeqparentonpkg-1.0
# [ebuild  N    ] dev-libs/useeqchildpkg-1.0
# the identical use-dep string, but useeqparentoffpkg's own IUSE="eqflag"
# (no "+") defaults it OFF -- "[eqflag=]" now evaluates to "[-eqflag]",
# which mismatches the child, so the dependency is reported unresolvable
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useeqparentoffpkg
# [ebuild  N    ] dev-libs/useeqparentoffpkg-1.0
# !!! no visible ebuild for dependency "dev-libs/useeqchildpkg"  (stderr)

# REQUIRED_USE is real and implemented: requireduseokpkg's own
# "foo? ( bar )" is genuinely satisfied (foo enabled globally, bar
# forced on by this package's own package.use entry)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requireduseokpkg
# [ebuild  N    ] dev-libs/requireduseokpkg-1.0
# requiredusebadpkg has the identical constraint but nothing forcing
# "bar" on -- genuinely violated, which aborts the WHOLE run (exit 1),
# a harsher severity than a merely unresolvable dependency
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadpkg
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"  (exit 1)
# ...and still aborts the whole run even when only reached as a
# dependency, not just as a top-level atom
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadparentpkg
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"  (exit 1)

# ...and the whole walk keeps going past the first violation: a SECOND,
# independent top-level atom's own unrelated REQUIRED_USE violation
# still gets attempted and reported too, not silently skipped once the
# first one failed -- matching real depgraph.py's own "collect every
# violation, only fail at the very end" severity, not "abort on the
# first hit"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/requiredusebadpkg dev-libs/requiredusebadpkg2
# emerge: REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: "foo? ( bar )"
# REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg2-1.0: "baz? ( qux )"  (exit 1)

# --autounmask: this package is masked by KEYWORDS alone ("~amd64", no
# package.accept_keywords entry) -- quiet by default (real
# --autounmask-keep-keywords defaults to suppressing keyword
# suggestions when --autounmask itself was never explicitly given)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/autounmaskkeywordpkg
# emerge: there are no ebuilds to satisfy "dev-libs/autounmaskkeywordpkg".  (exit 1)
# ...but once --autounmask is explicitly given, real portage RESOLVES the
# graph with the implicit `=cpv ~arch` change applied (real
# _display_autounmask) -- normal merge list on stdout, the "necessary to
# proceed" block on stderr, exit 0 (real actions.py:563)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask dev-libs/autounmaskkeywordpkg
# [ebuild  N    ] dev-libs/autounmaskkeywordpkg-1.0          (stdout)
#                                                            (stderr:)
# The following keyword changes are necessary to proceed:
#  (see "package.accept_keywords" in the portage(5) man page for more details)
# # required by dev-libs/autounmaskkeywordpkg (argument)
# =dev-libs/autounmaskkeywordpkg-1.0 ~amd64                  (exit 0)
# the same, now for a *dependency's* own keyword-masked-only candidate
# (dev-libs/autounmaskdepconsumer RDEPENDs on the fixture above) -- quiet
# by default (just the "no visible ebuild" line), exit 0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/autounmaskdepconsumer
# [ebuild  N    ] dev-libs/autounmaskdepconsumer-1.0
# !!! no visible ebuild for dependency "dev-libs/autounmaskkeywordpkg"  (exit 0)
# ...and once --autounmask is given, BOTH packages resolve and the block
# carries the real two-line dep chain
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask dev-libs/autounmaskdepconsumer
# [ebuild  N    ] dev-libs/autounmaskdepconsumer-1.0         (stdout)
# [ebuild  N    ] dev-libs/autounmaskkeywordpkg-1.0
#                                                            (stderr:)
# The following keyword changes are necessary to proceed:
#  (see "package.accept_keywords" in the portage(5) man page for more details)
# # required by dev-libs/autounmaskdepconsumer-1.0::testrepo
# # required by dev-libs/autounmaskdepconsumer (argument)
# =dev-libs/autounmaskkeywordpkg-1.0 ~amd64                  (exit 0)
# --json exposes the change as a top-level array
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask --json dev-libs/autounmaskdepconsumer | python3 -c 'import json,sys; print(json.load(sys.stdin)["autounmask_keyword_changes"])'
# [{'cpv': 'dev-libs/autounmaskkeywordpkg-1.0', 'token': '~amd64', 'dep_chain': ['required by dev-libs/autounmaskdepconsumer-1.0::testrepo', 'required by dev-libs/autounmaskdepconsumer (argument)']}]

# --autounmask-use: on by default (unlike the keyword kind), so
# useflagpkg's own real "foo" (globally enabled, but this atom demands
# "-foo") RESOLVES with an implicit package.use flip -- real portage's
# default. `>=<cpv>` atom form (real check_if_latest for USE, bug #536392)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend "dev-libs/useflagpkg[-foo]"
# [ebuild  N    ] dev-libs/useflagpkg-1.0                     (stdout)
#                                                             (stderr:)
# The following USE changes are necessary to proceed:
#  (see "package.use" in the portage(5) man page for more details)
# # required by dev-libs/useflagpkg[-foo] (argument)
# >=dev-libs/useflagpkg-1.0 -foo                              (exit 0)
# --autounmask-use=n restores the strict "USE-dep mismatch -> no visible
# candidate" behaviour
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --autounmask-use=n "dev-libs/useflagpkg[-foo]"
# emerge: there are no ebuilds to satisfy "dev-libs/useflagpkg[-foo]".  (exit 1)
# a dependency's own USE-dep mismatch resolves the same way, with the
# two-line dep chain (dev-libs/usedeprejectedpkg RDEPENDs the atom above)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/usedeprejectedpkg
# [ebuild  N    ] dev-libs/usedeprejectedpkg-1.0             (stdout)
# [ebuild  N    ] dev-libs/useflagpkg-1.0
#                                                            (stderr:)
# The following USE changes are necessary to proceed:
#  (see "package.use" in the portage(5) man page for more details)
# # required by dev-libs/usedeprejectedpkg-1.0::testrepo
# # required by dev-libs/usedeprejectedpkg (argument)
# >=dev-libs/useflagpkg-1.0 -foo                             (exit 0)
# --json exposes the change as a top-level array
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/useeqparentoffpkg | python3 -c 'import json,sys; print(json.load(sys.stdin)["autounmask_use_changes"])'
# [{'atom': '>=dev-libs/useeqchildpkg-1.0', 'token': '-eqflag', 'dep_chain': ['required by dev-libs/useeqparentoffpkg-1.0::testrepo', 'required by dev-libs/useeqparentoffpkg (argument)']}]

# IUSE's own "+"/"-" default markers are honored now: "+enableddefault"
# defaults on, "-disableddefault" stays off (own REQUIRED_USE requires
# exactly this), and "plainflag" (no default marker at all) is
# genuinely undecided by IUSE -- but forced on by this package's own
# package.use entry, proving IUSE defaults and package.use coexist
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/iusedefaultpkg
# [ebuild  N     ] dev-libs/iusedefaultpkg-1.0  USE="-disableddefault enableddefault plainflag"

# "x86" is never in this package's own IUSE, but IS a real, valid
# profiles/arch.list entry -- implicitly valid for REQUIRED_USE even
# though it's not the active profile's own arch (stays disabled), the
# same shape real media-libs/mesa's own REQUIRED_USE hits
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/archiuseimplicitpkg
# [ebuild  N    ] dev-libs/archiuseimplicitpkg-1.0

# Global use.force/use.mask win over a contradicting package.use entry:
# this package's own package.use entry tries to invert both flags
# ("-globalforceflag globalmaskflag"), but the profile's own use.force/
# use.mask (applied strictly after package.use, matching real
# regenerate()'s own literal-last-step ordering) win on both
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/globalprecedencepkg
# [ebuild  N     ] dev-libs/globalprecedencepkg-1.0  USE="globalforceflag -globalmaskflag"

# A profile-level "-flag" genuinely cancels an IUSE "+default": this
# package's own IUSE is "+cancelme" (defaults on), but
# profiles/default/make.defaults declares "-cancelme" -- real portage's
# own single continuous incremental walk lets it reach back and cancel
# the earlier IUSE default, not just fail to add on top of it
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/cancelledpkg
# [ebuild  N     ] dev-libs/cancelledpkg-1.0  USE="-cancelme"

# real profile/make.conf resolution: "foo" is enabled by the fixture's
# profile chain, so this package's foo?-gated dependency is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N    ] dev-libs/useflagpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# USE_EXPAND is real and implemented: profiles/base/make.defaults'
# VIDEO_CARDS="nvidia" expands into the pseudo-USE flag
# "video_cards_nvidia", which genuinely gates a dependency, not just -v
# display
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/useexpandpkg
# [ebuild  N     ] dev-libs/useexpandpkg-1.0  USE="-video_cards_amdgpu video_cards_nvidia"
# [ebuild  N    ] dev-libs/newpkg-1.0

# USE_EXPAND_UNPREFIXED is real and implemented too: profiles/arch/amd64/
# make.defaults' own ARCH="amd64" contributes the bare pseudo-USE flag
# "amd64" (no "arch_" prefix at all, unlike an ordinary USE_EXPAND
# variable) -- this is literally how "amd64" exists as a real USE flag
# in actual Gentoo, and it genuinely gates a dependency here too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/archusepkg
# [ebuild  N     ] dev-libs/archusepkg-1.0  USE="amd64 -riscv"
# [ebuild  N    ] dev-libs/newpkg-1.0

# package.use's own USE_EXPAND-prefix shorthand is real and implemented
# too: "dev-libs/packageuseexpandpkg PYTHON_TARGETS: python3_12" in
# fixtures/etc/portage/package.use expands to
# "python_targets_python3_12", user-level package.use only
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/packageuseexpandpkg
# [ebuild  N     ] dev-libs/packageuseexpandpkg-1.0  USE="python_targets_python3_12"
# [ebuild  N    ] dev-libs/newpkg-1.0

# use.stable.force/package.use.stable.mask are real and implemented too:
# stableusepkg's own KEYWORDS="amd64" (no "~") is genuinely stable, so
# both apply -- stableforceflag forced on (pulling in a real dependency)
# and maskflag masked back off despite package.use enabling it first
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/stableusepkg
# [ebuild  N     ] dev-libs/stableusepkg-1.0  USE="-maskflag stableforceflag"
# [ebuild  N    ] dev-libs/newpkg-1.0
# unstableusepkg shares the identical IUSE/RDEPEND/package.use entry,
# but its own KEYWORDS="~amd64" is genuinely NOT stable -- neither
# applies: stableforceflag stays off, maskflag stays on
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/unstableusepkg
# [ebuild  N     ] dev-libs/unstableusepkg-1.0  USE="maskflag -stableforceflag"

# package.mask: hidden, no matching package.unmask entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/hardmaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/hardmaskedpkg".  (exit 1)

# package.mask + package.unmask: masked, then unmasked again -> visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/maskedandunmaskedpkg
# [ebuild  N    ] dev-libs/maskedandunmaskedpkg-1.0

# repo-level profiles/package.mask (real portage's most common real-world
# masking source, e.g. security/arch masks) hides a package the same way
# a user-level package.mask entry does
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/repomaskedpkg".  (exit 1)

# a repo-level mask, cancelled by a profile-level package.unmask entry --
# proving the three sources (repo, profile chain, user) are genuinely
# stacked together, not checked independently
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedthenprofileunmaskedpkg
# [ebuild  N    ] dev-libs/repomaskedthenprofileunmaskedpkg-1.0

# a repo-level mask, cancelled by a "-atom" line in the user-level
# package.mask -- -atom removal now spans all three sources, not just
# within the one file that contains the "-atom" line
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repomaskedthenuserremovedpkg
# [ebuild  N    ] dev-libs/repomaskedthenuserremovedpkg-1.0

# package.accept_keywords wildcard ("*/wildcardkeywordpkg ~amd64") makes an
# otherwise ~amd64-only, not-globally-accepted package visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/wildcardkeywordpkg
# [ebuild  N    ] dev-libs/wildcardkeywordpkg-1.0

# package.accept_keywords "**" accepts a package with no KEYWORDS at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/livekeywordpkg
# [ebuild  N    ] dev-libs/livekeywordpkg-9999

# package.accept_keywords negation ("-amd64") revokes a keyword the
# global ACCEPT_KEYWORDS="amd64" already granted -- for this one
# genuinely stable package specifically
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/keywordrevokedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/keywordrevokedpkg".  (exit 1)

# package.accept_keywords "*"/"~*" wildcards: accept any stable/testing
# keyword respectively, distinct from "**" -- "*" alone would NOT have
# covered the second package below, since it's testing-only (~arm64)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/starkeywordpkg
# [ebuild  N    ] dev-libs/starkeywordpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/tildestarkeywordpkg
# [ebuild  N    ] dev-libs/tildestarkeywordpkg-1.0

# package.accept_keywords bare atom: no keyword tokens at all, real
# accept_keywords_defaults still grants an implicit "~amd64" (global
# ACCEPT_KEYWORDS="amd64", "~"-prefixed) -- not a no-op
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/bareacceptkeywordspkg
# [ebuild  N    ] dev-libs/bareacceptkeywordspkg-1.0

# package.accept_keywords is now also stacked from the profile chain, not
# just /etc/portage -- this package has no user-level entry at all, only
# a profile-level one (see fixtures/repo/profiles/arch/amd64)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/profileacceptkeywordspkg
# [ebuild  N    ] dev-libs/profileacceptkeywordspkg-1.0

# package.use ("*/packageuseenablepkg pkguseflag") enables a flag that's
# off everywhere else, pulling in its pkguseflag?-gated dependency
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packageuseenablepkg
# [ebuild  N    ] dev-libs/packageuseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# package.use ("dev-libs/packageusedisablepkg -foo") disables a flag for
# just this package, even though "foo" is on globally (contrast with
# dev-libs/useflagpkg above, whose own foo?-gated dependency IS pulled in)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packageusedisablepkg
# [ebuild  N    ] dev-libs/packageusedisablepkg-1.0

# package.use is now stacked from repo+profile too, not just
# /etc/portage -- neither of these packages has any user-level entry
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/repouseenablepkg
# [ebuild  N    ] dev-libs/repouseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/profileuseenablepkg
# [ebuild  N    ] dev-libs/profileuseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# a strong (!!) blocker matching an already-installed package is reported
# (not enforced -- exit code is still 0, same as real --pretend); the
# line follows real output.py::_blockers now
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/blockerpkg
# [ebuild  N    ] dev-libs/blockerpkg-1.0
# [blocks B     ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerpkg-1.0)

# a weak (!) blocker matching another package this same run would also
# newly merge (not just an installed one) -- "soft blocking"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/graphblockerparent
# [ebuild  N    ] dev-libs/graphblockerparent-1.0
# [ebuild  N    ] dev-libs/blockerpartnerpkg-1.0
# [ebuild  N    ] dev-libs/weakblockerpkg-1.0
# [blocks B     ] dev-libs/blockerpartnerpkg ("dev-libs/blockerpartnerpkg" is soft blocking dev-libs/weakblockerpkg-1.0)

# blocker lines print as one group AFTER every package line (real
# Display.print_blockers), not interleaved: blockerorderpkg's own blocker
# owner is the first entry, but the [blocks] line still lands last
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/blockerorderpkg
# [ebuild  N    ] dev-libs/blockerorderpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [blocks B     ] dev-libs/samepkg ("dev-libs/samepkg" is hard blocking dev-libs/blockerorderpkg-1.0)

# overlays: a package that exists only in the overlay repo (see
# fixtures/etc/portage/repos.conf) is found
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayonlypkg
# [ebuild  N    ] dev-libs/overlayonlypkg-1.0

# "::reponame" repo constraint: the same package, constrained to the
# repo it's NOT in, correctly finds nothing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayonlypkg::testrepo
# emerge: there are no ebuilds to satisfy "dev-libs/overlayonlypkg::testrepo".  (exit 1)

# same version in both repos: the higher-priority overlay's own copy is
# the one actually used, proven by its RDEPEND (not the main repo copy's)
# pulling in dev-libs/newpkg
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaytiepkg
# [ebuild  N    ] dev-libs/overlaytiepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# overlay repos' own package.mask: overlaymaskedpkg is masked only in the
# overlay's own profiles/package.mask (a bare atom, auto-scoped to
# "::overlay" by real append_repo) -- an unconstrained atom still
# resolves via the main repo's own, unaffected copy
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedpkg
# [ebuild  N    ] dev-libs/overlaymaskedpkg-1.0

# an explicit "::overlay" atom does hit that same auto-scoped mask
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedpkg::overlay
# emerge: there are no ebuilds to satisfy "dev-libs/overlaymaskedpkg::overlay".  (exit 1)

# the overlay's own package.unmask cancels that same overlay's own
# package.mask entry (both get the identical "::overlay" auto-scoping)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlaymaskedthenunmaskedpkg
# [ebuild  N    ] dev-libs/overlaymaskedthenunmaskedpkg-1.0

# repos.conf masters: the overlay has no explicit "masters =", so it
# implicitly masters the main repo -- mastermaskedpkg exists only in the
# overlay and is masked purely by the MAIN repo's own package.mask
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/mastermaskedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/mastermaskedpkg".  (exit 1)

# the overlay's own package.unmask still cancels a masters-inherited
# mask, since both get the identical "::overlay" auto-scoping
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/mastermaskedthenoverlayunmaskedpkg
# [ebuild  N    ] dev-libs/mastermaskedthenoverlayunmaskedpkg-1.0

# explicit repos.conf masters=: independentoverlay declares
# "masters = overlay", NOT the main repo -- independentmastermainonlypkg
# exists only there and is masked only by the MAIN repo's own
# package.mask, which does NOT apply since main isn't a declared master
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/independentmastermainonlypkg
# [ebuild  N    ] dev-libs/independentmastermainonlypkg-1.0

# independentmasteroverlaypkg (also only in independentoverlay) is
# masked only by the OVERLAY repo's own package.mask instead -- which
# DOES apply, since overlay is independentoverlay's declared master
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/independentmasteroverlaypkg
# emerge: there are no ebuilds to satisfy "dev-libs/independentmasteroverlaypkg".  (exit 1)

# overlay package.use/.force/.mask: all three now read from every repo,
# not just main -- overlayuseenablepkg exists only in the overlay, whose
# own profiles/package.use enables its own overlayuseflag
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayuseenablepkg
# [ebuild  N    ] dev-libs/overlayuseenablepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# package.use.force: the overlay's own profiles/package.use.force forces
# a flag on that's off by IUSE default and every other source
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayuseforcepkg
# [ebuild  N    ] dev-libs/overlayuseforcepkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# package.use.mask: the inverse -- IUSE="+overlaymaskflag" defaults the
# flag on, but the overlay's own profiles/package.use.mask masks it off
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/overlayusemaskpkg
# [ebuild  N    ] dev-libs/overlayusemaskpkg-1.0

# slot conflict: slotconflictnewconsumer resolves slotconflicttarget to
# 2.0 first; slotconflictoldconsumer's own "<...-2.0" constraint rejects
# that -- reported, not enforced (exit code and the rest of the graph are
# unaffected)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/slotconflictparent
# [ebuild  N    ] dev-libs/slotconflictparent-1.0
# [ebuild  N    ] dev-libs/slotconflictnewconsumer-1.0
# [ebuild  N    ] dev-libs/slotconflictoldconsumer-1.0
# [ebuild  N    ] dev-libs/slotconflicttarget-2.0
# [slot conflict] dev-libs/slotconflicttarget:0 resolved to dev-libs/slotconflicttarget-2.0, which does not satisfy "<dev-libs/slotconflicttarget-2.0"

# NOT a conflict: two different slots of the same package coexist as
# independent entries, same as real portage allows
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/multislotparent
# [ebuild  N    ] dev-libs/multislotparent-1.0
# [ebuild  N    ] dev-libs/multislotpkg-1.0
# [ebuild  N    ] dev-libs/multislotpkg-2.0

# virtuals: virtual/texteditor is shaped like the real virtual/pager (an
# ordinary ebuild, any-of RDEPEND) -- no dedicated resolution code exists
# for it, or is needed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/virtualconsumerpkg
# [ebuild  N    ] dev-libs/virtualconsumerpkg-1.0
# [ebuild  N    ] virtual/texteditor-0
# [ebuild  N    ] dev-libs/newpkg-1.0

# real "||" semantics: neither alternative in this package's own RDEPEND
# has a visible candidate anywhere, so BOTH still get reported (the
# fallback, matching the pre-existing "never silently wrong about
# whether a dependency exists" invariant) instead of one being silently
# dropped
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/anyofunresolvable
# [ebuild  N    ] dev-libs/anyofunresolvable-1.0
# !!! no visible ebuild for dependency "dev-libs/doesnotexist-anywhere"
# !!! no visible ebuild for dependency "dev-libs/alsodoesnotexist-anywhere"

# multiple top-level atoms: a dependency shared between two REQUESTED
# packages (not just two deps of one package) dedupes the same way a
# diamond dependency always did
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/shared-a dev-libs/shared-b
# [ebuild  N    ] dev-libs/shared-a-1.0
# [ebuild  N    ] dev-libs/shared-b-1.0
# [ebuild  N    ] dev-libs/common-1.0

# a bad top-level atom aborts the whole run immediately, in argv order --
# real portage's own "there are no ebuilds to satisfy" wording (from
# lib/_emerge/depgraph.py), not enforced/reported-and-continued the way a
# dependency's own missing candidate is
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/does-not-exist dev-libs/newpkg
# emerge: there are no ebuilds to satisfy "dev-libs/does-not-exist".  (exit 1)

# a top-level atom can now carry an operator/slot, same as a dependency
# atom always could -- resolve_pretend's own matching needed no changes
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend '>=dev-libs/newpkg-1.0'
# [ebuild  N    ] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/newpkg:0
# [ebuild  N    ] dev-libs/newpkg-1.0

# a blocker is still rejected as a target -- fixed to be an explicit,
# reported rejection instead of the pre-existing silent no-op (accepted
# by the CLI, then dropped by resolve_pretend_graph's own blocker skip)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend '!!dev-libs/newpkg'
# emerge (pilot v1): "!!dev-libs/newpkg" is a blocker, not a valid emerge target  (exit 2)

# @world expands in place to the union of the fixture world file's own
# atoms (newpkg directly, withdeps -- which recurses into newpkg again,
# deduped, and upgradepkg -- and a "@some-nested-set-reference" line
# that's silently skipped there, not mishandled, since a "@"-prefixed
# line genuinely fails real portage's own world-FILE validation too) and
# the fixture world_sets file's own "@nestedtestset" (nestedsetpkg
# directly, plus a further nested "@innernestedset" reference --
# innernestedsetpkg, which itself cycles back to "@nestedtestset" without
# looping). --update is added here purely so upgradepkg actually
# upgrades (see the --update example further below) rather than staying
# silently already-installed -- unrelated to @world itself
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update @world
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild  N    ] dev-libs/nestedsetpkg-1.0
# [ebuild  N    ] dev-libs/innernestedsetpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# @world combines with an explicit atom in the same invocation
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/samepkg @world
# dev-libs/samepkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild  N    ] dev-libs/nestedsetpkg-1.0
# [ebuild  N    ] dev-libs/innernestedsetpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# an unresolvable "@name" listed in world_sets is a real, immediate
# error (real PackageSetNotFound) -- unlike a missing world/world_sets
# *file* itself (a real, valid "nothing selected" state)
mkdir -p /tmp/badset-root/var/lib/portage
echo '@doesnotexist' > /tmp/badset-root/var/lib/portage/world_sets
PORTAGE_CONFIGROOT="$FX" ROOT="/tmp/badset-root" /tmp/emerge --pretend @world
# emerge: set 'doesnotexist' not found  (exit 1)

# a missing world file (a fresh ROOT that's never had anything merged
# into it) is a real, valid empty state, not an error -- it hits the same
# "nothing to resolve" error any other empty target list would
mkdir -p /tmp/empty-world-root
PORTAGE_CONFIGROOT="$FX" ROOT="/tmp/empty-world-root" /tmp/emerge --pretend @world
# emerge (pilot v1): no package atoms to resolve (the target list, after
# expanding any @world/@system, is empty)  (exit 2)

# @system is real and implemented too: base/packages contributes newpkg,
# the leaf profile's own default/packages contributes withdeps -- proving
# @system stacks across multiple profile levels and feeds the same
# recursion machinery @world does (withdeps recurses into newpkg again,
# deduped, and upgradepkg). --update again just so upgradepkg upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update @system
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# only the literal tokens "@world"/"@system" trigger expansion -- any
# other "@"-prefixed target falls through to the ordinary atom-parsing
# path and gets a clear error, not a silent no-op
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend @some-other-set
# emerge: invalid atom "@some-other-set"  (exit 1)

# --verbose/-v is real and implemented: USE flags are off by default,
# same as real emerge, and only shown with -v -- alphabetically sorted,
# limited to this package's own IUSE (foo enabled, missingflag disabled,
# per the fixture profile chain)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/useflagpkg
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
# [ebuild  N    ] dev-libs/newpkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/useflagpkg
# [ebuild  N    ] dev-libs/useflagpkg-1.0   (no -v: no USE= at all)
# [ebuild  N    ] dev-libs/newpkg-1.0

# -v/--verbose isn't a plain boolean in real emerge -- an explicit
# following "n" disables it again, same as real insert_optional_args
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v n dev-libs/useflagpkg
# [ebuild  N    ] dev-libs/useflagpkg-1.0   (explicit "n": no USE= shown)
# [ebuild  N    ] dev-libs/newpkg-1.0

# --newuse/-N is real and implemented: reinstallpkg is installed with
# IUSE="foo" declared but an empty vdb USE file (foo was off at merge
# time); the fixture profile chain enables "foo" globally now, so
# --newuse reports a Reinstall for the changed flag -- and still recurses
# into its own RDEPEND, exactly like a New/Upgrade entry would
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/reinstallpkg
# [ebuild   R   ] dev-libs/reinstallpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# without --newuse, the exact same package stays AlreadyInstalled -- the
# USE mismatch is real, but nothing checks for it unless --newuse is given.
# --noreplace isolates this from real portage's own separate "selective"
# default for a bare top-level atom (see --noreplace/--selective below)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/reinstallpkg
# dev-libs/reinstallpkg-1.0 is already installed; nothing to do

# --newuse is a no-op when USE hasn't changed -- samepkg has no IUSE at
# all (declared or in the vdb), so there's nothing to detect a change in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do

# --newuse's forced_flags subtraction: usemaskreinstallpkg's newly
# IUSE-declared flag is masked off by use.mask, so it never actually
# changed enablement -- without forced_flags this would spuriously
# report a Reinstall just because the flag now exists in IUSE at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/usemaskreinstallpkg
# dev-libs/usemaskreinstallpkg-1.0 is already installed; nothing to do

# --changed-use/-U is real and implemented too, a narrower sibling of
# --newuse: changedusepkg's newly IUSE-declared "brandnewflag" is real
# and unmasked (unlike usemaskreinstallpkg's own above), so --newuse
# still reports a Reinstall for it (IUSE simply gained a flag)...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --newuse dev-libs/changedusepkg
# [ebuild   R   ] dev-libs/changedusepkg-1.0
# ...but --changed-use never even looks at IUSE presence, only at
# enablement -- and that flag's own enablement never changed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-use dev-libs/changedusepkg
# dev-libs/changedusepkg-1.0 is already installed; nothing to do
# --changed-use still catches an ENABLEMENT change on a flag shared by
# both IUSE sets, same as reinstallpkg's own --newuse example above
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-use dev-libs/reinstallpkg
# [ebuild   R   ] dev-libs/reinstallpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --update/-u is real and implemented: with `selective` restored via
# --noreplace, real emerge does NOT offer to upgrade a package just
# because a newer version exists -- upgradepkg is installed at 1.0, a
# newer 2.0 is visible in the tree, but "emerge --noreplace
# dev-libs/upgradepkg" leaves it alone (real depgraph.py's own
# avoid_update, lines 7814/8448)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do
# --update (or its short alias -u) is what makes the newer version show up
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]
# --update threads through the whole dependency graph, not just a
# top-level atom: here upgradepkg is reached only as withdeps' own
# dependency, and still upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/withdeps
# [ebuild  N    ] dev-libs/withdeps-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# without --update AND without --noreplace/--selective, a bare top-level
# atom still finds the newer version on its own -- real portage's own
# "selective" gap (see --noreplace/--selective further down)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# --noreplace/-n and --selective are real and implemented: samepkg has no
# newer version and nothing else about it changed, yet a bare top-level
# atom still reports a plain reinstall, no reason given at all -- real
# portage's own "selective" gap (see the paragraph above)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/samepkg
# [ebuild   R   ] dev-libs/samepkg-1.0
# --noreplace (or its real synonym --selective) restores "nothing to do"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do
# --selective=n explicitly cancels selective even if another flag (here,
# --update) would otherwise have set it -- unlike the upgradepkg example
# above, samepkg has nothing newer, so --update alone still leaves it
# alone; --selective=n forces the bare reinstall anyway
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/samepkg
# dev-libs/samepkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --selective=n dev-libs/samepkg
# [ebuild   R   ] dev-libs/samepkg-1.0

# --deep/-D is real and implemented: without it, real emerge never walks
# an already-installed package's own further dependencies, no matter how
# deep the graph goes -- deeppkg is installed and RDEPENDs on deeppkg2
# (also installed), which itself RDEPENDs on newpkg (New), but neither
# ever shows up here. --noreplace keeps deeppkg itself AlreadyInstalled
# (see --noreplace/--selective further down), isolating --deep's own
# gating from real portage's own separate "selective" default for a bare
# top-level atom
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# a bare --deep (unlimited depth) walks the whole already-installed
# chain -- deeppkg2 itself stays silent (already installed, not a
# top-level atom), but newpkg's own [ebuild N] line now appears
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0
# --deep=N bounds the depth: 1 level reaches deeppkg2 but not newpkg
# (identical output to no --deep at all); 2 levels reaches all the way
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep=1 dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep=2 dev-libs/deeppkg
# dev-libs/deeppkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0

# --emptytree/-e reinstalls the whole deep dependency tree as though
# nothing is installed (real create_depgraph_params.py: empty + deep,
# selective popped) -- useful for comparing against real portage and for
# debugging resolution
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pe dev-libs/deeppkg
# [ebuild   R   ] dev-libs/deeppkg-1.0        <- installed -> bare reinstall
# [ebuild   R   ] dev-libs/deeppkg2-1.0       <- installed dep, no longer hidden
# [ebuild  N    ] dev-libs/newpkg-1.0         <- not installed -> New

# --exclude/-X is real and implemented: without it, --update offers the
# visible upgrade normally
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update dev-libs/upgradepkg
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]
# --exclude matching the installed package overrides --update entirely --
# it's checked first, unconditionally, before --update/--newuse/
# --changed-use ever get a say
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --exclude dev-libs/upgradepkg dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do
# excluding a package that isn't installed at all means there's no
# eligible candidate left -- the same fatal "no ebuilds to satisfy"
# outcome any other unsatisfiable top-level atom already gets
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --exclude dev-libs/newpkg dev-libs/newpkg
# emerge: there are no ebuilds to satisfy "dev-libs/newpkg".  (exit 1)
# real "action": "append" -- repeatable, and each occurrence's own value
# is itself a space-separated atom list, so both accumulate
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --exclude "dev-libs/does-not-exist dev-libs/upgradepkg" dev-libs/upgradepkg
# dev-libs/upgradepkg-1.0 is already installed; nothing to do

# --json is pilot-specific (NOT a real emerge option): the whole
# resolved graph as one line of JSON instead of the plain-text lines
# above, including "requested" and "required_by" -- two fields no
# plain-text line has ever carried
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/newpkg
# {"entries":[{"category":"dev-libs","package":"newpkg","outcome":"new","version":"1.0","slot":"0","source":"ebuild","provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},"requested":true,"required_by":[],"blockers":[]}],"slot_conflicts":[],"changed_deps_report":[]}
# a binary candidate's own "source" is "binary", not "ebuild" -- entry_to_json
# used to hardcode the literal "ebuild" regardless of the entry's actual
# source, a real bug only caught once a binary candidate could resolve at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json --usepkg dev-libs/binaryonlypkg
# {"entries":[{"category":"dev-libs","package":"binaryonlypkg","outcome":"new","version":"1.0","slot":"0","source":"binary","provenance":{"mask_entry":null,"unmask_entry":null,"keyword_entry":null},"requested":true,"required_by":[],"blockers":[]}],"slot_conflicts":[],"changed_deps_report":[]}
# dev-libs/common is a diamond dependency (both shared-a and shared-b
# RDEPEND on it) -- required_by lists both owners, sorted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/diamond | python3 -c 'import json,sys; print(next(e["required_by"] for e in json.load(sys.stdin)["entries"] if e["package"] == "common"))'
# [{'category': 'dev-libs', 'package': 'shared-a'}, {'category': 'dev-libs', 'package': 'shared-b'}]
# --json's own state-change trace: dev-libs/maskedandunmaskedpkg is
# matched by a package.mask entry that an identical package.unmask
# entry then cancels -- provenance records both, not just that the
# package ended up visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/maskedandunmaskedpkg | python3 -c 'import json,sys; print(json.load(sys.stdin)["entries"][0]["provenance"])'
# {'mask_entry': 'dev-libs/maskedandunmaskedpkg', 'unmask_entry': 'dev-libs/maskedandunmaskedpkg', 'keyword_entry': None}
# dev-libs/wildcardkeywordpkg is ~amd64-only, visible only via the
# "*/wildcardkeywordpkg ~amd64" package.accept_keywords entry --
# provenance names that specific entry, not just "some entry helped"
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --json dev-libs/wildcardkeywordpkg | python3 -c 'import json,sys; print(json.load(sys.stdin)["entries"][0]["provenance"])'
# {'mask_entry': None, 'unmask_entry': None, 'keyword_entry': '*/wildcardkeywordpkg'}

# LICENSE/ACCEPT_LICENSE/package.license masking (PMS 7.3.2) is real and
# implemented: neither the fixture profile chain nor make.conf sets
# ACCEPT_LICENSE at all, so real portage's own "* -@EULA" default
# applies -- profiles/base/license_groups defines EULA="SomeEula", so
# this package's own LICENSE="SomeEula" is masked
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/eulapkg
# emerge: there are no ebuilds to satisfy "dev-libs/eulapkg".  (exit 1)
# a || any-of LICENSE group is visible via any one accepted alternative
# -- GPL-2 is accepted by the real default's own "*" token
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/anyoflicensepkg
# [ebuild  N    ] dev-libs/anyoflicensepkg-1.0
# package.license unmasks an otherwise EULA-masked package for that one
# package specifically (etc/portage/package.license accepts SomeEula
# just for this atom)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/packagelicensepkg
# [ebuild  N    ] dev-libs/packagelicensepkg-1.0

# cross-repo profile parents: the main repo's own profiles/default/parent
# names "overlay:crossrepo-parent" -- that overlay directory's own
# license_groups extends EULA with "CrossRepoNonfree", so this package
# is masked only once the overlay-level entry actually joins the chain
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/crossrepolicensepkg
# emerge: there are no ebuilds to satisfy "dev-libs/crossrepolicensepkg".  (exit 1)

# a USE-conditional LICENSE is visible with the flag off (the default);
# its sibling has package.use forcing the same flag on, activating the
# conditional and masking it
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/uselicensepkg
# [ebuild  N    ] dev-libs/uselicensepkg-1.0
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/uselicensepkgforced
# emerge: there are no ebuilds to satisfy "dev-libs/uselicensepkgforced".  (exit 1)

# PROPERTIES/ACCEPT_PROPERTIES/package.properties and RESTRICT/
# ACCEPT_RESTRICT/package.accept_restrict masking are real and
# implemented: real portage's own default (from cnf/make.globals) is
# "*", accepting everything, so a plain declared PROPERTIES is visible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/propertiespkg
# [ebuild  N    ] dev-libs/propertiespkg-1.0
# package.properties/package.accept_restrict can still narrow
# acceptance for one specific package via a "-token", even under the
# otherwise-permissive global "*" default
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/interactivepkg
# emerge: there are no ebuilds to satisfy "dev-libs/interactivepkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/restrictedpkg
# emerge: there are no ebuilds to satisfy "dev-libs/restrictedpkg".  (exit 1)

# package.use.mask/package.use.force are real and implemented, atom
# specificity included: a repo-level package.use.force wildcard entry
# force-enables "forceflag"; the base profile's own package.use.mask
# masks "maskflag" and "specflag" via a bare atom; the leaf profile's
# own package.use.mask has a more specific exact-version atom that
# un-masks "specflag" again -- the more specific entry wins regardless
# of chain order, so specflag stays off (un-masked but never enabled)
# while maskflag stays masked (nothing un-masks it)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v dev-libs/pkgusemaskforcepkg
# [ebuild  N     ] dev-libs/pkgusemaskforcepkg-1.0  USE="forceflag -maskflag -specflag"

# --nodeps/-O is real and implemented: withdeps' own RDEPEND (which
# would otherwise pull in newpkg and upgradepkg -- see the plain
# recursion example above) is never even read
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --nodeps dev-libs/withdeps
# [ebuild  N    ] dev-libs/withdeps-1.0

# --nodeps still shows a resolved package's own USE display with -v --
# it's -N's own foo?-gated dependency recursion that's suppressed, not
# the package's own metadata
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -O -v dev-libs/useflagpkg
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"

# --onlydeps/-o is real and implemented: the exact inverse of --nodeps --
# withdeps' own dependencies (newpkg, upgradepkg) print normally, but
# withdeps' own [ebuild N] line is suppressed. --update is added again
# just so upgradepkg's own dependency-level entry actually upgrades
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --update --onlydeps dev-libs/withdeps
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild     U ] dev-libs/upgradepkg-2.0 [1.0]

# --onlydeps on an already-installed atom: no dependencies were ever
# going to be walked (same as without --onlydeps), and its own "already
# installed" line is suppressed too -- so the whole run prints nothing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --onlydeps dev-libs/samepkg
# (no output)

# short-flag bundling: "-pv" decomposes into -p + -v, both real,
# implemented flags -- native argparse behavior for boolean short
# options, not something requiring emerge-specific parsing
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge -pv dev-libs/useflagpkg
# [ebuild  N     ] dev-libs/useflagpkg-1.0  USE="foo -missingflag"
# [ebuild  N    ] dev-libs/newpkg-1.0

# a bundled flag reports on the first out-of-scope character, left to
# right, exactly like a standalone occurrence of it would
/tmp/emerge -pd dev-libs/newpkg
# emerge (pilot v1): option "--debug" is a real emerge option, but is not
# implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N,
# --changed-use/-U, --nodeps/-O, --onlydeps/-o, --update/-u, --deep/-D,
# --exclude/-X, --deselect/-W, --with-bdeps, --changed-deps,
# --changed-slot, and --help/-h are implemented so far; see README.md)
# (exit 2)

# --help/-h is real and implemented: a short, honest, pilot-specific
# summary, not a port of real emerge's own (157-line, colorized,
# ~130-flag) help text -- wins unconditionally, regardless of position
# or what else accompanies it
/tmp/emerge --help
# emerge (pilot v1): command-line interface to the Rust porting pilot
# ...
# See README.md for this pilot's current scope.
/tmp/emerge --jobs --help          # --help wins even combined with other flags
/tmp/emerge -ph                    # ...and even bundled with other short flags

# CLI surface recognition: a real emerge option this pilot doesn't
# implement is named specifically, not lumped in with a typo
/tmp/emerge --jobs dev-libs/newpkg
# emerge (pilot v1): option "--jobs" is a real emerge option, but is not
# implemented in this pilot (only --pretend/-p, --verbose/-v, --newuse/-N,
# --changed-use/-U, --nodeps/-O, --onlydeps/-o, --update/-u, --deep/-D,
# --exclude/-X, --deselect/-W, --with-bdeps, --changed-deps,
# --changed-slot, and --help/-h are implemented so far; see README.md)
# (exit 2)

# a token that isn't a real emerge option/action at all gets a
# different message
/tmp/emerge --totally-fake-option dev-libs/newpkg
# emerge: unrecognized option "--totally-fake-option"

# or against the Python reference implementation directly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" \
    python3 python/emerge_pretend_reference.py --pretend dev-libs/newpkg

# --deselect/-W is a standalone action, not a --pretend modifier -- it
# needs no repos.conf/profile at all, only ROOT's own world file and
# vdb, so this uses a small throwaway ROOT instead of $FX
mkdir -p /tmp/deselect-demo-root/var/lib/portage /tmp/deselect-demo-root/var/db/pkg/dev-libs/foo-1.0
echo "dev-libs/foo" > /tmp/deselect-demo-root/var/lib/portage/world
echo "dev-libs" > /tmp/deselect-demo-root/var/db/pkg/dev-libs/foo-1.0/CATEGORY
echo "0" > /tmp/deselect-demo-root/var/db/pkg/dev-libs/foo-1.0/SLOT
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/foo
# >>> Would remove dev-libs/foo from "world" favorites file...

# a target with no matching world-file entry at all reports nothing,
# regardless of installed status
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/bar
# >>> No matching atoms found in "world" favorites file...

# an explicit-category target needs NO installed check at all -- real
# dep_expand() returns it unchanged, and action_deselect seeds
# expanded_atoms with it unconditionally -- so a world-listed but never-
# installed package is still discarded
echo "dev-libs/nevermerged" >> /tmp/deselect-demo-root/var/lib/portage/world
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/nevermerged
# >>> Would remove dev-libs/nevermerged from "world" favorites file...

# real Atom.intersects() is deliberately narrower than a real version-
# range check: an exact-version target matches an identical world entry,
# but ">=" against that same version doesn't, even though 1.0 would
# actually satisfy ">=dev-libs/versioned-1.0" under real dependency
# resolution -- the operator itself must match exactly here
echo "=dev-libs/versioned-1.0" >> /tmp/deselect-demo-root/var/lib/portage/world
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect ">=dev-libs/versioned-1.0"
# >>> No matching atoms found in "world" favorites file...
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect "=dev-libs/versioned-1.0"
# >>> Would remove =dev-libs/versioned-1.0 from "world" favorites file...

# --deselect "@name" matches the separate world_sets file by exact name
# (never expanded against its own set members) -- reported against
# "world_sets", not "world"
echo "@mytools" > /tmp/deselect-demo-root/var/lib/portage/world_sets
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect @mytools
# >>> Would remove @mytools from "world_sets" favorites file...

# a plain atom and a "@name" target discarded together are sorted into
# one combined list, not two separate "world" then "world_sets" blocks
ROOT="/tmp/deselect-demo-root" /tmp/emerge --pretend --deselect dev-libs/foo @mytools
# >>> Would remove @mytools from "world_sets" favorites file...
# >>> Would remove dev-libs/foo from "world" favorites file...

# --with-bdeps: withbdepspkg is already installed, DEPENDs on
# builddeponlypkg, BDEPENDs on hostdeponlypkg, RDEPENDs on newpkg --
# --deep's default (--with-bdeps=y/auto) walks all three. --noreplace
# keeps withbdepspkg itself AlreadyInstalled (see --noreplace/--selective
# further down) -- without it, a bare top-level atom recurses into its
# own dependencies regardless of --deep at all (real portage's own
# "selective" gap turns it into a plain reinstall, and any New/Upgrade/
# Reinstall entry's own dependencies are always walked), so --deep's own
# gating couldn't be demonstrated otherwise
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/builddeponlypkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/hostdeponlypkg-1.0

# --with-bdeps=n: DEPEND/BDEPEND are skipped, but RDEPEND is unaffected
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep --with-bdeps n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0

# --with-bdeps-auto n: with no explicit --with-bdeps given, changes the
# *default* from "auto" (walk all three) down to "n" -- same effect as
# --with-bdeps n above, but via the default instead of an explicit value
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep --with-bdeps-auto n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/newpkg-1.0

# an explicit --with-bdeps always wins over --with-bdeps-auto regardless
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --deep --with-bdeps y --with-bdeps-auto n dev-libs/withbdepspkg
# dev-libs/withbdepspkg-1.0 is already installed; nothing to do
# [ebuild  N    ] dev-libs/builddeponlypkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/hostdeponlypkg-1.0

# --changed-deps: changeddepspkg's own vdb-recorded RDEPEND (samepkg)
# differs from its current ebuild's own RDEPEND (newpkg) -- reinstalls
# and recurses into the CURRENT ebuild's own dependency, not the vdb's
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/changeddepspkg
# [ebuild   R   ] dev-libs/changeddepspkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --changed-deps ignores a libc-only dependency change (strip_libc_deps):
# libcnoisepkg's own vdb RDEPEND names sys-libs/glibc, its current
# ebuild names sys-libs/musl -- both are real virtual/libc providers per
# the fixture vdb's own virtual/libc entry, so no reinstall fires
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps dev-libs/libcnoisepkg
# dev-libs/libcnoisepkg-1.0 is already installed; nothing to do

# --changed-deps-report: same stale RDEPEND as the --changed-deps example
# above, but reported (to stderr) instead of reinstalled -- stdout still
# shows the ordinary "already installed" line. --noreplace keeps
# changeddepspkg itself AlreadyInstalled -- --changed-deps-report is NOT
# one of real portage's own eight "selective" triggers (unlike
# --changed-deps itself), so without --noreplace a bare top-level atom
# would report a plain reinstall instead (see --noreplace/--selective
# further down), muddying this specific demonstration. The " for $FX"
# suffix below only appears because ROOT isn't "/" here, like every
# other example in this section -- real portage's own condition exactly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --noreplace --changed-deps-report dev-libs/changeddepspkg
# dev-libs/changeddepspkg-1.0 is already installed; nothing to do
#
# !!! Detected ebuild dependency change(s) without revision bump:
#
#     dev-libs/changeddepspkg-1.0::testrepo for $FX
#
# NOTE: Refer to the following page for more information about dependency
#       change(s) without revision bump:
#
#           https://wiki.gentoo.org/wiki/Project:Portage/Changed_dependencies
#
#       In order to suppress reports about dependency changes, add
#       --changed-deps-report=n to the EMERGE_DEFAULT_OPTS variable in
#       '/etc/portage/make.conf'.
#
# HINT: In order to avoid problems involving changed dependencies, use the
#       --changed-deps option to automatically trigger rebuilds when changed
#       dependencies are detected. Refer to the emerge man page for more
#       information about this option.

# --changed-deps-report is silent once --changed-deps is also given --
# --changed-deps reinstalls normally, exactly as its own example above
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps-report --changed-deps dev-libs/changeddepspkg
# [ebuild   R   ] dev-libs/changeddepspkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --changed-slot: changedslotpkg's own vdb-recorded SLOT ("0") differs
# from its current ebuild's own SLOT ("0/2", an ABI-bump sub-slot change)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-slot dev-libs/changedslotpkg
# [ebuild   R   ] dev-libs/changedslotpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --changed-deps/--changed-slot are independent, freely-combinable
# reinstall triggers -- changedslotpkg's own vdb RDEPEND is *also* stale,
# so giving both prints both reasons on the same line
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --changed-deps --changed-slot dev-libs/changedslotpkg
# [ebuild   R   ] dev-libs/changedslotpkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --with-test-deps: withtestdeppkg's own RDEPEND is "dev-libs/newpkg
# test? ( dev-libs/testonlydep )" -- without the flag, only the
# unconditional dev-libs/newpkg is pulled in
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/withtestdeppkg
# [ebuild  N    ] dev-libs/withtestdeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --with-test-deps additionally pulls in the test?-gated dep too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --with-test-deps dev-libs/withtestdeppkg
# [ebuild  N    ] dev-libs/withtestdeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
# [ebuild  N    ] dev-libs/testonlydep-1.0

# ...but only for a top-level (depth 0) atom -- withtestdepconsumer's own
# RDEPEND reaches withtestdeppkg at depth 1, so testonlydep stays absent
# even with --with-test-deps given
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --with-test-deps dev-libs/withtestdepconsumer
# [ebuild  N    ] dev-libs/withtestdepconsumer-1.0
# [ebuild  N    ] dev-libs/withtestdeppkg-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0

# --usepkg/--usepkgonly: binaryonlypkg exists only in fixtures/pkgdir's
# own Packages index, no ebuild anywhere -- invisible without --usepkg
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/binaryonlypkg
# emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg dev-libs/binaryonlypkg
# [binary  N    ] dev-libs/binaryonlypkg-1.0

# --binpkg-respect-use defaults on under --usepkg: binaryusemismatchpkg's
# own binary entry has USE: (empty) but the fixture profile's own global
# USE would select "foo" over its IUSE="foo" -- mismatch, so the binary
# is rejected and the identical-version ebuild is used instead
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg dev-libs/binaryusemismatchpkg
# [ebuild  N    ] dev-libs/binaryusemismatchpkg-1.0

# ...but --binpkg-respect-use defaults OFF under --usepkgonly (no ebuild
# fallback to reject *to*) -- the same mismatched binary is now accepted
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkgonly dev-libs/binaryusemismatchpkg
# [binary  N    ] dev-libs/binaryusemismatchpkg-1.0

# --usepkg-exclude: drops a binary candidate from the pool entirely --
# binaryonlypkg has no ebuild to fall back to, so it disappears completely
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --usepkg-exclude dev-libs/binaryonlypkg dev-libs/binaryonlypkg
# emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".  (exit 1)
# --usepkg-include: the inverse -- once given, a binary candidate must
# match one of the listed atoms to stay eligible; a non-matching list
# rejects it exactly like --usepkg-exclude does
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --usepkg-include dev-libs/doesnotexist-anywhere dev-libs/binaryonlypkg
# emerge: there are no ebuilds to satisfy "dev-libs/binaryonlypkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --usepkg-include dev-libs/binaryonlypkg dev-libs/binaryonlypkg
# [binary  N    ] dev-libs/binaryonlypkg-1.0

# --rebuilt-binaries: rebuiltbinarypkg is installed at 1.0 (BUILD_TIME
# 1000), but the binary index's own copy at the same version has
# BUILD_TIME 2000 -- off by default (--selective avoids the unrelated
# "bare top-level atom always reinstalls" behavior muddying this)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective dev-libs/rebuiltbinarypkg
# dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do
# given explicitly, the differing BUILD_TIME triggers a reinstall
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective --rebuilt-binaries dev-libs/rebuiltbinarypkg
# [binary   R   ] dev-libs/rebuiltbinarypkg-1.0
# --rebuilt-binaries-timestamp narrows it to "newer AND at/above this
# cutoff" -- 2000 is below 3000, so no reinstall; 2000 clears 1500
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective --rebuilt-binaries --rebuilt-binaries-timestamp 3000 dev-libs/rebuiltbinarypkg
# dev-libs/rebuiltbinarypkg-1.0 is already installed; nothing to do
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg --selective --rebuilt-binaries --rebuilt-binaries-timestamp 1500 dev-libs/rebuiltbinarypkg
# [binary   R   ] dev-libs/rebuiltbinarypkg-1.0
# the real, non-obvious default: --usepkgonly + bare --deep + --update
# together auto-enable --rebuilt-binaries with no explicit flag at all
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkgonly --deep --update --selective dev-libs/rebuiltbinarypkg
# [binary   R   ] dev-libs/rebuiltbinarypkg-1.0

# --getbinpkg: dev-libs/remotebinpkg exists ONLY in the binhost's own
# Packages index (fixtures/binhost/Packages, reached via
# fixtures/etc/portage/binrepos.conf) -- no ebuild, no local $PKGDIR
# entry, so --usepkg alone leaves it invisible
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --usepkg dev-libs/remotebinpkg
# emerge: there are no ebuilds to satisfy "dev-libs/remotebinpkg".  (exit 1)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --getbinpkg dev-libs/remotebinpkg
# [binary  N g  ] dev-libs/remotebinpkg-1.0
# -v renders the real `g` bracket column, the ::repo from the index's own
# REPO field, and the binary's SIZE as both the ` N KiB` line suffix and
# the Size of downloads: counter
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v --getbinpkg dev-libs/remotebinpkg
# [binary  N g   ] dev-libs/remotebinpkg-1.0::gentoo  USE="-rbfoo" 560 KiB
#
# Total: 1 package (1 new, 1 binary), Size of downloads: 560 KiB
# :slot/sub_slot decoration applies to a [binary ... g] line too
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -v --getbinpkg dev-libs/remotebinslotpkg
# [binary  N g   ] dev-libs/remotebinslotpkg-1.0:2/1::gentoo  1024 KiB
#
# Total: 1 package (1 new, 1 binary), Size of downloads: 1024 KiB
# -G/--getbinpkgonly resolves it the same way (binary-only)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend -G dev-libs/remotebinpkg
# [binary  N g  ] dev-libs/remotebinpkg-1.0

# --newrepo: newrepopkg is installed with a vdb repository file
# recording "oldrepo", but the current best candidate for this exact
# version lives in "testrepo" instead -- off by default...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective dev-libs/newrepopkg
# dev-libs/newrepopkg-1.0 is already installed; nothing to do
# ...fires once given explicitly
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective --newrepo dev-libs/newrepopkg
# [ebuild   R   ] dev-libs/newrepopkg-1.0
# a vdb repository file that DOES match the current provider never
# triggers a reinstall
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective --newrepo dev-libs/samerepopkg
# dev-libs/samerepopkg-1.0 is already installed; nothing to do
# samepkg has no vdb repository file at all -- real portage's own
# "__unknown__" sentinel applies, which never matches a real repo name,
# so --newrepo fires here too even though nothing really changed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --selective --newrepo dev-libs/samepkg
# [ebuild   R   ] dev-libs/samepkg-1.0

# --buildpkgonly: dualdep is New, and both its DEPEND and RDEPEND on
# newpkg are also New -- real portage refuses to resolve this at all,
# since newpkg itself would also need building
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --buildpkgonly dev-libs/dualdep
# [ebuild  N    ] dev-libs/dualdep-1.0
# [ebuild  N    ] dev-libs/newpkg-1.0
#
# !!! --buildpkgonly requires all dependencies to be merged.
# !!! Cannot merge requested packages. Merge deps and try again.
# (exit 1)
# buildpkgonlysatisfied is also New, but its own RDEPEND (samepkg) is
# already installed -- nothing else needs building, so it resolves fine
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend --buildpkgonly dev-libs/buildpkgonlysatisfied
# [ebuild  N    ] dev-libs/buildpkgonlysatisfied-1.0

# downgrade vs upgrade: downgradepkg is installed at 2.0, but only 1.0 is
# visible in the tree -- a genuine downgrade, distinct from an upgrade,
# and shown even without --update since the installed 2.0 has no visible
# candidate of its own to satisfy real avoid_update's own shortcut
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/downgradepkg
# [ebuild     UD] dev-libs/downgradepkg-1.0 [2.0]

# avoid_update bug fix (see "What this proves" above for the full
# writeup): keywordmaskedpkg is installed at 2.0 (~amd64-only, no
# longer ACCEPT_KEYWORDS-visible); requested directly as a TOP-LEVEL
# atom, it's still a real downgrade (real portage's own later
# avoid_update block DOES require visibility there)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/keywordmaskedpkg
# [ebuild     UD] dev-libs/keywordmaskedpkg-1.0 [2.0]
# ...but reached only as a DEPENDENCY (needskeywordmasked's own
# RDEPEND), real portage's own EARLIER avoid_update return requires no
# visibility at all -- kept exactly as installed
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/needskeywordmasked
# [ebuild  N    ] dev-libs/needskeywordmasked-1.0
# same again, but with a real USE-dep on the dependency atom too
# (checked against the installed package's own real vdb USE, not the
# current tree's -- the actual real-world sys-libs/liburing:=[...] case)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/needskeywordmaskeduse
# [ebuild  N    ] dev-libs/needskeywordmaskeduse-1.0

# bug 640318: an installed dependency's USE-dep flag can be valid purely
# because the package was BUILT with it -- builtusedivergedep-1.0 has vdb
# USE="divergedflag" but its current ebuild dropped divergedflag from
# IUSE, so nothing in the tree satisfies [divergedflag]...
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend 'dev-libs/builtusedivergedep[divergedflag]'
# emerge: there are no ebuilds to satisfy "dev-libs/builtusedivergedep[divergedflag]".  (exit 1)
# ...but as a DEPENDENCY it's kept as installed (real _iuse_implicit_built)
PORTAGE_CONFIGROOT="$FX" ROOT="$FX" /tmp/emerge --pretend dev-libs/needsbuiltusediverge
# [ebuild  N    ] dev-libs/needsbuiltusediverge-1.0
```

Try the `ebuild` stub (still a dry-run placeholder -- no real phase
execution -- but it now recognizes real ebuild syntax; see
ebuild.rs/ebuild_options.rs):

```sh
ln -sf "$(realpath rust/target/release/portuale)" /tmp/ebuild

# a real, valid ebuild command -- still just a no-op stub
/tmp/ebuild foo-1.0.ebuild merge
# ebuild (pilot stub): dry-run only, no phase execution yet (see
# README.md)
# ebuild file: "foo-1.0.ebuild"
# commands: ["merge"]

# real invocations often chain several phases in one call
/tmp/ebuild foo-1.0.ebuild clean compile install

# a real, value-taking option (--color y) is recognized and its value
# correctly skipped, not misread as the ebuild file
/tmp/ebuild --color y foo-1.0.ebuild merge

# a command that isn't one of doebuild()'s own valid commands is
# rejected, not silently accepted like the original bare stub did
/tmp/ebuild foo-1.0.ebuild not-a-real-phase
# ebuild: "not-a-real-phase" is not one of the valid ebuild commands

# --help/-h are real and implemented too, winning unconditionally
# regardless of position or what else accompanies them
/tmp/ebuild --help
# ebuild (pilot stub): command-line interface to the Rust porting pilot
# ...
/tmp/ebuild --not-a-real-option -h
# (same help text -- wins even alongside an otherwise-invalid option)

# --version is deliberately NOT specially implemented (see
# ebuild_options.rs's own doc comment: real portage.VERSION is derived
# live via "git describe" for a from-source checkout, not a static
# string) -- it's still a real, recognized option though, just a no-op
# like the other five, unlike emerge's own CLI philosophy of rejecting
# every merely-recognized-but-unimplemented flag by name
/tmp/ebuild --version foo-1.0.ebuild merge
# ebuild (pilot stub): dry-run only, no phase execution yet ...
```

Run the contract suite (builds the Rust binaries itself; requires `cargo`
on `PATH`):

```sh
python3 -m pytest tests -v
```

Run the benchmark / regression gate:

```sh
# report speedup, no gating (uses the vendored real Gentoo tree snapshot)
python3 bench/run_benchmark.py --ops 200000

# fall back to synthetic seeded-random version strings instead
python3 bench/run_benchmark.py --ops 200000 --dataset synthetic

# CI-style: fail if speedup regressed vs. the recorded baseline
python3 bench/run_benchmark.py --check-baseline

# record a new baseline after an intentional, reviewed perf change
python3 bench/run_benchmark.py --update-baseline

# same gate, wrapped as a pytest for CI (skipped by default -- see
# tests/test_benchmark_gate.py)
PORTUALE_RUN_BENCHMARK=1 python3 -m pytest tests/test_benchmark_gate.py -v
```

Run the musl static-build smoke test (requires `podman` or `docker`; builds
a container image, so it needs network access for the Alpine base layer
and `apk add rust cargo` the first time):

```sh
bash musl/smoke_test.sh

# same gate, wrapped as a pytest for CI (skipped by default -- see
# tests/test_musl_smoke.py)
PORTUALE_RUN_MUSL_SMOKE=1 python3 -m pytest tests/test_musl_smoke.py -v -s
```

Real ebuild phase execution (task #54 -- see "What this proves" above for
the full writeup): `ebuild <file> install` runs the real `pretend` through
`install` phase sequence via an embedded `brush` shell, landing real files
under a real `${D}`. Uses `fixtures/repo/dev-libs/phasepkg`, whose
own `src_install` calls real `insinto`/`doins`:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild install
# (real phase output, including some known-nonfatal noise -- see
# ebuild_phases.rs's own "KNOWN, DOCUMENTED GAPS" -- then:)
#  * Final size of build directory: 0 KiB
#  * Final size of installed tree:  4 KiB
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt
# hello from phasepkg
```

Real merge/filesystem mutation (task #55 -- see "What this proves" above
for the full writeup): `ebuild <file> merge` runs the same real `install`
chain, then really runs `pkg_preinst`, really copies `${D}` into a real
`${ROOT}` and writes a real vdb entry, then really runs `pkg_postinst`.
Uses `fixtures/repo/dev-libs/mergepkg`, whose own `src_install`
calls real `insinto`/`doins`/`dosym`, and whose own `pkg_preinst`/
`pkg_postinst` each drop a marker file under `${T}` proving the real
ordering (preinst before the merge is visible, postinst only after):

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export ROOT="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild merge
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
cat "${ROOT}"/usr/share/mergepkg/hello.txt
# hello from mergepkg
readlink "${ROOT}"/usr/share/mergepkg/hello-link.txt
# hello.txt
cat "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0/CONTENTS
# dir /usr
# dir /usr/share
# dir /usr/share/mergepkg
# sym /usr/share/mergepkg/hello-link.txt -> hello.txt <mtime>
# obj /usr/share/mergepkg/hello.txt <md5> <mtime>
ls "${PORTAGE_TMPDIR}"/portage/dev-libs/mergepkg-1.0/temp/ | grep -E "preinst|postinst"
# postinst-ran-after-merge
# preinst-ran-before-merge
cat "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0/COUNTER
# 0 (a bare integer, no trailing newline -- the real vdb COUNTER format)
ls "${ROOT}"/var/db/pkg/dev-libs/
# mergepkg-1.0 (no leftover -MERGING-mergepkg-1.0 temp dir)
```

Real package removal: `ebuild <file> unmerge` (see "What this proves"
above for the full writeup) really deletes what `merge` just installed:

```sh
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/mergepkg/mergepkg-1.0.ebuild unmerge
# (real prerm/postrm phase output, then exit 0)
test -e "${ROOT}"/usr/share/mergepkg && echo "still there" || echo "gone"
# gone
test -e "${ROOT}"/var/db/pkg/dev-libs/mergepkg-1.0 && echo "still there" || echo "gone"
# gone
test -e "${ROOT}"/var/db/pkg/dev-libs && echo "still there" || echo "gone"
# gone (the now-empty category directory is removed too)
```

Real `CONFIG_PROTECT` (see "What this proves" above for the full
writeup): a locally-edited `/etc` file survives a merge. Uses
`fixtures/repo/dev-libs/configpkg`, whose own `src_install`
installs a *new* `/etc/configpkg.conf`:

```sh
mkdir -p "${ROOT}"/etc
echo "admin's own edits" > "${ROOT}"/etc/configpkg.conf
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild merge
cat "${ROOT}"/etc/configpkg.conf
# admin's own edits          <- untouched
cat "${ROOT}"/etc/._cfg0000_configpkg.conf
# new content from configpkg  <- diverted here instead
grep configpkg "${ROOT}"/var/db/pkg/dev-libs/configpkg-1.0/CONTENTS
# obj /etc/configpkg.conf <md5-of-the-new-content> <mtime>
```

(A real host's own ambient `CONFIG_PROTECT` -- if this pilot's dev/test
machine is itself a real Gentoo system -- will override the `/etc`
default shown above via the same env-var-sourced CLI boundary; export
`CONFIG_PROTECT=/etc` explicitly first if reproducing this by hand
outside the test suite, which never inherits host env vars this way.)

Real symlink `CONFIG_PROTECT`, `NOCONFMEM`, and `new_protect_filename`'s
own file-reuse logic (see "What this proves" above for the full writeup):
a locally-repointed `/etc` symlink survives a merge exactly like a
regular file does, and `NOCONFMEM` changes the real, visible outcome of a
repeat merge. Uses `fixtures/repo/dev-libs/configsympkg`, whose
own `src_install` installs a *new* `/etc/configsympkg.conf` symlink
pointing at `new-target`:

```sh
export CONFIG_PROTECT=/etc
mkdir -p "${ROOT}"/etc
ln -sfn admins-own-target "${ROOT}"/etc/configsympkg.conf
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/configsympkg/configsympkg-1.0.ebuild merge
readlink "${ROOT}"/etc/configsympkg.conf
# admins-own-target          <- untouched
readlink "${ROOT}"/etc/._cfg0000_configsympkg.conf
# new-target                  <- diverted here instead
grep configsympkg "${ROOT}"/var/db/pkg/dev-libs/configsympkg-1.0/CONTENTS
# sym /etc/configsympkg.conf -> new-target <mtime>
```

Re-merging `configpkg` (the regular-file example above) a second time
with content unchanged shows `NOCONFMEM`'s real, visible effect -- not a
second numbered file, but whether the logical path stays protected:

```sh
# Without NOCONFMEM: the already-offered update applies directly.
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild merge
cat "${ROOT}"/etc/configpkg.conf
# new content from configpkg  <- overwritten, no ._cfg0001_ spawned

# With NOCONFMEM: re-protected instead, reusing ._cfg0000_ (its content
# already matches -- new_protect_filename()'s own reuse logic) rather
# than spawning a ._cfg0001_ with identical content.
export NOCONFMEM=1
echo "admin's own edits" > "${ROOT}"/etc/configpkg.conf
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/configpkg/configpkg-1.0.ebuild merge
cat "${ROOT}"/etc/configpkg.conf
# admin's own edits           <- protected again, not overwritten
cat "${ROOT}"/etc/._cfg0000_configpkg.conf
# new content from configpkg  <- reused, no ._cfg0001_ spawned
unset NOCONFMEM
```

`--debug` (task #56 -- see "What this proves" above for the full
writeup): really exports `PORTAGE_DEBUG=1`, so real `bin/ebuild.sh`'s own
`set -x` guard fires -- real bash xtrace, not simulated. Uses
`fixtures/repo/dev-libs/debugpkg`, whose own `src_install` writes
the `PORTAGE_DEBUG` value it actually observed to `${T}/portage-debug-value.txt`:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/debugpkg/debugpkg-1.0.ebuild install --debug
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, PLUS real bash xtrace not present without --debug, e.g.:)
# ++ local needle=--allow-extra-vars
# ++ shift
# ...
# + set +x
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/debugpkg-1.0/temp/portage-debug-value.txt
# 1
```

Without `--debug`, the same run produces zero `+`/`++`-prefixed lines and
the marker file reads `0` instead.

Real `ebuild <file> package`/binpkg building (see "What this proves"
above for the full writeup): runs the real `install` chain, then really
invokes `bin/misc-functions.sh`'s own `__dyn_package`, producing a
genuine XPAK `.tbz2` and a real `Packages` index entry. Uses
`fixtures/repo/dev-libs/packagepkg` (`RDEPEND="dev-libs/samepkg"`,
so its own metadata round-trip is visible in the index):

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export ROOT="$(mktemp -d)"
export PKGDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/packagepkg/packagepkg-1.0.ebuild install package
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
file "${PKGDIR}"/dev-libs/packagepkg-1.0.tbz2
# ...: Gentoo binary package (XPAK)
cat "${PKGDIR}"/Packages
# TIMESTAMP: <unix time>
#
# CPV: dev-libs/packagepkg-1.0
# SLOT: 0
# KEYWORDS: amd64
# RDEPEND: dev-libs/samepkg
# BUILD_TIME: <unix time>
```

Real `emerge --buildpkgonly` execution (see "What this proves" above for
the full writeup): given *without* `--pretend`, actually builds a real
binary package. `dev-libs/packagepkg`'s own `RDEPEND` (`dev-libs/samepkg`)
already has an installed vdb entry under the fixture `ROOT`, so
`--buildpkgonly`'s own real depgraph gate has nothing to object to:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_CONFIGROOT="$(realpath fixtures)"
export ROOT="$(realpath fixtures)"
export PORTAGE_TMPDIR="$(mktemp -d)"
export PKGDIR="$(mktemp -d)"
ln -sf "$(realpath rust/target/release/portuale)" /tmp/emerge
/tmp/emerge --buildpkgonly dev-libs/packagepkg
# [ebuild  N    ] dev-libs/packagepkg-1.0
# >>> Building binary for dev-libs/packagepkg-1.0...
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
file "${PKGDIR}"/dev-libs/packagepkg-1.0.tbz2
# ...: Gentoo binary package (XPAK)
cat "${PKGDIR}"/Packages
# TIMESTAMP: <unix time>
#
# CPV: dev-libs/packagepkg-1.0
# SLOT: 0
# KEYWORDS: amd64
# RDEPEND: dev-libs/samepkg
# BUILD_TIME: <unix time>

# --pretend still suppresses the real build entirely, same atom:
/tmp/emerge --pretend --buildpkgonly dev-libs/packagepkg
# [ebuild  N    ] dev-libs/packagepkg-1.0
# (no ">>> Building binary" line, no real files written)

# BINPKG_FORMAT=gpkg builds the newer GLEP 78 format instead, via real,
# unmodified bin/gpkg-helper.py (BINPKG_COMPRESS=gzip keeps it off zstd):
export PKGDIR="$(mktemp -d)"
BINPKG_FORMAT=gpkg BINPKG_COMPRESS=gzip /tmp/emerge --buildpkgonly dev-libs/packagepkg
file "${PKGDIR}"/dev-libs/packagepkg-1.0.gpkg.tar
# ...: Gentoo GLEP 78 (GPKG) binary package for "packagepkg-1.0" using gzip compression
tar -tf "${PKGDIR}"/dev-libs/packagepkg-1.0.gpkg.tar
# packagepkg-1.0/gpkg-1
# packagepkg-1.0/metadata.tar.gz
# packagepkg-1.0/image.tar.gz
# packagepkg-1.0/Manifest
grep PATH "${PKGDIR}"/Packages
# PATH: dev-libs/packagepkg-1.0.gpkg.tar

# a real, nonempty SRC_URI with no Manifest entry at all is refused
# outright, rather than fetched unverified (dev-libs/fetchpkg has one
# and nothing else -- see "What this proves" above for why):
/tmp/emerge --buildpkgonly dev-libs/fetchpkg
# [ebuild  N    ] dev-libs/fetchpkg-1.0
# >>> Building binary for dev-libs/fetchpkg-1.0...
# emerge: dev-libs/fetchpkg-1.0: fetchpkg-1.0.tar.gz: no Manifest entry,
# cannot verify -- refusing to fetch unverifiable content
# (exit 1)
```

Real `SRC_URI` fetch (see "What this proves" above for the full
writeup): `dev-libs/verifiedfetchpkg`'s own real `SRC_URI` exercises
the full real grammar (arrow-rename plus a `test?` conditional group);
pre-seeding `DISTDIR` with a correctly-digested payload (matching the
fixture's own checked-in `Manifest`) fires the real already-verified
skip-fetch path, so this example needs no live network access at all:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export DISTDIR="$(mktemp -d)"
printf 'hello from verifiedfetchpkg\n' > "${DISTDIR}"/verifiedfetchpkg-1.0.tar.gz
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild install
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0 -- no network access, since the
# pre-seeded file already matches the fixture's own real Manifest digests)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/verifiedfetchpkg-1.0/temp/fetch-vars.txt
# A=verifiedfetchpkg-1.0.tar.gz
# AA=verifiedfetchpkg-1.0.tar.gz verifiedfetchpkg-tests-1.0.tar.gz

# RESTRICT=fetch: the plain SRC_URI is never downloaded. With the
# distfile absent, install fails fast (no network) with a
# "place it in DISTDIR by hand" pointer:
export PORTAGE_TMPDIR="$(mktemp -d)"; export DISTDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/fetchrestrictpkg/fetchrestrictpkg-1.0.ebuild install
# ebuild: fetchrestrictpkg-1.0.tar.gz: no working candidate mirror for
#   "https://example.invalid/frp-payload.bin" (RESTRICT=fetch bars
#   downloading it -- place a verified copy in <DISTDIR> by hand ...)
# ... exit 1
# With the file placed by hand (and Manifest-verified), it installs:
printf 'fetchrestrictpkg fixture distfile\n' > "${DISTDIR}"/fetchrestrictpkg-1.0.tar.gz
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/fetchrestrictpkg/fetchrestrictpkg-1.0.ebuild install
# ... exit 0
```

Real eclass `inherit()` support (see "What this proves" above for the
full writeup): `dev-libs/eclasspkg` really `inherit`s a real (if
fixture-only) eclass and calls a real function it defines:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild install
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/eclasspkg-1.0/temp/eclass-marker.txt
# hello from pilotcheck.eclass
```

The pipeline-deadlock fix (see "What this proves" above for the full
writeup): `dev-libs/bigeclasspkg` inherits `bigfixture.eclass` (~400
functions, deliberately large enough that real `bin/phase-functions.
sh`'s own post-phase `__save_ebuild_env | __filter_readonly_variables`
pipe exceeds the OS pipe buffer) and used to hang here indefinitely
before the fix:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    fixtures/repo/dev-libs/bigeclasspkg/bigeclasspkg-1.0.ebuild install
# (real phase output, including the same known-nonfatal noise as the
# task #54 example, then exit 0 -- promptly, not after a hang)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/bigeclasspkg-1.0/temp/bigfixture-marker.txt
# hello from bigfixture.eclass
```

`--shell bash|brush` (see "What this proves" above for the full
writeup): the default is now a real `bash` subprocess; `--shell brush`
opts into the embedded brush shell instead. `emerge` has the same flag,
and it now covers every real (non-`--pretend`) phase chain `emerge` can
drive — a source merge, a binpkg merge, the `pkg_prerm`/`pkg_postrm`
hooks under `-C`/`--unmerge`/`--depclean`/`--prune`/`--clean`/
`--rage-clean`, and `emerge --config`'s `pkg_config`.

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
rust/target/release/portuale ebuild --shell brush \
    fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild install
# (real phase output, then exit 0)
cat "${PORTAGE_TMPDIR}"/portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt
# hello from phasepkg

# --shell also selects the backend for emerge --config's pkg_config:
export PORTAGE_CONFIGROOT="$PWD/fixtures" ROOT="$(mktemp -d)"
mkdir -p "$ROOT/var/lib"
mkdir -p /tmp/pbin && ln -sf "$(realpath rust/target/release/portuale)" /tmp/pbin/ebuild
/tmp/pbin/ebuild --shell brush \
    fixtures/repo/dev-libs/emergeconfigpkg/emergeconfigpkg-1.0.ebuild merge
rust/target/release/portuale emerge --config --shell brush dev-libs/emergeconfigpkg
cat "$ROOT/var/lib/emergeconfigpkg.configured"   # configured 1.0
```

Full real merge against a live Gentoo tree (needs root, a
`3rdparty/portage/` checkout, and the real toolchain installed):

```sh
cd rust && cargo build --release && cd ../..
ln -sf "$(realpath rust/target/release/portuale)" rust/target/release/emerge
sudo rust/target/release/portuale emerge -v app-portage/eix
# fetch -> pretend -> setup -> unpack -> prepare -> configure -> compile
# -> test -> install -> vdb merge
# >>> app-portage/eix-0.36.9 merged.
eix --version            # 0.36.9
qlist -I app-portage/eix # app-portage/eix
```

Real `mirror://` resolution (see "What this proves" above for the full
writeup): a real `mirror://debian/...` `SRC_URI` entry on the real
system's own `gentoo` repo checkout, previously unfetchable:

```sh
cd rust && cargo build --release && cd ../..
export PORTAGE_TMPDIR="$(mktemp -d)"
export DISTDIR="$(mktemp -d)"
rust/target/release/portuale ebuild \
    /.gentoo/repos/gentoo/app-arch/unzip/unzip-6.0_p31.ebuild unpack
# (real phase output, then exit 0)
ls "${DISTDIR}"
# unzip60.tar.gz  unzip_6.0-31.debian.tar.xz
```

Applet listing and per-applet help:

```sh
rust/target/release/portuale            # or `portuale --help` / `-h`
# portuale: a multicall binary -- runs as `emerge` or `ebuild` ...
# Applets:
#    emerge   resolve dependencies and build, merge, or unmerge packages ...
#    ebuild   run individual build phases (unpack/compile/install/...) ...
rust/target/release/portuale frobnicate ; echo "exit=$?"
# portuale: unrecognized applet "frobnicate" ... -- run `portuale --help` ...
# exit=1

ln -sf "$(realpath rust/target/release/portuale)" rust/target/release/emerge
rust/target/release/emerge --help   # grouped tour: Actions / Dependency
                                    # and target selection / Autounmask /
                                    # Binary packages / Build scheduling /
                                    # Output / Pilot-only
```
