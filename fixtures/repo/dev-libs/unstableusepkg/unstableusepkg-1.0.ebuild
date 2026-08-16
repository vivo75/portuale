EAPI=8
DESCRIPTION="fixture package: KEYWORDS=~amd64 is genuinely NOT stable under real KeywordsManager.isStable (its own already-~-prefixed keyword changes nothing when re-unstabilized), so use.stable.force never applies here -- contrast with dev-libs/stableusepkg's own real amd64 keyword"
SLOT="0"
KEYWORDS="~amd64"
IUSE="stableforceflag maskflag"
RDEPEND="stableforceflag? ( dev-libs/newpkg )"
