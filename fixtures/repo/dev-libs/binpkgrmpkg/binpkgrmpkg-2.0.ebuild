EAPI=8
DESCRIPTION="fixture package: the full binpkg pkg_* phase-hook chain (setup/preinst/postinst/prerm/postrm)"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	echo "payload ${PVR}" > "${T}/payload-${PVR}.txt" || die
	insinto /usr/share/${PN}
	doins "${T}/payload-${PVR}.txt"
}

pkg_setup() {
	mkdir -p "${EROOT}/var/lib" || die
	echo "setup-${PVR}" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_preinst() {
	echo "preinst-${PVR}" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_postinst() {
	echo "postinst-${PVR}" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_prerm() {
	echo "prerm-${PVR}" >> "${EROOT}/var/lib/${PN}.log" || die
}

pkg_postrm() {
	echo "postrm-${PVR}" >> "${EROOT}/var/lib/${PN}.log" || die
}
