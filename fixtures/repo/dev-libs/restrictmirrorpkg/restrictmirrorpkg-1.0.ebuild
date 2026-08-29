EAPI=8
DESCRIPTION="fixture package: RESTRICT=mirror -- verified-in-place so no live network is needed"
SRC_URI="https://example.invalid/payload.bin -> restrictmirrorpkg-1.0.tar.gz"
SLOT="0"
KEYWORDS="amd64"
RESTRICT="mirror"

src_install() {
	echo "A=${A}" > "${T}/fetch-vars.txt" || die
}
