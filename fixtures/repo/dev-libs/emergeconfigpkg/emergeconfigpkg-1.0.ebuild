EAPI=8
DESCRIPTION="fixture package: emerge --config runs pkg_config from the vdb-saved env"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "payload ${PVR}" > "${T}/emergeconfigpkg.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/emergeconfigpkg.txt"
}

pkg_config() {
	mkdir -p "${EROOT}/var/lib" || die
	echo "configured ${PVR}" > "${EROOT}/var/lib/${PN}.configured" || die
}
