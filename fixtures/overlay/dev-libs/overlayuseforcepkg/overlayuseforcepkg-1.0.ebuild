EAPI=8
DESCRIPTION="fixture package: its overlayforceflag?-gated dependency is pulled in only because the OVERLAY's own profiles/package.use.force forces the flag on -- unset by IUSE default and every other source"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="overlayforceflag? ( dev-libs/newpkg )"
