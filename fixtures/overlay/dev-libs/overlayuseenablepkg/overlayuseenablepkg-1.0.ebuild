EAPI=8
DESCRIPTION="fixture package: its overlayuseflag?-gated dependency is pulled in only because the OVERLAY's own profiles/package.use enables it -- the main repo has no such entry"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="overlayuseflag? ( dev-libs/newpkg )"
