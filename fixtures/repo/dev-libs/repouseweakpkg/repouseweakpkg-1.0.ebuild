EAPI=8
DESCRIPTION="fixture package: repo-level profiles/package.use enables repoweakflag, but the profile make.defaults disables it -- real configdict[repo] is weaker than configdict[defaults], so the flag ends up OFF"
SLOT="0"
KEYWORDS="amd64"
IUSE="repoweakflag"
RDEPEND="repoweakflag? ( dev-libs/newpkg )"
