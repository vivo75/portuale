EAPI=8
DESCRIPTION="fixture: build-cycle with dev-libs/gpcycleb gated behind USE=x -- same shape as usecyclea, but also pulled in by dev-libs/gpcyclec's own hard [x] use-dep (the grandparent-conflict path)"
SLOT="0"
KEYWORDS="amd64"
IUSE="+x"
DEPEND="x? ( dev-libs/gpcycleb )"
