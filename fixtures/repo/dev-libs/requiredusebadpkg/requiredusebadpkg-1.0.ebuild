EAPI=8
DESCRIPTION="fixture package: REQUIRED_USE 'foo? ( bar )' genuinely violated -- foo enabled globally, bar left off (no package.use entry for this package)"
SLOT="0"
KEYWORDS="amd64"
IUSE="foo bar"
REQUIRED_USE="foo? ( bar )"
