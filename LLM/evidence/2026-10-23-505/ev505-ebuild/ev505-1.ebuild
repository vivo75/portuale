EAPI=8
DESCRIPTION="env probe"
HOMEPAGE="https://example.invalid"
LICENSE="GPL-2"
SLOT="0"
IUSE="test"
KEYWORDS="amd64"
S="${WORKDIR}"
src_install() {
	echo hi > "${T}/hi"
	dodoc "${T}/hi"
	exeinto /usr/bin
}
