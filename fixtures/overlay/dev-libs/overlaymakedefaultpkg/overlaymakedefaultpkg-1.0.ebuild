EAPI=8
DESCRIPTION="fixture package: the overlay's own profiles/make.defaults USE enables a flag, pulling in a dependency"
SLOT="0"
KEYWORDS="amd64"
IUSE="omdflag other"
RDEPEND="omdflag? ( dev-libs/newpkg )"
