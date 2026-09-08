EAPI=8
DESCRIPTION="remote keep-going fixture A (no hooks; the test forges its index SIZE so the download fails)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "payload A" > "${T}/payload.txt" || die
	insinto /usr/share/rmkga
	doins "${T}/payload.txt"
}
