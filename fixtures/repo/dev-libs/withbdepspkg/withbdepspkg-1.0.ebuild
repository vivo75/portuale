EAPI=8
DESCRIPTION="fixture package: --deep+--with-bdeps=n -> already installed, DEPEND/BDEPEND skipped but RDEPEND still walked"
SLOT="0"
KEYWORDS="amd64"
DEPEND="dev-libs/builddeponlypkg"
BDEPEND="dev-libs/hostdeponlypkg"
RDEPEND="dev-libs/newpkg"
