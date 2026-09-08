EAPI=8
DESCRIPTION="remote keep-going fixture B (depends on A, so A's failure skips it)"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/rmkga"

src_install() {
	echo "payload B" > "${T}/payload.txt" || die
	insinto /usr/share/rmkgb
	doins "${T}/payload.txt"
}
