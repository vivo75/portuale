EAPI=8
DESCRIPTION="fixture package: KEYWORDS=amd64 (no ~) is genuinely stable under real KeywordsManager.isStable, so use.stable.force/package.use.stable.mask both apply -- see dev-libs/unstableusepkg for the contrasting ~amd64 case"
SLOT="0"
KEYWORDS="amd64"
IUSE="stableforceflag maskflag"
RDEPEND="stableforceflag? ( dev-libs/newpkg )"
