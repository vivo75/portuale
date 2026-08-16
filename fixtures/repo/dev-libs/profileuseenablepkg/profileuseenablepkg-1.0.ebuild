EAPI=8
DESCRIPTION="fixture package: its foo?-gated dependency is pulled in only because a profile level's own package.use enables a flag that's off everywhere else"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="profileuseflag? ( dev-libs/newpkg )"
