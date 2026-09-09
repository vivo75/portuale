EAPI=8
DESCRIPTION="fixture: build-cycle with dev-libs/fucycleb gated behind USE=x -- same shape as gpcyclea, but the grandparent dev-libs/fucyclec constrains x only conditionally ([x?]), so the fix survives with followup_change"
SLOT="0"
KEYWORDS="amd64"
IUSE="+x"
DEPEND="x? ( dev-libs/fucycleb )"
