EAPI=8
DESCRIPTION="fixture package: RESTRICT=fetch -- the plain SRC_URI is never fetched; only a pre-placed, verified DISTDIR copy works"
SRC_URI="https://example.invalid/frp-payload.bin -> fetchrestrictpkg-1.0.tar.gz"
SLOT="0"
KEYWORDS="amd64"
RESTRICT="fetch"

src_install() {
	echo "A=${A}" > "${T}/fetch-vars.txt" || die
}
