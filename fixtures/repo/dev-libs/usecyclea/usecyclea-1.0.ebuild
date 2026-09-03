EAPI=8
DESCRIPTION="fixture: build-cycle with dev-libs/usecycleb gated behind USE=x -- disabling x breaks the cycle (_find_suggestions)"
SLOT="0"
KEYWORDS="amd64"
IUSE="+x"
DEPEND="x? ( dev-libs/usecycleb )"
