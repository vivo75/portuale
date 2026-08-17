EAPI=8
DESCRIPTION="fixture package: USE_EXPAND_UNPREFIXED (ARCH=\"amd64\" in profiles/arch/amd64/make.defaults contributes the bare pseudo-USE flag amd64, no prefix at all) drives a dependency exactly like a normal USE flag would"
SLOT="0"
KEYWORDS="amd64"
IUSE="amd64 riscv"
RDEPEND="amd64? ( dev-libs/newpkg ) riscv? ( dev-libs/hiddendep )"
