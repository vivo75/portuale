EAPI=8
DESCRIPTION="fixture package: pkg_prerm/pkg_postrm emit elog/ewarn, for elog on unmerge"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo hi > "${T}/f" || die
	doins "${T}/f"
}

pkg_prerm() {
	ewarn "config files in /etc/${PN} are left behind"
}

pkg_postrm() {
	elog "run revdep-rebuild after removing this package"
}
