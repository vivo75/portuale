EAPI=8
DESCRIPTION="fixture package: standalone pkg_config/pkg_info commands (real single-phase execution, no install/merge chain)"
SLOT="0"
KEYWORDS="amd64"

pkg_config() {
	touch "${T}"/pkg-config-ran || die
}

pkg_info() {
	touch "${T}"/pkg-info-ran || die
}
