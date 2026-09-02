EAPI=8
DESCRIPTION="fixture package: defines pkg_info(), so emerge --info <atom> shows a Package Settings block"
SLOT="0"
KEYWORDS="amd64"
IUSE="+alpha beta"

pkg_info() {
	einfo "pkginfopkg ${PVR}"
}
