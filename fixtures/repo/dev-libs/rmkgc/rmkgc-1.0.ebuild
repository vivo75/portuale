EAPI=8
DESCRIPTION="remote keep-going fixture C (independent; still merges under --keep-going)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "payload C" > "${T}/payload.txt" || die
	insinto /usr/share/rmkgc
	doins "${T}/payload.txt"
}
