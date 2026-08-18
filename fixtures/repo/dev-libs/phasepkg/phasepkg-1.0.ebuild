EAPI=8
DESCRIPTION="fixture package: real ebuild phase execution (task #54) -- a real, explicit src_install using real insinto/doins"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "hello from phasepkg" > "${T}/hello.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/hello.txt"
}
