EAPI=8
DESCRIPTION="fixture package: --with-test-deps pulls in its own test?-gated RDEPEND, only for a top-level atom"
SLOT="0"
KEYWORDS="amd64"
IUSE="test"
RDEPEND="dev-libs/newpkg test? ( dev-libs/testonlydep )"
