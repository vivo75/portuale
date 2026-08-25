EAPI=8
DESCRIPTION="fixture package: standalone pkg_config/pkg_info/pkg_prerm/pkg_postrm commands (real single-phase execution, no install/merge/unmerge chain)"
SLOT="0"
KEYWORDS="amd64"

pkg_config() {
	touch "${T}"/pkg-config-ran || die
}

pkg_info() {
	touch "${T}"/pkg-info-ran || die
}

pkg_prerm() {
	touch "${T}"/pkg-prerm-ran || die
}

pkg_postrm() {
	touch "${T}"/pkg-postrm-ran || die
}
