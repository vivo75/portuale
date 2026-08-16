EAPI=8
DESCRIPTION="fixture package: USE_EXPAND (VIDEO_CARDS=\"nvidia\" in profiles/base/make.defaults expands into the pseudo-USE flag video_cards_nvidia) drives a dependency exactly like a normal USE flag would"
SLOT="0"
KEYWORDS="amd64"
IUSE="video_cards_nvidia video_cards_amdgpu"
RDEPEND="video_cards_nvidia? ( dev-libs/newpkg ) video_cards_amdgpu? ( dev-libs/hiddendep )"
