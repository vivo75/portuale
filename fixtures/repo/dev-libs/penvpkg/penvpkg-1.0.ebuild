EAPI=8
DESCRIPTION="fixture package: package.env env-file USE= enables a flag, pulling in a dependency"
SLOT="0"
KEYWORDS="amd64"
IUSE="penvflag penvother"
RDEPEND="penvflag? ( dev-libs/newpkg )"
