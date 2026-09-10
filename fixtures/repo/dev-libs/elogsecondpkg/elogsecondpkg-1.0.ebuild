EAPI=8
DESCRIPTION="fixture package: second elog emitter, for multi-package mail_summary"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo hi > "${T}/f" || die
	doins "${T}/f"
	elog "second package reporting in"
}
