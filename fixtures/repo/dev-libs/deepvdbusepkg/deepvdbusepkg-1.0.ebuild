EAPI=8
DESCRIPTION="fixture: installed -text-style; current ebuild has wantdep?( dep ), profile enables wantdep, but vdb USE does not"
SLOT="0"
KEYWORDS="amd64"
IUSE="wantdep"
RDEPEND="wantdep? ( dev-libs/deepvdbusetarget )"
