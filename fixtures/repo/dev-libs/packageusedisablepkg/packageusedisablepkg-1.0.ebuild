EAPI=8
DESCRIPTION="fixture package: package.use disables 'foo' (globally enabled by the fixture profile chain) for this package only, so its foo?-gated dependency is not pulled in"
SLOT="0"
KEYWORDS="amd64"
IUSE="foo"
RDEPEND="foo? ( dev-libs/newpkg )"
