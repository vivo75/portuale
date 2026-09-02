EAPI=8
DESCRIPTION="fixture package: elogrmpkg 2.0, replaces 1.0 in-place so 1.0's pkg_prerm/pkg_postrm elog must fire"
SLOT="0"
KEYWORDS="amd64"

src_install() {
	insinto /usr/share/${PN}
	echo hi2 > "${T}/f" || die
	doins "${T}/f"
}
