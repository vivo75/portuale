EAPI=8
DESCRIPTION="fixture package: RDEPEND uses real USE-dep syntax, which used to be silently dropped by the graph BFS before USE deps were supported at all; the (+) defaults below let both dependency atoms genuinely satisfy USE-dep enforcement without depending on either target's own IUSE/profile state"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/newpkg[bar(+)] dev-libs/multislotpkg:1[baz(+)?]"
