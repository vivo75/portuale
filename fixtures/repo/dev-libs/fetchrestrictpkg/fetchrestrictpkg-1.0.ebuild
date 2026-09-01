EAPI=8
DESCRIPTION="fixture package: RESTRICT=fetch -- the plain SRC_URI is never fetched; only a pre-placed, verified DISTDIR copy works"
SRC_URI="https://example.invalid/frp-payload.bin -> fetchrestrictpkg-1.0.tar.gz"
SLOT="0"
KEYWORDS="amd64"
RESTRICT="fetch"

pkg_nofetch() {
	elog "Please download fetchrestrictpkg-1.0.tar.gz from https://example.org/downloads/"
	elog "and place it in ${DISTDIR}"
}

# The distfile is a digest-verified stand-in, not a real archive -- this
# fixture exercises RESTRICT=fetch + the verified-skip-fetch path, not
# unpacking. Skip the EAPI-8 default src_unpack (which would `tar` the
# stand-in and fail).
src_unpack() { :; }

src_install() {
	echo "A=${A}" > "${T}/fetch-vars.txt" || die
}
