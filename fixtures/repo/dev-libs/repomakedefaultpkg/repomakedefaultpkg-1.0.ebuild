EAPI=8
DESCRIPTION="fixture package: repo profiles/make.defaults USE enables a flag, pulling in a dependency"
SLOT="0"
KEYWORDS="amd64"
IUSE="repomakedefaultflag repo_amd64 other"
RDEPEND="repomakedefaultflag? ( dev-libs/newpkg )"
