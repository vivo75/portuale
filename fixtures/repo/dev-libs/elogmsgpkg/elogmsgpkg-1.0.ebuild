EAPI=8
DESCRIPTION="fixture package: emits elog/ewarn/einfo messages, for the elog echo module"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo hi > "${T}/f" || die
	doins "${T}/f"
	elog "this package needs manual configuration"
	elog "see /usr/share/doc for details"
}

pkg_postinst() {
	ewarn "a deprecated feature is still enabled"
	einfo "purely informational note (not echoed by default)"
}
