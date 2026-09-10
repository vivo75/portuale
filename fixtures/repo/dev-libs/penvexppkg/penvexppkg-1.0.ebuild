EAPI=8
DESCRIPTION="fixture package: package.env env-file USE= with ${VAR} expansion"
SLOT="0"
KEYWORDS="amd64"
IUSE="penvexp-penvexpscope amd64-penvexp penvexpother"
RDEPEND="penvexp-penvexpscope? ( dev-libs/newpkg )"
