EAPI=8
DESCRIPTION="fixture package: real USE flags (not the old hardcoded empty set) drive use_reduce"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="foo? ( dev-libs/newpkg ) missingflag? ( dev-libs/hiddendep )"
