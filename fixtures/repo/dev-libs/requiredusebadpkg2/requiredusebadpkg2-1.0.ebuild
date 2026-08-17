EAPI=8
DESCRIPTION="fixture package: REQUIRED_USE 'baz? ( qux )' genuinely violated -- baz enabled globally, qux never declared/enabled anywhere. A SECOND, independent REQUIRED_USE violation (see dev-libs/requiredusebadpkg) so the two together can prove the whole graph walk continues past the first violation instead of aborting on it."
SLOT="0"
KEYWORDS="amd64"
IUSE="baz qux"
REQUIRED_USE="baz? ( qux )"
