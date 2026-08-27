EAPI=8
DESCRIPTION="fixture: IUSE-aware _* wildcard USE_EXPAND expansion -- package.use linguas_* enables every linguas_X in this package's own IUSE; package.use.mask keeps linguas_en off"
SLOT="0"
KEYWORDS="amd64"
IUSE="linguas_en linguas_de"
RDEPEND="linguas_de? ( dev-libs/wildexpanddep ) linguas_en? ( dev-libs/wildexpandmasked )"
