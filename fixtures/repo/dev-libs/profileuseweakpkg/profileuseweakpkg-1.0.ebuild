EAPI=8
DESCRIPTION="fixture package: a profile level's own package.use enables profweakflag, but make.conf disables it -- real configdict[defaults] is weaker than configdict[conf], so the flag ends up OFF"
SLOT="0"
KEYWORDS="amd64"
IUSE="profweakflag"
RDEPEND="profweakflag? ( dev-libs/newpkg )"
