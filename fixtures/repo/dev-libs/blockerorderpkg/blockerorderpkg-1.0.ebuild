EAPI=8
DESCRIPTION="fixture package: a strong (!!) blocker against dev-libs/samepkg PLUS a further dep (dev-libs/newpkg) -- proves blocker lines print after every package line, not inline"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="!!dev-libs/samepkg
	dev-libs/newpkg"
