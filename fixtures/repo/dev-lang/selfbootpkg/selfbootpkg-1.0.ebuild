EAPI=8
DESCRIPTION="fixture: BDEPEND || ( >=self-2 self-bootstrap ) -- the first branch resolves only to this package (circular), so a fresh resolve must pick the bootstrap branch (go-bootstrap pattern, L0 finding D)"
SLOT="0"
KEYWORDS="amd64"
BDEPEND="|| ( >=dev-lang/selfbootpkg-2.0 dev-lang/selfbootbootstrap )"
