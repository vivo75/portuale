EAPI=8
DESCRIPTION="fixture: IUSE +feat, RDEPENDs dev-libs/parentflipchildpkg[feat=] -- with feat ON the child must enable its own masked feat (impossible), so real --autounmask-use suggests flipping THIS package's feat OFF instead"
SLOT="0"
KEYWORDS="amd64"
IUSE="+feat"
RDEPEND="dev-libs/parentflipchildpkg[feat=]"
